//! skill 的收集、去重与标签管理。
//!
//! 核心约束（来自本机实测）：
//!
//! - 磁盘上 `~/.agents/skills` 必须保持平铺。ZCode 只扫
//!   `<skills>/<名字>/SKILL.md` 两层，加分类子目录就读不到，
//!   所以"分类"完全由标签索引承担。
//! - 标签存在 `~/.agents/agentcenter.toml` 里，不写进 skill 目录。
//!   那 6 个由 `npx skills` 管理的 skill 一旦目录内被改动，
//!   下次 update 就会报冲突。
//! - 比对用内容指纹而非目录名。同名不同内容必须报冲突，不能覆盖。

use crate::config::{SkillMappingConfig, SkillTags};
use crate::link;
use crate::types::{
    CollectPlan, LinkKind, LinkMode, OrphanAction, OrphanActionResult, OrphanRelation, OrphanSkill,
    PrivateLinkState, PrivateLinkView, Skill, SkillMappingView,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 中心库位置：`~/.agents/skills`。
///
/// 走 `store::home_dir()` 以跟随 `AGENTCENTER_HOME`。
pub fn shared_skills_dir() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join("skills")
}

/// 某个 agent 的专用 skill 库：`~/.agents/skills-mapping-<agent>`。
///
/// 专用 skill 的本体放这里，不进公共库 —— 否则 19 个 agent 都会读到
/// 一个只有某一个 agent 才用得上的 skill。agent 那边靠链接读回来。
pub fn private_skills_root(agent: &str) -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join(format!("skills-mapping-{agent}"))
}

/// `npx skills` 的 lock 文件位置：`~/.agents/.skill-lock.json`。
pub fn skill_lock_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join(".skill-lock.json")
}

/// 从 SKILL.md 的 frontmatter 里取出 description。
///
/// 支持三种写法，都是实际 skill 里出现过的：
///
/// 1. 单行：`description: 一句话说明`
/// 2. 折叠块：`description: >` 或 `description: >-`，内容在下面缩进的行
/// 3. 字面块：`description: |` 或 `description: |-`
///
/// 第 2、3 种如果只按行取值，会得到 `>` 这种残渣（本机三个 skill 就是
/// 这样），所以必须往下读缩进内容。
fn read_description(skill_md: &Path) -> String {
    let Ok(text) = fs::read_to_string(skill_md) else {
        return String::new();
    };

    // 先截出 frontmatter 的正文部分
    let lines: Vec<&str> = text.lines().collect();
    let mut start = None;
    let mut end = None;
    let mut seen = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            seen += 1;
            if seen == 1 {
                start = Some(i + 1);
            } else {
                end = Some(i);
                break;
            }
        }
    }
    let (Some(s), Some(e)) = (start, end) else {
        return String::new();
    };
    let fm = &lines[s..e];

    // 找到 description 那一行
    let Some(idx) = fm
        .iter()
        .position(|l| l.trim_start().starts_with("description:"))
    else {
        return String::new();
    };

    let head = fm[idx].trim_start().trim_start_matches("description:").trim();

    // 判断是不是块标量
    let is_block = matches!(head, ">" | ">-" | "|" | "|-" | ">+" | "|+");

    if !is_block {
        // 单行：去掉两端引号
        return head.trim_matches('"').trim_matches('\'').to_string();
    }

    // 块标量：收集后续比 description 行缩进更深的行
    let desc_indent = fm[idx].len() - fm[idx].trim_start().len();
    let mut collected: Vec<String> = Vec::new();
    for line in &fm[idx + 1..] {
        let indent = line.len() - line.trim_start().len();
        let blank = line.trim().is_empty();
        if blank {
            // 空行在折叠块里表示段落分隔，保留
            collected.push(String::new());
            continue;
        }
        if indent <= desc_indent {
            break; // 回到同级或更浅，块结束
        }
        collected.push(line.trim().to_string());
    }

    // 折叠（>）把换行变空格，字面（|）保留换行；空行转成段落分隔
    let folded = head.starts_with('>');
    let mut out = String::new();
    let mut pending_blank = false;
    for line in collected {
        if line.is_empty() {
            pending_blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if pending_blank || !folded { "\n" } else { " " });
        }
        out.push_str(&line);
        pending_blank = false;
    }

    out.trim().to_string()
}

/// 统计目录里的文件数和总字节数，跳过 `.git`。
fn dir_stats(path: &Path) -> (usize, u64) {
    let mut count = 0;
    let mut size = 0u64;
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || e.file_name() != ".git")
        .flatten()
    {
        if entry.file_type().is_file() {
            count += 1;
            size += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    (count, size)
}

/// 读取 `npx skills` 的 lock 文件，返回「skill 名 → 来源仓库」的映射。
///
/// 读不到或格式不符时返回空表，让 skill 全部显示为"未受管"，
/// 而不是让整个界面报错。
pub fn read_lock_sources() -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = fs::read_to_string(skill_lock_path()) else {
        return out;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return out;
    };
    let Some(skills) = json.get("skills").and_then(|v| v.as_object()) else {
        return out;
    };
    for (name, entry) in skills {
        if let Some(src) = entry.get("source").and_then(|v| v.as_str()) {
            out.insert(name.clone(), src.to_string());
        }
    }
    out
}

/// 列出中心库里的全部 skill，附带标签和受管信息。
///
/// 只扫一层子目录，与各 agent 的读取方式保持一致。
///
/// 专用 skill 也列在这里，靠 `agents` 字段区分（空 = 公共）。分开列表反而
/// 让用户要在两处找同一个 skill；标签、搜索、筛选都应该对它同样生效。
pub fn list_skills(cfg: &crate::config::SkillsConfig) -> anyhow::Result<Vec<Skill>> {
    let root = shared_skills_dir();
    let lock = read_lock_sources();
    let mut out = Vec::new();

    // 中心库里的公共 skill：`agents` 为空
    if root.exists() {
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // 跳过隐藏目录，如 .git
            if name.starts_with('.') {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            let (file_count, size_bytes) = dir_stats(&path);
            let hash = link::sha256_dir(&path).unwrap_or_default();

            out.push(Skill {
                description: read_description(&skill_md),
                name,
                tags: Vec::new(),
                managed: false,
                source: None,
                hash: hash.chars().take(8).collect(),
                file_count,
                size_bytes,
                agents: Vec::new(),
                store_path: None,
                link_state: PrivateLinkState::Ok,
            });
        }
    }

    // 专用 skill：本体在 skills-mapping-<agent>，公共库里不该有同名项
    for m in &cfg.mappings {
        let Some(store) = m.store_dir() else {
            continue;
        };
        let dir = store.join(&m.skill);
        let skill_md = dir.join("SKILL.md");

        // 同名项已在公共库里出现：两边都留会让 agent 读到哪一个变得不确定
        // （链接和真实目录同名的处理各家不同）。跳过专用这条并在描述里说明。
        if out.iter().any(|s| s.name == m.skill) {
            if let Some(existing) = out.iter_mut().find(|s| s.name == m.skill) {
                existing.description = format!(
                    "{}（注意：{} 也记录为专用 skill，公共库里这份与它同名，需要手工处理）",
                    existing.description,
                    m.skill
                );
            }
            continue;
        }

        let view = inspect_private(m);

        if !skill_md.exists() {
            // 本体不在了但记录还在：这必须是可见的错误。
            // 静默跳过会让用户以为 skill 被删了，而记录会一直挂着。
            out.push(Skill {
                description: format!(
                    "本体目录不存在（{}）。记录还在，跑一次「修复链接」或取消专用",
                    crate::config::contract_tilde(&dir)
                ),
                name: m.skill.clone(),
                tags: Vec::new(),
                managed: false,
                source: None,
                hash: String::new(),
                file_count: 0,
                size_bytes: 0,
                agents: m.agents.clone(),
                store_path: Some(crate::config::contract_tilde(&dir)),
                link_state: view.state,
            });
            continue;
        }

        let (file_count, size_bytes) = dir_stats(&dir);
        let hash = link::sha256_dir(&dir).unwrap_or_default();

        out.push(Skill {
            description: read_description(&skill_md),
            name: m.skill.clone(),
            tags: Vec::new(),
            managed: false,
            source: None,
            hash: hash.chars().take(8).collect(),
            file_count,
            size_bytes,
            agents: m.agents.clone(),
            store_path: Some(crate::config::contract_tilde(&dir)),
            link_state: view.state,
        });
    }

    // 标签和受管信息统一在最后套上，两条来源共用同一份逻辑
    for s in out.iter_mut() {
        s.tags = cfg.tags.get(&s.name).cloned().unwrap_or_default();
        s.managed = lock.contains_key(&s.name);
        s.source = lock.get(&s.name).cloned();
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 检查一条专用记录的链接现场，给出一条完整视图。
///
/// 每个归属 agent 单独判一次：一处断了不代表全部断了，用户需要知道
/// 具体修哪一个位置。
pub fn inspect_private(m: &SkillMappingConfig) -> SkillMappingView {
    let store = m.store_dir();
    let store_dir = store
        .as_ref()
        .map(|s| s.join(&m.skill))
        .unwrap_or_else(|| PathBuf::from(&m.skill));
    let store_exists = store_dir.exists() || link::is_link(&store_dir);

    let mut links = Vec::new();
    let mut worst = if store_exists {
        PrivateLinkState::Ok
    } else {
        PrivateLinkState::Broken
    };

    for (agent_id, target) in m.targets() {
        let label = crate::agents::find(&agent_id)
            .map(|a| a.label().to_string())
            .unwrap_or_else(|| agent_id.clone());

        let (state, actual, note) = inspect_one_link(&target, &store_dir);

        // 整体状态取最差：有一处断了就得修，不能因为别处正常就说没事
        worst = worse_state(worst, state);

        links.push(PrivateLinkView {
            agent: agent_id,
            label,
            path: crate::config::contract_tilde(&target),
            state,
            actual,
            note,
        });
    }

    // 配置里写了不存在的 agent，或者 agents 为空：也算有问题
    if m.agents.is_empty() {
        worst = PrivateLinkState::Broken;
    } else if links.len() < m.agents.len() {
        worst = worse_state(worst, PrivateLinkState::Broken);
    }

    SkillMappingView {
        skill: m.skill.clone(),
        agents: m.agents.clone(),
        store_path: crate::config::contract_tilde(&store_dir),
        store_exists,
        links,
        state: worst,
        mode: m.mode,
    }
}

/// 单个位置的链接状态。
fn inspect_one_link(target: &Path, store: &Path) -> (PrivateLinkState, Option<LinkKind>, Option<String>) {
    if link::is_link(target) {
        // 链接还在就去认它实际是 junction 还是 symlink，别一律当成 junction
        let actual = if target.exists() {
            Some(link::detect_link_kind(target))
        } else {
            None
        };
        return match link::read_link_target(target) {
            Some(dest) if !dest.exists() => (
                PrivateLinkState::Broken,
                actual,
                Some(format!("链接指向 {}，那里不存在", dest.display())),
            ),
            Some(dest) if link::same_file(&dest, store) => (PrivateLinkState::Ok, actual, None),
            Some(dest) => (
                PrivateLinkState::Broken,
                actual,
                Some(format!("链接指向 {}，不是专用库那一份", dest.display())),
            ),
            None => (PrivateLinkState::Broken, actual, Some("读不到链接目标".to_string())),
        };
    }

    if target.exists() {
        // 存在但不是链接：要么没被专用化过，要么用户把链接换回了真实目录
        return (
            PrivateLinkState::Broken,
            None,
            Some("这里是真实目录，不是指向专用库的链接".to_string()),
        );
    }

    (PrivateLinkState::Missing, None, Some("位置不存在".to_string()))
}

/// 合并两个链接状态，返回更严重的那个。
fn worse_state(a: PrivateLinkState, b: PrivateLinkState) -> PrivateLinkState {
    match (a, b) {
        (PrivateLinkState::Broken, _) | (_, PrivateLinkState::Broken) => PrivateLinkState::Broken,
        (PrivateLinkState::Missing, _) | (_, PrivateLinkState::Missing) => {
            PrivateLinkState::Missing
        }
        _ => PrivateLinkState::Ok,
    }
}

/// 扫描散落在各 agent 目录里的 skill。
///
/// 只扫已安装 agent 的 skills 目录。对于本身就是链接的条目，
/// 检查它是否有效；有效的链接指向中心库，属于已收好的状态，不算散落。
pub fn scan_orphans(tags: &SkillTags) -> anyhow::Result<Vec<OrphanSkill>> {
    let shared = shared_skills_dir();
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // 中心库现有 skill 的指纹与描述，用于判断「是否相同」和冲突时对照
    let mut shared_hashes: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    if shared.exists() {
        for entry in fs::read_dir(&shared)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(h) = link::sha256_dir(&path) {
                    shared_hashes.insert(name, h);
                }
            }
        }
    }

    // 注意：不能因为 agent "原生读共享目录" 就跳过它的 skills 目录。
    // Cursor 和 OpenCode 都会读 ~/.agents/skills，但各自的 ~/.cursor/skills、
    // ~/.config/opencode/skills 里也常有独立安装的 skill，那部分同样需要收集。
    // 唯一的例外是目录本身不存在，那时没什么可扫的。
    for agent in crate::agents::all() {
        let dir = &agent.skills_dir();
        if !dir.exists() {
            continue;
        }

        // read_dir 返回 Result，先取出 ReadDir 再决定是否遍历；
        // 单个目录读不到不该让整个扫描失败，所以用 continue 跳过。
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // 隐藏目录和 .system 之类跳过
            if name.starts_with('.') {
                continue;
            }

            // 同一个 skill 可能在多个目录出现，用「路径」去重而不是名字
            let key = path.to_string_lossy().to_string();
            if !seen.insert(key) {
                continue;
            }

            let location = crate::config::contract_tilde(dir);

            // 本来就是链接：检查有效性
            if link::is_link(&path) {
                match link::read_link_target(&path) {
                    Some(dest) if dest.exists() => {
                        // 已指向中心库且有效，属于打理好的状态，不报告
                        continue;
                    }
                    _ => {
                        out.push(OrphanSkill {
                            name,
                            location,
                            agent: Some(agent.id().to_string()),
                            path: path.to_string_lossy().to_string(),
                            relation: OrphanRelation::BrokenLink,
                            description: String::new(),
                            file_count: 0,
                            size_bytes: 0,
                            hash: String::new(),
                        });
                        continue;
                    }
                }
            }

            if !path.is_dir() {
                continue;
            }
            // 没有 SKILL.md 的不算 skill
            if !path.join("SKILL.md").exists() {
                continue;
            }

            let relation = match shared_hashes.get(&name) {
                None => OrphanRelation::Unique,
                Some(h) => match link::sha256_dir(&path) {
                    Ok(local) if &local == h => OrphanRelation::Identical,
                    _ => OrphanRelation::Conflict,
                },
            };

            // 描述、大小、指纹都算一遍交给界面：冲突时用户要靠这些信息
            // 判断该采用哪一份，只给路径是看不出来的。
            let (file_count, size_bytes) = dir_stats(&path);
            let hash: String = link::sha256_dir(&path)
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect();

            out.push(OrphanSkill {
                name,
                location,
                agent: Some(agent.id().to_string()),
                path: path.to_string_lossy().to_string(),
                relation,
                description: read_description(&path.join("SKILL.md")),
                file_count,
                size_bytes,
                hash,
            });
        }
    }

    // 借 tags 参数让签名与 list_skills 一致，便于上层统一处理
    let _ = tags;

    Ok(out)
}

/// 把扫描结果分组，形成可执行的计划。
pub fn build_plan(orphans: Vec<OrphanSkill>) -> CollectPlan {
    let mut plan = CollectPlan::default();
    for o in orphans {
        match o.relation {
            OrphanRelation::Unique => plan.to_import.push(o),
            OrphanRelation::Identical => plan.to_dedupe.push(o),
            OrphanRelation::Conflict => plan.conflicts.push(o),
            OrphanRelation::BrokenLink => plan.broken.push(o),
        }
    }
    plan
}

/// 执行收集计划，返回逐条处理的结果说明。
///
/// 只处理 `to_import` 和 `to_dedupe`。冲突和坏链接一律不动 ——
/// 那两种情况需要人看过后再决定，工具替用户做决定风险太大。
pub fn apply_plan(plan: &CollectPlan) -> anyhow::Result<Vec<String>> {
    let shared = shared_skills_dir();
    fs::create_dir_all(&shared)?;
    let mut log = Vec::new();

    // 移入：把散落的目录搬到中心库，原位置建链接指回来
    for o in &plan.to_import {
        let src = PathBuf::from(&o.path);
        let dst = shared.join(&o.name);

        if dst.exists() {
            log.push(format!("[跳过] {} 中心库已存在同名目录", o.name));
            continue;
        }

        match fs::rename(&src, &dst) {
            Ok(()) => {
                // 搬完后在原位置建链接，让原来的 agent 还能读到
                match link::create_link(&src, &dst, crate::types::LinkMode::Auto) {
                    Ok((kind, _)) => log.push(format!(
                        "[移入] {} 从 {} → 中心库，原位置建立 {kind:?}",
                        o.name, o.location
                    )),
                    Err(e) => log.push(format!(
                        "[警告] {} 已移入中心库，但原位置建链接失败：{e}",
                        o.name
                    )),
                }
            }
            Err(e) => {
                log.push(format!("[失败] {} 移动失败：{e}", o.name));
            }
        }
    }

    // 去重：内容相同，删掉散落的那一份，原位置建链接
    for o in &plan.to_dedupe {
        let src = PathBuf::from(&o.path);
        let dst = shared.join(&o.name);

        // 删副本前再确认一次中心库那份还在
        if !dst.exists() {
            log.push(format!(
                "[跳过] {} 中心库那份不见了，保留散落副本",
                o.name
            ));
            continue;
        }

        match fs::remove_dir_all(&src) {
            Ok(()) => match link::create_link(&src, &dst, crate::types::LinkMode::Auto) {
                Ok((kind, _)) => log.push(format!(
                    "[去重] {} 删除 {} 的副本，改建 {kind:?}",
                    o.name, o.location
                )),
                Err(e) => log.push(format!(
                    "[警告] {} 副本已删，但原位置建链接失败：{e}",
                    o.name
                )),
            },
            Err(e) => log.push(format!("[失败] {} 删除副本失败：{e}", o.name)),
        }
    }

    if !plan.conflicts.is_empty() {
        log.push(format!(
            "{} 个同名但内容不同的 skill 未处理，需要你决定保留哪一份",
            plan.conflicts.len()
        ));
    }
    if !plan.broken.is_empty() {
        log.push(format!(
            "{} 个坏链接未处理，建议手动检查",
            plan.broken.len()
        ));
    }

    Ok(log)
}

/// 对**一条**散落 skill 执行用户选定的处置。
///
/// 和 [`apply_plan`] 的区别：那个按关系批量套用固定策略，这个由用户在
/// 界面上逐条选。所以每条都单独重新校验现场，不信任界面拿着的旧扫描结果
/// —— 对话框开着的时候用户完全可能在资源管理器里动过文件。
///
/// 每个分支的共同底线：**中心库原有的内容不会因为误点而消失**。
/// 需要覆盖中心库（`TakeOrphan`）时先把中心库那份备份掉。
pub fn apply_action(path: &str, action: OrphanAction) -> OrphanActionResult {
    let src = PathBuf::from(path);
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let result = |ok: bool, message: String| OrphanActionResult {
        name: name.clone(),
        path: path.to_string(),
        action,
        ok,
        message,
    };

    // 现场复核：路径没了就别继续，直接告诉用户重扫
    if !src.exists() && !link::is_link(&src) {
        return result(false, "这条已经不在磁盘上了，请重新扫描".to_string());
    }

    let shared = shared_skills_dir();

    match action {
        OrphanAction::Import => import_one(&src, &name, &shared).map_or_else(
            |e| result(false, format!("移入失败：{e}")),
            |m| result(true, m),
        ),

        OrphanAction::Dedupe | OrphanAction::TakeShared => {
            let dst = shared.join(&name);
            if !dst.exists() {
                return result(
                    false,
                    format!("中心库里没有 {name}，不能用中心库那一份覆盖"),
                );
            }
            replace_with_link(&src, &dst, "散落副本").map_or_else(
                |e| result(false, format!("处理失败：{e}")),
                |m| result(true, m),
            )
        }

        OrphanAction::TakeOrphan => {
            let dst = shared.join(&name);
            if !dst.exists() {
                // 中心库没有这一份，那「采用散落版」等价于移入
                return import_one(&src, &name, &shared).map_or_else(
                    |e| result(false, format!("移入失败：{e}")),
                    |m| result(true, m),
                );
            }
            // 先备份中心库那份，再把散落这份搬到中心库
            match link::backup_dir_copy(&dst) {
                Ok(bak) => {
                    if let Err(e) = fs::remove_dir_all(&dst) {
                        return result(false, format!("删除中心库旧版失败：{e}"));
                    }
                    match fs::rename(&src, &dst) {
                        Ok(()) => match link::create_link(&src, &dst, crate::types::LinkMode::Auto)
                        {
                            Ok((kind, _)) => result(
                                true,
                                format!(
                                    "已用散落版替换中心库；旧版备份在 {}；原位置建立 {kind:?}",
                                    bak.display()
                                ),
                            ),
                            Err(e) => result(
                                true,
                                format!(
                                    "已替换中心库；旧版备份在 {}；但原位置建链接失败：{e}",
                                    bak.display()
                                ),
                            ),
                        },
                        Err(e) => result(false, format!("搬到中心库失败：{e}")),
                    }
                }
                Err(e) => result(false, format!("备份中心库旧版失败：{e}")),
            }
        }

        OrphanAction::Delete => {
            // 只删这一份，不建链接。用户明确要断掉这个 agent 对它的访问
            if link::is_link(&src) {
                fs::remove_file(&src)
                    .or_else(|_| fs::remove_dir(&src))
                    .map_or_else(
                        |e| result(false, format!("删除链接失败：{e}")),
                        |_| result(true, "已删除链接".to_string()),
                    )
            } else {
                fs::remove_dir_all(&src).map_or_else(
                    |e| result(false, format!("删除失败：{e}")),
                    |_| result(true, "已删除这份散落副本".to_string()),
                )
            }
        }

        OrphanAction::RemoveLink => {
            if !link::is_link(&src) {
                return result(false, "现在不是链接了（可能刚被改过），请重新扫描".to_string());
            }
            fs::remove_file(&src)
                .or_else(|_| fs::remove_dir(&src))
                .map_or_else(
                    |e| result(false, format!("删除坏链接失败：{e}")),
                    |_| result(true, "已删除坏链接".to_string()),
                )
        }

        OrphanAction::Privatize => {
            // 从路径反推所属 agent：只有知道是哪个 agent 的目录，
            // 才知道本体该放进哪个 skills-mapping-<agent>。
            let Some(agent) = agent_of_path(&src) else {
                return result(
                    false,
                    format!(
                        "认不出 {} 属于哪个 agent 的 skills 目录，无法标记专用",
                        src.display()
                    ),
                );
            };
            privatize_one(&src, &agent).map_or_else(
                |e| result(false, format!("标记专用失败：{e}")),
                |m| result(true, m),
            )
        }
    }
}

/// 从一个路径反推它属于哪个 agent 的 skills 目录。
///
/// 用「路径是否在该 agent 的 skills_dir 下」来判断。必须比最长前缀 ——
/// `~/.agents/skills` 和 `~/.cursor/skills` 不冲突，但同一个 agent 可能
/// 有嵌套目录，取最长的那个能避免把 `~/.pi/agent/skills` 误认成别处。
fn agent_of_path(path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for agent in crate::agents::all() {
        let dir = agent.skills_dir();
        if let Ok(rest) = path.strip_prefix(&dir) {
            // 必须在目录下，不能是目录本身
            if rest.as_os_str().is_empty() {
                continue;
            }
            let len = dir.as_os_str().len();
            if best.as_ref().map(|(l, _)| len > *l).unwrap_or(true) {
                best = Some((len, agent.id().to_string()));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// 把一个散落 skill 标记为某个 agent 专用。
///
/// 步骤和顺序都有讲究：
/// 1. 先确认公共库没有同名项 —— 有的话这个 skill 是公共的，不能私有化
/// 2. 确认专用库里也没有同名项 —— 有的话说明已经标记过，别覆盖
/// 3. 建专用库目录，把本体搬进去（`fs::rename`，同卷零拷贝）
/// 4. 在原位置建链接指回本体，agent 那边行为完全不变
/// 5. 最后写配置记录。**放最后**：前面任何一步失败都不会留下
///    「记录存在但磁盘没弄好」的状态，那比没有记录更难排查
pub fn privatize_one(src: &Path, agent: &str) -> anyhow::Result<String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| anyhow::anyhow!("路径没有目录名：{}", src.display()))?;

    if !src.join("SKILL.md").exists() {
        anyhow::bail!("{} 里没有 SKILL.md，不像一个 skill 目录", src.display());
    }

    let shared = shared_skills_dir().join(&name);
    // 本体本身就在公共库里时（`skills private add` 从公共库取），
    // 这个同名项就是要搬走的那一份，不算冲突。
    // 用规范化路径比较，避免 `~` 展开差异导致误判。
    let src_in_shared = fs::canonicalize(src)
        .ok()
        .zip(fs::canonicalize(&shared).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !src_in_shared && (shared.exists() || link::is_link(&shared)) {
        anyhow::bail!(
            "公共库里已经有同名 skill（{}）。要私有化它，得先决定公共库那份怎么处理",
            crate::config::contract_tilde(&shared)
        );
    }

    let root = private_skills_root(agent);
    let store = root.join(&name);
    if store.exists() || link::is_link(&store) {
        anyhow::bail!(
            "专用库里已经有同名目录（{}），这个 skill 可能已经标记过",
            crate::config::contract_tilde(&store)
        );
    }

    fs::create_dir_all(&root)?;
    fs::rename(src, &store).map_err(|e| {
        anyhow::anyhow!(
            "搬到专用库失败：{e}。若是跨磁盘，请先把 {} 移到 {} 附近再试",
            src.display(),
            root.display()
        )
    })?;

    // 记录最后写。写失败时把本体搬回原处，让磁盘和配置保持一致
    let entry = SkillMappingConfig {
        skill: name.clone(),
        agents: vec![agent.to_string()],
        mode: LinkMode::Auto,
    };
    let mut cfg = crate::config::Config::load()?;
    cfg.skills.upsert_mapping(entry);
    if let Err(e) = cfg.save() {
        // 回滚搬移：把本体搬回去，不留半截状态
        let _ = fs::rename(&store, src);
        anyhow::bail!("写配置失败，已把本体搬回原处：{e}");
    }

    // 链接统一交给 sync 建，不在这里手工建。两个理由：
    // 1. `private add` 时本体来自公共库，那里不该留链接（留了等于没私有化），
    //    该建链接的位置是 agent 自己的 skills 目录，不是 src
    // 2. 建链接、清挡路目录、备份的判断逻辑只写一份，避免两条路各写一遍
    let log = sync_private_links(false)?;
    let detail = log
        .iter()
        .find(|l| l.contains(&name))
        .cloned()
        .unwrap_or_else(|| "链接状态未知，跑一次「修复链接」确认".to_string());

    Ok(format!(
        "已移入专用库 {}，仅 {agent} 可见；{detail}",
        crate::config::contract_tilde(&store)
    ))
}

/// 重建所有专用 skill 的链接，返回逐条日志。
///
/// 幂等：已经是对的链接直接跳过。这是「修复链接」按钮和
/// `skills private sync` 的共同实现。
pub fn sync_private_links(dry_run: bool) -> anyhow::Result<Vec<String>> {
    let cfg = crate::config::Config::load()?;
    let mut log = Vec::new();

    if cfg.skills.mappings.is_empty() {
        log.push("没有专用 skill 记录".to_string());
        return Ok(log);
    }

    for m in &cfg.skills.mappings {
        let Some(root) = m.store_dir() else {
            log.push(format!("[跳过] {} 没有归属 agent，记录无效", m.skill));
            continue;
        };
        let store = root.join(&m.skill);

        if !store.exists() {
            log.push(format!(
                "[错误] {} 的本体不存在：{}。需要取消这条记录，或把本体放回去",
                m.skill,
                crate::config::contract_tilde(&store)
            ));
            continue;
        }

        let targets = m.targets();
        if targets.len() < m.agents.len() {
            // 配置里写了不认识的 agent id
            let known: Vec<&str> = targets.iter().map(|(a, _)| a.as_str()).collect();
            log.push(format!(
                "[警告] {} 的归属记录里有不认识的 agent（认识的是 {known:?}）",
                m.skill
            ));
        }

        for (agent_id, target) in targets {
            let (state, _, _) = inspect_one_link(&target, &store);
            match state {
                PrivateLinkState::Ok => {
                    log.push(format!("[最新] {} → {} 已是最新，跳过", m.skill, agent_id));
                }
                PrivateLinkState::Missing | PrivateLinkState::Broken => {
                    if dry_run {
                        log.push(format!(
                            "[待修] {} → {} 当前是 {state:?}，需要重建链接",
                            m.skill, agent_id
                        ));
                        continue;
                    }

                    // 目标目录必须存在。不替用户创建 agent 的 skills 根目录 ——
                    // 那通常意味着 agent 没装，凭空造一个目录只会让下次扫描混乱
                    if let Some(parent) = target.parent() {
                        if !parent.exists() {
                            log.push(format!(
                                "[跳过] {} → {} 的 skills 目录不存在（{}），agent 可能没装",
                                m.skill,
                                agent_id,
                                crate::config::contract_tilde(parent)
                            ));
                            continue;
                        }
                    }

                    // 清掉挡路的东西。真实目录要备份 —— 里面可能有用户改动
                    if link::is_link(&target) {
                        fs::remove_file(&target).or_else(|_| fs::remove_dir(&target))?;
                    } else if target.exists() {
                        let bak = link::backup_dir_copy(&target)?;
                        fs::remove_dir_all(&target)?;
                        log.push(format!(
                            "[备份] {} → {} 的原有内容已备份到 {}",
                            m.skill,
                            agent_id,
                            bak.display()
                        ));
                    }

                    match link::create_link(&target, &store, m.mode) {
                        Ok((kind, _)) => log.push(format!(
                            "[重建] {} → {} 建立 {kind:?}",
                            m.skill, agent_id
                        )),
                        Err(e) => log.push(format!(
                            "[失败] {} → {} 建链接失败：{e}",
                            m.skill, agent_id
                        )),
                    }
                }
            }
        }
    }

    Ok(log)
}

/// 取消一条专用记录。
///
/// `to_shared = true`：本体移回公共库 `~/.agents/skills/<name>`，
/// agent 目录里的链接全删掉 —— 这个 skill 变成所有 agent 都能用。
/// `to_shared = false`：本体先复制进备份目录，然后删掉，链接一起删。
///
/// 两种都会删掉配置记录。先动磁盘再动配置，和 `unmap` 的顺序一致：
/// 反过来会出现「记录没了但链接还在」，而链接删除失败时用户还能重试。
pub fn remove_private(skill: &str, to_shared: bool) -> anyhow::Result<String> {
    let mut cfg = crate::config::Config::load()?;
    let Some(m) = cfg.skills.mapping_for(skill).cloned() else {
        anyhow::bail!("没有 {skill} 的专用记录。跑 `skills private list` 看现有记录");
    };

    let store = m
        .store_dir()
        .map(|r| r.join(skill))
        .ok_or_else(|| anyhow::anyhow!("{skill} 的记录没有归属 agent，无法定位本体"))?;

    // 1. 删掉各 agent 目录里的链接。删链接永远安全：不碰本体
    let mut removed_links = 0;
    for (agent_id, target) in m.targets() {
        if link::is_link(&target) {
            fs::remove_file(&target).or_else(|_| fs::remove_dir(&target)).map_err(|e| {
                anyhow::anyhow!("删除 {agent_id} 的链接失败：{e}")
            })?;
            removed_links += 1;
        } else if target.exists() {
            // 是真实目录，不是我们建的链接。不删 —— 可能是用户自己放的东西
            // 但要让调用方知道这里留了东西
            removed_links += 0;
        }
    }

    // 2. 处理本体
    let body_note = if !store.exists() {
        format!("本体已不存在（{}），只清理了记录和链接", crate::config::contract_tilde(&store))
    } else if to_shared {
        let shared = shared_skills_dir();
        let dst = shared.join(skill);
        if dst.exists() || link::is_link(&dst) {
            anyhow::bail!(
                "公共库里已经有同名 skill（{}）。链接已删除，本体仍在 {}，请先处理公共库那份再试",
                crate::config::contract_tilde(&dst),
                crate::config::contract_tilde(&store)
            );
        }
        fs::create_dir_all(&shared)?;
        fs::rename(&store, &dst).map_err(|e| {
            anyhow::anyhow!(
                "搬到公共库失败：{e}。链接已删除，本体仍在 {}",
                crate::config::contract_tilde(&store)
            )
        })?;
        format!("本体已移入公共库 {}", crate::config::contract_tilde(&dst))
    } else {
        let bak = link::backup_dir_copy(&store)?;
        fs::remove_dir_all(&store)?;
        format!("本体已备份到 {} 后删除", bak.display())
    };

    // 3. 最后删配置记录
    cfg.skills.remove_mapping(skill);
    cfg.save()?;

    let mode = if to_shared { "移入公共库" } else { "备份后删除" };
    Ok(format!(
        "已取消 {skill} 的专用（{mode}）：删除 {removed_links} 处链接；{body_note}"
    ))
}

/// 把散落目录移进中心库，并在原位置建链接指回来。
fn import_one(src: &Path, name: &str, shared: &Path) -> anyhow::Result<String> {
    fs::create_dir_all(shared)?;
    let dst = shared.join(name);

    if dst.exists() {
        anyhow::bail!("中心库已存在同名目录 {name}，要覆盖请用「采用散落版」");
    }

    fs::rename(src, &dst)?;
    match link::create_link(src, &dst, crate::types::LinkMode::Auto) {
        Ok((kind, _)) => Ok(format!("已移入中心库，原位置建立 {kind:?}")),
        Err(e) => Ok(format!("已移入中心库，但原位置建链接失败：{e}")),
    }
}

/// 删掉散落那一份，在原位置建一个指向中心库的链接。
fn replace_with_link(src: &Path, dst: &Path, what: &str) -> anyhow::Result<String> {
    if src.is_dir() && !link::is_link(src) {
        fs::remove_dir_all(src)?;
    } else {
        fs::remove_file(src).or_else(|_| fs::remove_dir(src))?;
    }
    match link::create_link(src, dst, crate::types::LinkMode::Auto) {
        Ok((kind, _)) => Ok(format!("已删除{what}，原位置建立 {kind:?} 指向中心库")),
        Err(e) => anyhow::bail!("{what}已删，但原位置建链接失败：{e}"),
    }
}
