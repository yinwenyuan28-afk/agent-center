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

use crate::config::SkillTags;
use crate::link;
use crate::types::{CollectPlan, OrphanAction, OrphanActionResult, OrphanRelation, OrphanSkill, Skill};
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
pub fn list_skills(tags: &SkillTags) -> anyhow::Result<Vec<Skill>> {
    let root = shared_skills_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let lock = read_lock_sources();
    let mut out = Vec::new();

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
            name: name.clone(),
            tags: tags.get(&name).cloned().unwrap_or_default(),
            managed: lock.contains_key(&name),
            source: lock.get(&name).cloned(),
            hash: hash.chars().take(8).collect(),
            file_count,
            size_bytes,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
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
    }
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
