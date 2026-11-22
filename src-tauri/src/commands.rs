//! Tauri IPC 命令。
//!
//! 每个 `#[tauri::command]` 对应前端 `src/lib/ipc.ts` 里的一个函数，
//! 两边名字必须一致。命令只做参数整理和错误包装，
//! 真正的逻辑都在 config / link / skills / skillscli / git 里。

use crate::config::Config;
use crate::types::{
    CollectPlan, GitStatus, ManagedSkill, Mapping, OrphanSkill, RunResult, Skill, SkillMappingView,
};
use crate::{git, link, skills, skillscli};

/// 读取全部映射及其健康状态。
///
/// 纯读操作，可以安全地在界面上频繁调用。
#[tauri::command]
pub fn list_mappings() -> Result<Vec<Mapping>, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let resolved = cfg.resolved_mappings();
    Ok(resolved.iter().map(link::inspect).collect())
}

/// 同步全部映射。`dry_run` 为 true 时只报告将要做什么，不写盘。
///
/// 异步执行：建 9 个链接虽然快，但每个都涉及文件系统 IO 和可能的
/// 备份操作，放在 IPC 线程上仍会卡界面。
#[tauri::command]
pub async fn sync_mappings(dry_run: bool) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || sync_mappings_inner(dry_run))
        .await
        .map_err(|e| format!("同步失败：{e}"))?
}

fn sync_mappings_inner(dry_run: bool) -> Result<RunResult, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let resolved = cfg.resolved_mappings();

    let mut log = Vec::new();
    let mut synced = 0;
    let mut skipped = 0;

    // 先刷新合成文件，再建链接。
    //
    // 顺序不能反：`link::sync_one` 会先检查 `m.source.exists()`，
    // 而合成文件是派生物，可能还没生成或已经落后。先合成能保证
    // 链接指向的内容就是最新拼出来的那一份。
    //
    // dry_run 时不写盘，但仍要报告哪些合成文件会重建 ——
    // 否则用户看到「预览无变化」，真跑却改了文件。
    for outcome in crate::doc::compose_all(!dry_run) {
        match outcome.action.as_str() {
            "skipped" if dry_run => {
                log.push(format!("[预览] 文档合成 {}：{}", outcome.agent, outcome.message))
            }
            "created" | "updated" | "removed" => log.push(format!(
                "[文档] {}：{}",
                outcome.agent, outcome.message
            )),
            "error" => log.push(format!("[失败] 文档合成 {}：{}", outcome.agent, outcome.message)),
            _ => {}
        }
    }

    for m in &resolved {
        if m.disabled {
            log.push(format!("[跳过] {}: 已取消映射", m.label));
            skipped += 1;
            continue;
        }

        if !m.config_dir.exists() {
            log.push(format!("[跳过] {}: 配置目录不存在", m.label));
            skipped += 1;
            continue;
        }

        if dry_run {
            log.push(format!(
                "[预览] {}: {} → {}",
                m.label,
                m.source.display(),
                m.target.display()
            ));
            continue;
        }

        match link::sync_one(m) {
            Ok((kind, note)) => {
                log.push(format!("[{}] {}: {kind:?} — {note}", m.label, m.target.display()));
                synced += 1;
            }
            Err(e) => {
                log.push(format!("[失败] {}: {e}", m.label));
            }
        }
    }

    if !dry_run {
        log.push(format!("完成：{synced} 个已同步，{skipped} 个跳过"));
    }

    Ok(RunResult::success(log.join("\n")))
}

/// 只修复指定 agent 的映射。异步，理由同上。
#[tauri::command]
pub async fn repair_mapping(agent: String) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || repair_mapping_inner(agent))
        .await
        .map_err(|e| format!("修复失败：{e}"))?
}

fn repair_mapping_inner(agent: String) -> Result<RunResult, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let resolved = cfg.resolved_mappings();

    let Some(m) = resolved.iter().find(|m| m.agent == agent) else {
        return Ok(RunResult::failure(1, format!("找不到 agent：{agent}")));
    };

    match link::sync_one(m) {
        Ok((kind, note)) => Ok(RunResult::success(format!(
            "[{}] {kind:?} — {note}",
            m.label
        ))),
        Err(e) => Ok(RunResult::failure(1, format!("[{}] {e}", m.label))),
    }
}

/// 取消某个 agent 的映射：删掉目标位置的链接或副本，并记下「已取消」。
///
/// 为什么必须记下来：只删链接的话，下一次「同步全部」会立刻把它建回来，
/// 用户会以为按钮没生效。标记写在配置文件里，同步时跳过。
///
/// 删除动作本身遵守 [`link::remove_one`] 的边界：母版一律不碰，
/// 内容被改过的目标移进备份目录而不是直接删。
#[tauri::command]
pub async fn unmap(agent: String) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || unmap_inner(agent))
        .await
        .map_err(|e| format!("取消映射失败：{e}"))?
}

fn unmap_inner(agent: String) -> Result<RunResult, String> {
    let mut cfg = Config::load().map_err(|e| e.to_string())?;
    let resolved = cfg.resolved_mappings();

    let Some(m) = resolved.iter().find(|m| m.agent == agent) else {
        return Ok(RunResult::failure(1, format!("找不到 agent：{agent}")));
    };

    // 先删文件再写配置。反过来会出现「标记已取消但链接还在」，
    // 而且文件删除失败时用户还能重试。
    let note = match link::remove_one(m) {
        Ok(note) => note,
        Err(e) => return Ok(RunResult::failure(1, format!("[{}] {e}", m.label))),
    };

    cfg.set_mapping_disabled(&agent, true)
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;

    Ok(RunResult::success(format!(
        "[{}] 已取消映射：{note}\n下次同步会跳过，点「恢复映射」可以重新建立",
        m.label
    )))
}

/// 恢复某个被取消的映射：清掉「已取消」标记并立刻重建链接。
#[tauri::command]
pub async fn remap(agent: String) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || remap_inner(agent))
        .await
        .map_err(|e| format!("恢复映射失败：{e}"))?
}

fn remap_inner(agent: String) -> Result<RunResult, String> {
    let mut cfg = Config::load().map_err(|e| e.to_string())?;

    cfg.set_mapping_disabled(&agent, false)
        .map_err(|e| e.to_string())?;
    cfg.save().map_err(|e| e.to_string())?;

    // 重新读一遍，让 resolved 里的 disabled 已经是 false
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let resolved = cfg.resolved_mappings();
    let Some(m) = resolved.iter().find(|m| m.agent == agent) else {
        return Ok(RunResult::failure(1, format!("找不到 agent：{agent}")));
    };

    match link::sync_one(m) {
        Ok((kind, note)) => Ok(RunResult::success(format!(
            "[{}] 已恢复映射：{kind:?} — {note}",
            m.label
        ))),
        Err(e) => Ok(RunResult::failure(1, format!("[{}] {e}", m.label))),
    }
}

/// 读取母版 AGENTS.md 的内容。
///
/// 文件不存在不当作错误 —— 首次使用时本来就没有，界面上要用
/// 「创建」而不是「读取失败」来引导用户。
#[tauri::command]
pub fn read_doc() -> Result<crate::doc::DocContent, String> {
    crate::doc::read().map_err(|e| e.to_string())
}

/// 覆盖写入母版 AGENTS.md。
#[tauri::command]
pub fn save_doc(content: String) -> Result<RunResult, String> {
    Ok(crate::doc::write(&content))
}

/// 创建母版文件。已存在时拒绝，避免误覆盖。
#[tauri::command]
pub fn create_doc(content: String) -> Result<RunResult, String> {
    Ok(crate::doc::create(&content))
}

/// 列出每个 agent 的指令文档现状：有无专用片段、合成是否落后。
///
/// 纯只读，界面可以频繁调用。
#[tauri::command]
pub fn list_agent_docs() -> Result<Vec<crate::types::AgentDoc>, String> {
    Ok(crate::doc::list_agent_docs())
}

/// 读某个 agent 的专用片段。没有片段时返回 `exists=false`。
#[tauri::command]
pub fn read_agent_doc(agent: String) -> Result<crate::doc::DocContent, String> {
    crate::doc::specific_read(&agent).map_err(|e| e.to_string())
}

/// 写某个 agent 的专用片段。内容为空表示取消这个 agent 的专用内容。
#[tauri::command]
pub fn save_agent_doc(agent: String, content: String) -> Result<RunResult, String> {
    Ok(crate::doc::specific_write(&agent, &content))
}

/// 重建所有 agent 的合成指令文档。`dry_run` 为真时只报告。
///
/// 独立成命令是为了让用户能单独刷新文档而不碰链接 —— 比如只想看看
/// 合成后有多大。`sync` 内部也会先跑一遍这个。
#[tauri::command]
pub fn compose_agent_docs(dry_run: bool) -> Result<RunResult, String> {
    let outcomes = crate::doc::compose_all(!dry_run);
    if outcomes.is_empty() {
        return Ok(RunResult::success("没有需要合成的文档".to_string()));
    }
    let lines: Vec<String> = outcomes
        .iter()
        .map(|o| format!("[{}] {}：{}", o.action, o.agent, o.message))
        .collect();
    Ok(RunResult::success(lines.join("\n")))
}

/// 列出中心库里的所有 skill。
///
/// 公共 skill 和专用 skill 一起返回：`agents` 为空表示公共，
/// 非空表示只给那几个 agent 用。分开查询会让界面要在两处拼一张表。
#[tauri::command]
pub fn list_skills() -> Result<Vec<Skill>, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    skills::list_skills(&cfg.skills).map_err(|e| e.to_string())
}

/// 覆盖某个 skill 的标签。
///
/// 标签写在配置文件里，不碰 skill 目录本身，避免影响 `npx skills update`。
#[tauri::command]
pub fn set_skill_tags(name: String, tags: Vec<String>) -> Result<RunResult, String> {
    let mut cfg = Config::load().map_err(|e| e.to_string())?;

    // 去掉空标签和两端空白，并去重
    let mut clean: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().to_string();
        if !t.is_empty() && !clean.contains(&t) {
            clean.push(t);
        }
    }

    if clean.is_empty() {
        cfg.skills.tags.remove(&name);
    } else {
        cfg.skills.tags.insert(name.clone(), clean);
    }

    cfg.save().map_err(|e| e.to_string())?;
    Ok(RunResult::success(format!("已更新 {name} 的标签")))
}

/// 扫描散落在各 agent 目录里的 skill。
#[tauri::command]
pub fn scan_orphans() -> Result<Vec<OrphanSkill>, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    skills::scan_orphans(&cfg.skills.tags).map_err(|e| e.to_string())
}

/// 执行收集。`apply` 为 false 时只返回计划。
#[tauri::command]
pub fn collect_skills(apply: bool) -> Result<CollectPlan, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    let orphans = skills::scan_orphans(&cfg.skills.tags).map_err(|e| e.to_string())?;
    let plan = skills::build_plan(orphans);

    if apply {
        // 应用结果通过日志返回，但前端契约是 CollectPlan，
        // 所以这里把处理日志交给调用方用 scan 后的状态体现
        skills::apply_plan(&plan).map_err(|e| e.to_string())?;
    }

    Ok(plan)
}

/// 列出全部专用 skill 记录及其链接现场状态。
///
/// 纯只读。界面用它渲染专用 skill 管理区，CLI 的
/// `skills private list --json` 也用它。
#[tauri::command]
pub fn list_skill_mappings() -> Result<Vec<SkillMappingView>, String> {
    let cfg = Config::load().map_err(|e| e.to_string())?;
    Ok(cfg
        .skills
        .mappings
        .iter()
        .map(skills::inspect_private)
        .collect())
}

/// 重建专用 skill 的链接。`dry_run` 为真时只报告会做什么。
#[tauri::command]
pub fn sync_skill_mappings(dry_run: bool) -> Result<Vec<String>, String> {
    skills::sync_private_links(dry_run).map_err(|e| e.to_string())
}

/// 取消一条专用记录。
///
/// `to_shared` 为真时本体移回公共库；为假时先备份到
/// `~/.agents/backup/` 再删。两种都会删掉各 agent 目录里的链接。
#[tauri::command]
pub fn remove_skill_mapping(skill: String, to_shared: bool) -> Result<RunResult, String> {
    skills::remove_private(&skill, to_shared)
        .map(RunResult::success)
        .map_err(|e| e.to_string())
}

/// 对一条散落 skill 执行用户选定的处置。
///
/// 与 `collect_skills` 的区别：那个按关系套固定策略批量处理，
/// 这个由用户在散落清单 dialog 里逐条点。可能移动或删除目录，
/// 所以走 spawn_blocking，避免磁盘操作卡住界面。
#[tauri::command]
pub async fn apply_orphan_action(
    path: String,
    action: crate::types::OrphanAction,
) -> Result<crate::types::OrphanActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(crate::skills::apply_action(&path, action))
    })
    .await
    .map_err(|e| format!("处置失败：{e}"))?
}

/// 读取 lock 文件里的受管 skill 清单。
#[tauri::command]
pub fn list_managed() -> Result<Vec<ManagedSkill>, String> {
    skillscli::list_managed().map_err(|e| e.to_string())
}

/// 检查更新。退出码 1 表示有更新可用，不是失败。
///
/// **异步命令**：`npx` 要联网拉仓库，可能要十几秒。Tauri 的同步命令
/// 在 IPC 线程上执行，会让窗口无响应，所以耗时操作必须标 `async`，
/// Tauri 会把它丢到线程池，界面保持可交互。
#[tauri::command]
pub async fn check_updates() -> Result<RunResult, String> {
    // spawn_blocking 而不是直接调用：clap/Command 是阻塞 IO，
    // 放在 async 上下文里会占住 executor 的线程
    tauri::async_runtime::spawn_blocking(skillscli::check_updates)
        .await
        .map_err(|e| format!("检查更新失败：{e}"))
}

/// 更新指定 skill，不传则更新全部。
///
/// 同上，`npx skills update` 会下载文件，必须异步执行，
/// 否则窗口会出现「未响应」。
#[tauri::command]
pub async fn update_skills(names: Vec<String>) -> Result<RunResult, String> {
    tauri::async_runtime::spawn_blocking(move || skillscli::update(&names))
        .await
        .map_err(|e| format!("更新失败：{e}"))
}

/// 读取 `~/.agents` 的 git 状态。
#[tauri::command]
pub fn git_status() -> Result<GitStatus, String> {
    Ok(git::status())
}

/// 按指定路径提交。
#[tauri::command]
pub fn git_commit(paths: Vec<String>, message: String) -> Result<RunResult, String> {
    Ok(git::commit(&paths, &message))
}

/// 推送当前分支。
#[tauri::command]
pub fn git_push() -> Result<RunResult, String> {
    Ok(git::push())
}

/// API key 体检。纯只读，不写任何文件。
///
/// 返回的数据里只有指纹和末尾 4 位，没有密钥原文 ——
/// 原文连序列化的机会都没有，在 credentials 模块里就被丢掉了。
#[tauri::command]
pub fn inspect_credentials() -> Result<crate::types::CredentialOverview, String> {
    Ok(crate::credentials::inspect())
}

// ---- provider 统一管理 ----

/// 列出母版里的 provider。key 原文一并返回，供编辑框显示当前值。
#[tauri::command]
pub async fn list_providers() -> Result<Vec<crate::types::ProviderView>, String> {
    tauri::async_runtime::spawn_blocking(list_providers_inner)
        .await
        .map_err(|e| format!("list_providers 任务失败：{e}"))?
}

fn list_providers_inner() -> Result<Vec<crate::types::ProviderView>, String> {
    let list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .map(|p| {
            let tail: String = p
                .api_key
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            crate::types::ProviderView {
                id: p.id,
                name: p.name,
                base_url: p.base_url,
                kind: p.kind.as_str().to_string(),
                kind_label: p.kind.label().to_string(),
                agent_kinds: p
                    .agent_kinds
                    .iter()
                    .map(|(a, k)| (a.clone(), k.as_str().to_string()))
                    .collect(),
                models: p.models,
                agents: p.agents,
                claude_model_map: p.claude_model_map,
                codex_requires_openai_auth: p.codex_requires_openai_auth,
                codex_model: p.codex_model,
                note: p.note,
                key_len: p.api_key.len(),
                key_tail: if p.api_key.len() >= 4 { tail } else { String::new() },
                api_key: p.api_key,
            }
        })
        .collect())
}

/// 保存一条 provider（新增或更新）。
///
/// `api_key` 传空串表示"不修改现有 key"，这样界面可以在不改 key 的
/// 情况下编辑其他字段。
///
/// `models` 接收的是完整模型规格列表（含上下文窗口、模态等元数据）。
/// 前端可以把只有 id 的模型传上来，母版会按「纯 id」形态写回去。
///
/// `agent_kinds` 是按 agent 覆盖的协议。同一个端点在不同 agent 里
/// 用的协议可能不同（实测 opencode.ai/zen/go 在 ZCode 里是 anthropic、
/// 在 Pi 里是 openai-responses），所以协议不能只存在 provider 上。
/// 这里传空表示「全部用 `kind` 这个默认值」。
#[tauri::command]
pub fn save_provider(
    id: String,
    name: String,
    base_url: String,
    kind: String,
    api_key: String,
    models: Vec<crate::providers::model::ModelSpec>,
    agents: Vec<String>,
    agent_kinds: std::collections::BTreeMap<String, String>,
    // Codex 的 requires_openai_auth 覆盖值：null 表示交给注入器自动判断。
    // 前端每次保存都会把它原样带回，否则编辑其他字段时会把这个设置洗掉。
    codex_requires_openai_auth: Option<bool>,
    // Codex 用哪个模型。null 表示沿用 models 的第一个。
    // 和上面的开关一样，前端每次保存都要原样带回。
    codex_model: Option<String>,
    note: String,
) -> Result<RunResult, String> {
    let mut list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;

    let kind_val = crate::providers::model::ProviderKind::parse(&kind);
    let kinds: std::collections::BTreeMap<String, crate::providers::model::ProviderKind> = agent_kinds
        .into_iter()
        .map(|(a, k)| {
            (
                a,
                crate::providers::model::ProviderKind::parse(&k),
            )
        })
        .collect();

    match list.iter_mut().find(|p| p.id == id) {
        Some(existing) => {
            existing.name = name;
            existing.base_url = base_url;
            existing.kind = kind_val;
            existing.agent_kinds = kinds;
            // 空 key 表示不修改
            if !api_key.trim().is_empty() {
                existing.api_key = api_key;
            }
            existing.models = models;
            existing.agents = agents;
            existing.codex_requires_openai_auth = codex_requires_openai_auth;
            // 只接受还存在于 models 里的名字，避免编辑模型列表后留下悬空引用
            existing.codex_model = codex_model.filter(|m| {
                !m.trim().is_empty() && existing.models.iter().any(|x| x.id == *m)
            });
            // 编辑表单不改 Claude 映射；映射由分配弹窗单独写
            existing.note = note;
        }
        None => {
            if api_key.trim().is_empty() {
                return Ok(RunResult::failure(1, "新增 provider 必须提供 API key"));
            }
            // 先筛出有效的模型引用，再构造结构体 —— models 在字面量里会被 move
            let picked = codex_model.filter(|m| {
                !m.trim().is_empty() && models.iter().any(|x| x.id == *m)
            });
            list.push(crate::providers::model::Provider {
                id: id.clone(),
                name,
                base_url,
                kind: kind_val,
                agent_kinds: kinds,
                api_key,
                models,
                agents,
                claude_model_map: crate::types::ClaudeModelMap::default(),
                codex_requires_openai_auth,
                codex_model: picked,
                note,
            });
        }
    }

    crate::providers::store::save(&list).map_err(|e| e.to_string())?;

    let mut msg = format!("已保存 provider {id}");
    if let Some(warn) = crate::providers::store::check_gitignore() {
        msg.push_str("\n\n[警告] ");
        msg.push_str(&warn);
    }
    Ok(RunResult::success(msg))
}

/// 从母版删除一条 provider。
#[tauri::command]
pub fn delete_provider(id: String) -> Result<RunResult, String> {
    let mut list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() == before {
        return Ok(RunResult::failure(1, format!("母版里没有 {id}")));
    }
    crate::providers::store::save(&list).map_err(|e| e.to_string())?;
    Ok(RunResult::success(format!("已从母版移除 {id}")))
}

/// 对比母版与各 agent 现状。**只读，不写盘。**
#[tauri::command]
pub fn diff_providers() -> Result<crate::types::ProviderDiffReport, String> {
    let report = crate::providers::diff::run().map_err(|e| e.to_string())?;
    Ok(crate::types::ProviderDiffReport {
        lines: report
            .lines
            .into_iter()
            .map(|l| crate::types::ProviderDiffLine {
                agent: l.agent,
                path: l.path,
                action: match l.action {
                    crate::providers::model::DiffAction::Add => "add".into(),
                    crate::providers::model::DiffAction::Update => "update".into(),
                    crate::providers::model::DiffAction::Remove => "remove".into(),
                    crate::providers::model::DiffAction::Unchanged => "unchanged".into(),
                    crate::providers::model::DiffAction::Conflict => "conflict".into(),
                },
                target: l.target,
                detail: l.detail,
            })
            .collect(),
        conflicts: report.conflicts,
        agent_count: report.agent_count,
        changed_files: report.changed_files,
    })
}

/// 从各 agent 现有配置回填模型元数据到母版。
///
/// 只补母版里空着的字段，不覆盖已有值。`dry_run` 时只算报告不写盘。
#[tauri::command]
pub fn backfill_models(dry_run: bool) -> Result<crate::types::BackfillReport, String> {
    crate::providers::backfill::run(dry_run).map_err(|e| e.to_string())
}

/// 读取 provider 分配矩阵：provider 池 × agent。
///
/// 和 `list_providers` 返回同一份数据，多带 agent 列信息（是否安装）。
/// 界面默认只列 provider，点「分配」才展开卡片，靠这份数据。
///
/// 异步 + spawn_blocking：同步命令若在 GUI 线程上卡住，前端骨架屏会
/// 一直不消，连 8s 超时都救不了（WebView 定时器也可能跟着僵住）。
#[tauri::command]
pub async fn provider_matrix() -> Result<crate::types::ProviderMatrix, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::providers::matrix::build().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("provider_matrix 任务失败：{e}"))?
}

/// 整行替换某个 provider 的分配列表。
///
/// 一次写整行而不是逐格写，避免中间状态。
/// Claude 是单槽位，勾上它会自动从其他 provider 上摘掉。
#[tauri::command]
pub fn set_provider_row(
    id: String,
    agents: Vec<String>,
    claude_model_map: Option<crate::types::ClaudeModelMap>,
) -> Result<RunResult, String> {
    match crate::providers::matrix::set_row(&id, &agents, claude_model_map) {
        Ok(()) => Ok(RunResult::success(format!(
            "已更新 {id} 的分配：{}",
            if agents.is_empty() {
                "（无）".to_string()
            } else {
                agents.join(", ")
            }
        ))),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 切换某个 provider 在某个 agent 的分配状态。
#[tauri::command]
pub fn set_provider_assignment(
    provider_id: String,
    agent: String,
    on: bool,
) -> Result<RunResult, String> {
    match crate::providers::matrix::set_assignment(&provider_id, &agent, on) {
        Ok(()) => Ok(RunResult::success(format!(
            "{provider_id} 在 {agent} {}",
            if on { "已启用" } else { "已取消" }
        ))),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 把母版里的 provider 注入到各 agent。
///
/// 异步：涉及多个文件的读写，放主线程会卡界面。
#[tauri::command]
pub async fn sync_providers(dry_run: bool) -> Result<crate::types::ProviderSyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || sync_providers_inner(dry_run))
        .await
        .map_err(|e| format!("同步失败：{e}"))?
}

fn sync_providers_inner(dry_run: bool) -> Result<crate::types::ProviderSyncReport, String> {
    let list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Ok(crate::types::ProviderSyncReport {
            ok: false,
            written: Vec::new(),
            skipped: Vec::new(),
            backups: Vec::new(),
            log: vec!["母版里没有 provider，无事可做".to_string()],
        });
    }

    let report = crate::providers::sync(&list, dry_run).map_err(|e| e.to_string())?;
    Ok(crate::types::ProviderSyncReport {
        ok: report.ok,
        written: report.written,
        skipped: report.skipped,
        backups: report.backups,
        log: report.log,
    })
}

/// 设置某个 provider 的 Codex 后端开关，并立刻写入 Codex 配置。
///
/// 单独一个命令而不是走 `save_provider`：那个要传全字段（models、
/// agentKinds、agents…），前端为了改一个布尔值去拼整个 provider 很容易
/// 漏字段把别的设置洗掉。
///
/// 只改母版里这一条的这个字段，然后只注入 codex 一家 —— 不跑全量 sync，
/// 避免为了一个开关去重写其它 agent 的配置。
#[tauri::command]
pub fn set_codex_requires_openai_auth(
    provider_id: String,
    value: Option<bool>,
) -> Result<RunResult, String> {
    set_codex_field(provider_id, |p| p.codex_requires_openai_auth = value)
}

/// 设置某个 provider 分配给 Codex 时用哪个模型，并立刻写入 Codex 配置。
///
/// Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型进不去。
/// 「用哪个模型」只能靠 `config.toml` 顶层 `model` 这个键，所以这里改的是
/// 母版里的 `codex_model`，再由注入器写进配置文件。
///
/// 传空字符串或 null 表示取消指定，回退到 `models` 的第一个。
#[tauri::command]
pub fn set_codex_model(provider_id: String, model: Option<String>) -> Result<RunResult, String> {
    let clean = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    set_codex_field(provider_id, |p| p.codex_model = clean)
}

/// 改 Codex 相关的单个字段，然后只重写 `~/.codex/config.toml`。
///
/// 两个 Codex 专属设置（后端开关、用哪个模型）共用这段：都是「只改母版
/// 一条的一个字段 + 只注入 codex 一家」。抽出来是为了让写入流程只写一遍 ——
/// 前端为了改一个值去拼整个 provider 很容易漏字段，把别的设置洗掉。
///
/// 参数 `apply` 在拿到目标 provider 后执行，负责把新值写进内存里的列表。
/// 返回写入日志，失败时返回 `RunResult::failure`。
fn set_codex_field(
    provider_id: String,
    apply: impl FnOnce(&mut crate::providers::model::Provider),
) -> Result<RunResult, String> {
    let mut list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;
    let Some(target) = list.iter_mut().find(|p| p.id == provider_id) else {
        return Ok(RunResult::failure(
            1,
            format!("母版里没有 provider：{provider_id}"),
        ));
    };
    apply(target);
    crate::providers::store::save(&list).map_err(|e| e.to_string())?;

    // 只重写 codex 一家的配置
    let codex = crate::agents::find("codex")
        .ok_or_else(|| "找不到 codex agent 实现".to_string())?;
    let plans = codex.provider_plan(&list).map_err(|e| e.to_string())?;

    let mut lines = Vec::new();
    for plan in plans {
        if !plan.changed {
            lines.push(format!("[跳过] {} 无变化", plan.path));
            continue;
        }
        let real = crate::config::expand_tilde(&plan.path);
        match crate::providers::safe_write::safe_write(&real, &plan.content, &plan.pre_hash) {
            Ok(_) => {
                lines.push(format!("[写入] {}", plan.path));
                for s in &plan.summary {
                    lines.push(format!("       {s}"));
                }
            }
            Err(e) => {
                return Ok(RunResult::failure(1, format!("写入失败：{e}")));
            }
        }
    }
    if lines.is_empty() {
        lines.push("Codex 配置未发生变化".to_string());
    }

    Ok(RunResult::success(lines.join("\n")))
}

/// 读 Cursor 的自带密钥（BYOK）状态。
///
/// Cursor 的配置在 SQLite 库 `state.vscdb` 里，不在任何文本文件，
/// 也不参与 `providers sync`。所以单独一组命令，界面上单独一块。
///
/// 返回结果不含密钥原文，只有长度和末尾 4 位。
#[tauri::command]
pub fn cursor_byok_status() -> Result<crate::types::CursorByokState, String> {
    crate::providers::cursor_db::state_view().map_err(|e| e.to_string())
}

/// 算「要让某个 provider 在 Cursor 里生效，还差哪些改动」。只读。
///
/// `models` 是希望出现在 Cursor 模型选择器里的名字。省略表示不管模型。
#[tauri::command]
pub fn cursor_byok_plan(
    provider_id: String,
    models: Option<Vec<String>>,
) -> Result<crate::types::CursorByokPlan, String> {
    let (base_url, key) = read_provider_secret(&provider_id)?;
    crate::providers::cursor_db::plan_view(&base_url, &key, &models.unwrap_or_default())
        .map_err(|e| e.to_string())
}

/// 把某个 provider 写进 Cursor。要求 Cursor 已完全退出。
///
/// `models` 是希望出现在 Cursor 模型选择器里的名字。省略表示不动模型列表
/// —— 母版里一条 provider 往往有好几个模型，不能靠空列表猜用户要哪个。
///
/// `force` 只该在写副本时用。界面上不给这个入口 —— 用户点了写入就
/// 应该先关掉 Cursor，这是唯一的正确做法。
#[tauri::command]
pub fn cursor_byok_apply(
    provider_id: String,
    models: Option<Vec<String>>,
) -> Result<RunResult, String> {
    let (base_url, key) = read_provider_secret(&provider_id)?;

    let plan = match crate::providers::cursor_db::plan(&base_url, &key, &models.unwrap_or_default())
    {
        Ok(p) => p,
        Err(e) => return Ok(RunResult::failure(1, format!("无法生成计划：{e}"))),
    };

    if plan.is_noop() {
        return Ok(RunResult::success(format!(
            "Cursor 的配置已经和 {provider_id} 一致，无需改动。"
        )));
    }

    // 进程检查和真正的写入都在 apply 里做，这里不再重复判断 ——
    // 否则会出现「界面上说不行、实际能写」这种两处结论不一致的情况
    match crate::providers::cursor_db::apply(&plan, &key, false) {
        Ok(lines) => {
            let mut out = Vec::new();
            out.push(format!("已写入 Cursor（{provider_id}）："));
            for l in &lines {
                out.push(format!("  - {l}"));
            }
            out.push(String::new());
            out.push("重启 Cursor 后生效。".to_string());
            Ok(RunResult::success(out.join("\n")))
        }
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 从 Cursor 的模型选择器里删掉指定模型。
///
/// 只改 `userAddedModels` 这一项，不动开关、地址、密钥 —— 删一个模型
/// 不该顺带重写接口配置。
#[tauri::command]
pub fn cursor_byok_remove_models(models: Vec<String>) -> Result<RunResult, String> {
    if models.is_empty() {
        return Ok(RunResult::failure(1, "没有指定要删的模型".to_string()));
    }

    let plan = crate::providers::cursor_db::plan_remove_models(&models);
    match crate::providers::cursor_db::apply(&plan, "", false) {
        Ok(lines) => {
            let mut out = vec!["已更新 Cursor 的模型选择器：".to_string()];
            for l in &lines {
                out.push(format!("  - {l}"));
            }
            out.push(String::new());
            out.push("重启 Cursor 后生效。".to_string());
            Ok(RunResult::success(out.join("\n")))
        }
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 让 Cursor 回到「不用自带密钥」：关总开关、清接口地址。
///
/// 密钥留在库里不清 —— 下次再启用时不用重填。
#[tauri::command]
pub fn cursor_byok_reset() -> Result<RunResult, String> {
    let plan = crate::providers::cursor_db::CursorPlan {
        set_use_openai_key: Some(false),
        set_base_url: Some(String::new()),
        write_key: false,
        add_models: Vec::new(),
        remove_models: Vec::new(),
        summary: vec!["关闭自带密钥，清空接口地址".into()],
    };

    match crate::providers::cursor_db::apply(&plan, "", false) {
        Ok(lines) => {
            let mut out = vec!["已重置 Cursor：".to_string()];
            for l in &lines {
                out.push(format!("  - {l}"));
            }
            out.push(String::new());
            out.push("重启 Cursor 后回到官方模型。".to_string());
            Ok(RunResult::success(out.join("\n")))
        }
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 从母版取某个 provider 的接口地址和密钥。
///
/// 三个 Cursor 命令都要这两项，缺失时的报错也只写一遍。
/// 密钥只在这个进程内流转，不返回给前端。
fn read_provider_secret(id: &str) -> Result<(String, String), String> {
    let list = crate::providers::store::load_or_default().map_err(|e| e.to_string())?;
    let Some(p) = list.iter().find(|p| p.id == id) else {
        return Err(format!("母版里没有 provider：{id}"));
    };
    if p.base_url.trim().is_empty() {
        return Err(format!("provider {id} 没有 base_url，Cursor 需要它"));
    }
    if p.api_key.trim().is_empty() {
        return Err(format!("provider {id} 没有 api_key，Cursor 需要它"));
    }
    Ok((p.base_url.clone(), p.api_key.clone()))
}

/// 读 Claude Code 的首次引导状态。
///
/// 这个开关和 provider 无关：它写的是 `~/.claude.json` 根上的
/// `hasCompletedOnboarding`，描述「这台机器上的 Claude Code 是否已
/// 完成引导」。所以它是独立命令，不参与 `providers sync`。
#[tauri::command]
pub fn claude_onboarding_status() -> Result<crate::types::ClaudeOnboardingStatus, String> {
    crate::providers::claude_onboarding::status().map_err(|e| e.to_string())
}

/// 设置或清除 Claude Code 的首次引导跳过标记。
///
/// 只动 `~/.claude.json` 的一个键，文件里其他内容（登录态、项目历史、
/// MCP server）一个字节都不碰。
#[tauri::command]
pub fn set_claude_onboarding_skip(skip: bool) -> Result<RunResult, String> {
    match crate::providers::claude_onboarding::set_skipped(skip) {
        Ok((changed, message)) => Ok(if changed {
            RunResult::success(message)
        } else {
            // 没有实际改动不算失败，但把原因说清楚
            RunResult::success(message)
        }),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 读取母版的原始 TOML 文本，供界面直接编辑。
#[tauri::command]
pub fn read_providers_toml() -> Result<String, String> {
    let p = crate::providers::store::path();
    if !p.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&p).map_err(|e| e.to_string())
}

/// 直接写入母版 TOML 文本（高级编辑用）。
#[tauri::command]
pub fn write_providers_toml(content: String) -> Result<RunResult, String> {
    let p = crate::providers::store::path();
    // 先验证能解析，避免写坏
    if let Err(e) = toml::from_str::<crate::providers::model::ProviderFile>(&content) {
        return Ok(RunResult::failure(1, format!("TOML 格式错误：{e}")));
    }
    if let Some(parent) = p.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Ok(RunResult::failure(1, format!("创建目录失败：{e}")));
        }
    }
    match std::fs::write(&p, content) {
        Ok(()) => Ok(RunResult::success("母版已保存")),
        Err(e) => Ok(RunResult::failure(1, format!("写入失败：{e}"))),
    }
}

// ---- MCP 统一管理 ----

/// 列出母版里的 MCP server。环境变量和请求头只返回值名，不返回值。
#[tauri::command]
pub fn list_mcp_servers() -> Result<Vec<crate::types::McpServerView>, String> {
    let list = crate::mcp::store::load().map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .map(|s| {
            // 先算 display_name 和键名列表，再 move 字段
            let name = s.display_name().to_string();
            let env_keys: Vec<String> = s.env.keys().cloned().collect();
            let header_keys: Vec<String> = s.headers.keys().cloned().collect();
            crate::types::McpServerView {
                id: s.id,
                name,
                transport: s.transport.as_str().to_string(),
                command: s.command,
                args: s.args,
                cwd: s.cwd,
                url: s.url,
                env_keys,
                header_keys,
                enabled: s.enabled,
                agents: s.agents,
                note: s.note,
            }
        })
        .collect())
}

/// 保存一条 MCP server（新增或更新）。
///
/// `env` / `headers` 传完整键值对；传空表示清空。
/// 注意这和 provider 的「空 key 表示不修改」不同 ——
/// MCP 的 env 是个 map，清空和不变的语义需要区分，所以用完整替换。
#[tauri::command]
pub fn save_mcp_server(
    id: String,
    name: String,
    transport: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    url: String,
    env: std::collections::BTreeMap<String, String>,
    headers: std::collections::BTreeMap<String, String>,
    enabled: bool,
    agents: Vec<String>,
    note: String,
) -> Result<RunResult, String> {
    let mut list = crate::mcp::store::load().map_err(|e| e.to_string())?;

    let server = crate::mcp::model::McpServer {
        id: id.clone(),
        name,
        transport: crate::mcp::model::Transport::parse(&transport),
        command,
        args,
        cwd,
        url,
        headers,
        env,
        enabled,
        agents,
        note,
    };

    if let Some(err) = server.validate() {
        return Ok(RunResult::failure(1, err));
    }

    match list.iter_mut().find(|s| s.id == id) {
        Some(existing) => *existing = server,
        None => list.push(server),
    }

    crate::mcp::store::save(&list).map_err(|e| e.to_string())?;

    let mut msg = format!("已保存 MCP server {id}");
    if let Some(warn) = crate::mcp::store::check_gitignore() {
        msg.push_str("\n\n[警告] ");
        msg.push_str(&warn);
    }
    Ok(RunResult::success(msg))
}

/// 从母版删除一条 MCP server。
#[tauri::command]
pub fn delete_mcp_server(id: String) -> Result<RunResult, String> {
    let mut list = crate::mcp::store::load().map_err(|e| e.to_string())?;
    let before = list.len();
    list.retain(|s| s.id != id);
    if list.len() == before {
        return Ok(RunResult::failure(1, format!("母版里没有 {id}")));
    }
    crate::mcp::store::save(&list).map_err(|e| e.to_string())?;
    Ok(RunResult::success(format!("已从母版移除 {id}")))
}

/// 对比 MCP 母版与各 agent 现状。只读。
#[tauri::command]
pub fn diff_mcp() -> Result<crate::types::McpDiffReportView, String> {
    let report = crate::mcp::diff_run().map_err(|e| e.to_string())?;
    Ok(crate::types::McpDiffReportView {
        lines: report
            .lines
            .into_iter()
            .map(|l| crate::types::McpDiffLineView {
                agent: l.agent,
                path: l.path,
                action: l.action,
                target: l.target,
                detail: l.detail,
            })
            .collect(),
        agent_count: report.agent_count,
        changed_files: report.changed_files,
    })
}

/// 把母版里的 MCP server 注入到各 agent。异步执行。
#[tauri::command]
pub async fn sync_mcp(dry_run: bool) -> Result<crate::types::McpSyncReportView, String> {
    tauri::async_runtime::spawn_blocking(move || sync_mcp_inner(dry_run))
        .await
        .map_err(|e| format!("同步失败：{e}"))?
}

fn sync_mcp_inner(dry_run: bool) -> Result<crate::types::McpSyncReportView, String> {
    let list = crate::mcp::store::load().map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Ok(crate::types::McpSyncReportView {
            ok: false,
            written: Vec::new(),
            skipped: Vec::new(),
            backups: Vec::new(),
            log: vec!["母版里没有 MCP server，无事可做".to_string()],
        });
    }

    let report = crate::mcp::sync_run(&list, dry_run).map_err(|e| e.to_string())?;
    Ok(crate::types::McpSyncReportView {
        ok: report.ok,
        written: report.written,
        skipped: report.skipped,
        backups: report.backups,
        log: report.log,
    })
}

/// 读取 MCP 母版原始 TOML。
#[tauri::command]
pub fn read_mcp_toml() -> Result<String, String> {
    let p = crate::mcp::store::path();
    if !p.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&p).map_err(|e| e.to_string())
}

/// 写入 MCP 母版 TOML。先校验格式。
#[tauri::command]
pub fn write_mcp_toml(content: String) -> Result<RunResult, String> {
    let p = crate::mcp::store::path();
    if let Err(e) = toml::from_str::<crate::mcp::model::McpFile>(&content) {
        return Ok(RunResult::failure(1, format!("TOML 格式错误：{e}")));
    }
    if let Some(parent) = p.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Ok(RunResult::failure(1, format!("创建目录失败：{e}")));
        }
    }
    match std::fs::write(&p, content) {
        Ok(()) => Ok(RunResult::success("MCP 母版已保存")),
        Err(e) => Ok(RunResult::failure(1, format!("写入失败：{e}"))),
    }
}

/// 读取 MCP 分配矩阵：server 池 × agent 的勾选关系。
#[tauri::command]
pub fn mcp_matrix() -> Result<crate::mcp::matrix::McpMatrix, String> {
    crate::mcp::matrix::build().map_err(|e| e.to_string())
}

/// 设置某个 server 在某个 agent 的启用状态（单格切换）。
#[tauri::command]
pub fn set_mcp_assignment(server_id: String, agent: String, on: bool) -> Result<RunResult, String> {
    match crate::mcp::matrix::set_assignment(&server_id, &agent, on) {
        Ok(()) => Ok(RunResult::success(format!(
            "{} → {} {}",
            server_id,
            agent,
            if on { "启用" } else { "停用" }
        ))),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 整行替换某个 server 的分配列表。
#[tauri::command]
pub fn set_mcp_row(server_id: String, agents: Vec<String>) -> Result<RunResult, String> {
    match crate::mcp::matrix::set_row(&server_id, &agents) {
        Ok(()) => Ok(RunResult::success(format!(
            "{} 的分配已更新（{} 个 agent）",
            server_id,
            agents.len()
        ))),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

/// 从各 agent 现有配置导入 server 到池子。
#[tauri::command]
pub fn import_mcp_from_agents() -> Result<RunResult, String> {
    match crate::mcp::matrix::import_from_agents() {
        Ok(added) => {
            if added.is_empty() {
                Ok(RunResult::success("没有新的 server 可导入"))
            } else {
                Ok(RunResult::success(format!(
                    "已导入 {} 个 server：{}",
                    added.len(),
                    added.join(", ")
                )))
            }
        }
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}

// ---- token 用量统计 ----

/// 统计各 agent 的 token 用量。
///
/// `days` 是统计窗口天数，0 表示不限。
/// `refresh` 为真时先丢掉全部缓存，从头重算 —— 界面上对应「重算」按钮，
/// 用户怀疑数字不对时用。平时统计是增量的，只读新增的数据。
///
/// 异步执行：启用的 agent 越多、会话文件越大，读盘时间越长
/// （本机 pi 595 MB + codex 191 MB + Cursor 的 4.2 GB 库）。
/// 放在 IPC 线程上会直接卡住界面。
#[tauri::command]
pub async fn usage_report(days: u32, refresh: bool) -> Result<crate::types::UsageReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::usage::collect_report(days, refresh))
        .await
        .map_err(|e| format!("统计失败：{e}"))
}

/// 打开或关闭某个 agent 的用量统计。
///
/// 写的是 `~/.agents/agentcenter.toml` 的 `[usage] enabled` 名单。
/// 只改配置，不读任何用量数据 —— 打开之后要等下一次 `usage_report` 才见数字。
#[tauri::command]
pub fn set_usage_enabled(agent: String, on: bool) -> Result<RunResult, String> {
    match crate::usage::set_enabled(&agent, on) {
        Ok(now) => Ok(RunResult::success(format!(
            "{agent} 的用量统计已{}",
            if now { "启用" } else { "关闭" }
        ))),
        Err(e) => Ok(RunResult::failure(1, e.to_string())),
    }
}
