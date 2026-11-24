//! 母版 AGENTS.md 的读取、创建与写入，以及「通用 + 每个 agent 专用」的合成。
//!
//! 单独成一个模块，因为它是整个工具唯一一份"内容本体"：
//! 链接引擎只负责把它映射出去，这里负责它的内容本身。
//!
//! # 三层文件
//!
//! ```text
//! ~/.agents/AGENTS.md              通用片段，所有 agent 都读
//! ~/.agents/AGENTS.d/<agent>.md    某个 agent 的专用片段。存在才算数
//! ~/.agents/composed/<agent>.md    两者合成的产物，派生物
//! ```
//!
//! 有专用片段的 agent，链接指向 `composed/<agent>.md`；没有的，仍直接链接
//! 通用片段 —— 这样升级本工具不会改动任何一个已有映射，编辑通用文档也立刻
//! 对所有 agent 生效。只有真的需要区别对待的 agent 才多一层合成。
//!
//! # 三条安全约定
//!
//! 1. 读取时文件不存在不算错误，返回 exists=false 让界面引导创建
//! 2. `create` 只在文件不存在时写，避免覆盖已有内容
//! 3. `write` 直接覆盖，但调用方（界面）必须先让用户看到原内容
//!
//! # 为什么合成必须原地写
//!
//! [`crate::link::create_link`] 对文件按 symlink → hardlink → copy 降级。
//! Windows 上没开开发者模式时多半落到**硬链接**，此时合成文件与 agent 的
//! 指令文件是同一个 inode。
//!
//! 若合成时用「写临时文件再 rename」这种原子写法，rename 会产生新 inode，
//! 而 agent 那边的硬链接仍指向旧 inode —— 内容改了，agent 永远读到旧的。
//! 所以 [`compose_one`] 必须 `fs::write` 原地截断重写。
//!
//! 代价是崩溃时可能留下半截文件。这个取舍成立：合成文件可以随时重建，
//! 真正的用户内容（片段）走备份保护。

use crate::config::{contract_tilde, Config};
use crate::types::{AgentDoc, RunResult};
use serde::Serialize;
use std::path::PathBuf;

/// 母版文件的读取结果。
#[derive(Debug, Clone, Serialize)]
pub struct DocContent {
    /// 文件是否存在
    pub exists: bool,
    /// 文件内容。不存在时为空串
    pub content: String,
}

/// 一次合成动作的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeOutcome {
    /// 哪个 agent
    pub agent: String,
    /// 做了什么：`none` / `created` / `updated` / `removed` / `skipped` / `error`
    pub action: String,
    /// 合成文件路径（`~` 形式）。没有专用片段时为空
    pub path: Option<String>,
    /// 人话说明
    pub message: String,
}

/// 母版文件的实际路径，从配置里取（默认 `~/.agents/AGENTS.md`）。
///
/// 回退路径走 `store::home_dir()`，跟随 `AGENTCENTER_HOME`。
pub fn doc_path() -> std::path::PathBuf {
    Config::load()
        .map(|c| c.source_path())
        .unwrap_or_else(|_| default_doc_path())
}

/// 默认母版路径。配置读不出来时用它。
fn default_doc_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join("AGENTS.md")
}

/// 片段与合成产物所在的根目录：`~/.agents`。
///
/// 不用 `doc_path()` 的父目录 —— 用户可以把通用母版改到别的地方
/// （`[source] instructions`），但片段目录始终跟着 `~/.agents`，
/// 否则改一个配置项就会让所有专用片段"消失"。
fn agents_dir() -> PathBuf {
    crate::providers::store::home_dir().join(".agents")
}

/// 某个 agent 的专用片段路径：`~/.agents/AGENTS.d/<agent>.md`。
pub fn specific_path(agent: &str) -> PathBuf {
    agents_dir()
        .join("AGENTS.d")
        .join(format!("{agent}.md"))
}

/// 某个 agent 的合成产物路径：`~/.agents/composed/<agent>.md`。
pub fn composed_path(agent: &str) -> PathBuf {
    agents_dir().join("composed").join(format!("{agent}.md"))
}

/// 这个 agent 有没有专用片段。
pub fn has_specific(agent: &str) -> bool {
    specific_path(agent).is_file()
}

/// 某个 agent 实际要读的源文件。
///
/// 有专用片段 → 合成产物；没有 → 通用母版。
///
/// 返回路径不代表文件一定存在：合成产物要跑过 [`compose_one`] 才有。
/// 调用方（`config::resolved_mappings`）拿到它交给 `link::sync_one`，
/// 而 sync 自己会处理"源文件不存在"。
pub fn effective_source(agent: &str) -> PathBuf {
    if has_specific(agent) {
        composed_path(agent)
    } else {
        doc_path()
    }
}

/// 读取母版内容。
pub fn read() -> anyhow::Result<DocContent> {
    read_at(&doc_path())
}

/// 读取某个 agent 的专用片段。没有片段时返回 exists=false。
pub fn specific_read(agent: &str) -> anyhow::Result<DocContent> {
    read_at(&specific_path(agent))
}

/// 从指定路径读内容。不存在不算错误。
fn read_at(path: &std::path::Path) -> anyhow::Result<DocContent> {
    if !path.exists() {
        return Ok(DocContent {
            exists: false,
            content: String::new(),
        });
    }
    let content = std::fs::read_to_string(path)?;
    Ok(DocContent {
        exists: true,
        content,
    })
}

/// 覆盖写入母版。父目录不存在时自动创建。
pub fn write(content: &str) -> RunResult {
    write_at(&doc_path(), content, "已保存")
}

/// 写入某个 agent 的专用片段。
///
/// 内容 trim 后为空 = 取消这个 agent 的专用内容：删掉片段文件，
/// 并顺手清理它的合成产物，让链接下次 sync 时收回指向通用母版。
pub fn specific_write(agent: &str, content: &str) -> RunResult {
    // 校验 agent 是我们的注册表里认识的那些。认不出来就说明调用方传错了，
    // 写下去只会留下一个永远不会被读到的孤儿文件。
    if crate::agents::find(agent).is_none() {
        return RunResult::failure(1, format!("不认识的 agent：{agent}"));
    }

    let path = specific_path(agent);

    if content.trim().is_empty() {
        if !path.exists() {
            return RunResult::success(format!("{agent} 本来就没有专用片段"));
        }
        // 先备份再删。用户可能只是想临时清空，内容还得能找回来
        let backup_note = backup_before_remove(&path);
        if let Err(e) = std::fs::remove_file(&path) {
            return RunResult::failure(1, format!("删除片段失败：{e}"));
        }
        // 合成产物是派生物，跟着清掉，否则链接会继续指向它
        let composed = composed_path(agent);
        if composed.exists() {
            let _ = std::fs::remove_file(&composed);
        }
        return RunResult::success(format!(
            "已取消 {agent} 的专用片段{backup_note}；下次同步会改用通用母版"
        ));
    }

    // 覆盖前备份旧版。第一次写入时文件不存在，跳过备份
    let backup_note = if path.exists() {
        backup_before_remove(&path)
    } else {
        String::new()
    };

    match write_at(&path, content, "已保存专用片段") {
        r if r.ok => {
            let mut r = r;
            if !backup_note.is_empty() {
                r.output.push_str(&backup_note);
            }
            r
        }
        r => r,
    }
}

/// 把一个文件复制进 `~/.agents/backup/<时间戳>/`，返回说明文字。
///
/// 复制而不是移动：调用方随后可能还要读它（比如拿它和旧内容比对）。
fn backup_before_remove(path: &std::path::Path) -> String {
    let Ok(bak) = crate::link::backup_file_copy(path) else {
        // 备份失败不该阻止主流程 —— 用户要的是"保存"或"删除"，
        // 备份只是保险。失败时明确说出来，别假装成功了。
        return "（旧版备份失败）".to_string();
    };
    format!("；旧版已备份到 {}", bak.display())
}

/// 往指定路径写内容，父目录自动创建。
fn write_at(path: &std::path::Path, content: &str, verb: &str) -> RunResult {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return RunResult::failure(1, format!("创建目录失败：{e}"));
        }
    }
    match std::fs::write(path, content) {
        Ok(()) => RunResult::success(format!(
            "{verb} {}（{} 字节）",
            contract_tilde(path),
            content.len()
        )),
        Err(e) => RunResult::failure(1, format!("写入失败：{e}")),
    }
}

/// 创建母版文件。已存在时拒绝，把判断留给用户而不是直接覆盖。
pub fn create(content: &str) -> RunResult {
    let path = doc_path();
    if path.exists() {
        return RunResult::failure(
            1,
            format!(
                "{} 已存在，未覆盖。要修改请用编辑功能",
                contract_tilde(&path)
            ),
        );
    }
    write(content)
}

/// 把「通用 + 专用」拼成一份完整文档。
///
/// 拼接规则简单到没什么可调的：通用在前，空行分隔，专用在后。
/// 不做标题包装 —— 用户写片段时自己控制标题层级，工具加标题反而
/// 会和片段里的标题打架。
fn join_parts(common: &str, specific: &str) -> String {
    let common = common.trim_end();
    let specific = specific.trim_start();
    if common.is_empty() {
        return format!("{specific}\n");
    }
    if specific.is_empty() {
        return format!("{common}\n");
    }
    format!("{common}\n\n{specific}\n")
}

/// 生成（或刷新）某个 agent 的合成文件。
///
/// `apply = false` 时只判断需不需要动手，不写盘。
///
/// 幂等靠内容比对实现：拼出来的内容和磁盘上的一模一样就跳过，
/// 连 mtime 都不动。这样 `sync` 连跑两次第二次是真正的零改动。
pub fn compose_one(agent: &str, apply: bool) -> ComposeOutcome {
    let specific = specific_path(agent);

    let fail = |msg: String| ComposeOutcome {
        agent: agent.to_string(),
        action: "error".to_string(),
        path: Some(contract_tilde(&specific)),
        message: msg,
    };

    if !specific.exists() {
        // 没有专用片段：不该有合成产物。留着的旧产物要清掉，
        // 否则链接会继续指向一份不再更新的快照。
        let composed = composed_path(agent);
        if composed.exists() {
            if apply {
                if let Err(e) = std::fs::remove_file(&composed) {
                    return fail(format!("清理旧合成文件失败：{e}"));
                }
                return ComposeOutcome {
                    agent: agent.to_string(),
                    action: "removed".to_string(),
                    path: Some(contract_tilde(&composed)),
                    message: "专用片段已删除，合成文件一并清理".to_string(),
                };
            }
            return ComposeOutcome {
                agent: agent.to_string(),
                action: "skipped".to_string(),
                path: Some(contract_tilde(&composed)),
                message: "专用片段已删除，合成文件待清理".to_string(),
            };
        }
        return ComposeOutcome {
            agent: agent.to_string(),
            action: "none".to_string(),
            path: None,
            message: "没有专用片段，直接使用通用母版".to_string(),
        };
    }

    let common = match std::fs::read_to_string(doc_path()) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 通用母版还没建，但专用片段已经写了。合成一份只有专用的文档，
            // 让 agent 至少能读到专用内容；同时把情况说清楚。
            String::new()
        }
        Err(e) => return fail(format!("读通用母版失败：{e}")),
    };

    let specific_text = match std::fs::read_to_string(&specific) {
        Ok(s) => s,
        Err(e) => return fail(format!("读专用片段失败：{e}")),
    };

    let merged = join_parts(&common, &specific_text);
    let composed = composed_path(agent);

    // 内容一致就跳过。用读文件比对而不是记哈希：合成文件通常只有几十 KB，
    // 读一遍比维护一份额外的指纹记录更不容易出错
    if composed.exists() {
        if let Ok(existing) = std::fs::read_to_string(&composed) {
            if existing == merged {
                return ComposeOutcome {
                    agent: agent.to_string(),
                    action: "skipped".to_string(),
                    path: Some(contract_tilde(&composed)),
                    message: "已是最新".to_string(),
                };
            }
        }
    }

    if !apply {
        return ComposeOutcome {
            agent: agent.to_string(),
            action: "skipped".to_string(),
            path: Some(contract_tilde(&composed)),
            message: if composed.exists() {
                "内容有变化，待重建".to_string()
            } else {
                "合成文件不存在，待创建".to_string()
            },
        };
    }

    // 关键：原地写，不能用临时文件 + rename。原因见模块注释
    let existed = composed.exists();
    let result = write_at(&composed, &merged, if existed { "已更新" } else { "已生成" });

    let note = if common.is_empty() {
        "（通用母版不存在，只合成了专用片段）"
    } else {
        ""
    };

    if result.ok {
        ComposeOutcome {
            agent: agent.to_string(),
            action: if existed { "updated" } else { "created" }.to_string(),
            path: Some(contract_tilde(&composed)),
            message: format!("{}{note}", result.output),
        }
    } else {
        fail(result.output)
    }
}

/// 给所有有指令文件的 agent 生成合成文件。
///
/// 只有注册表里声明了 `instruction_file()` 的 agent 才参与 —— 和
/// `resolved_mappings()` 的过滤条件一致，避免为不参与映射的 agent
/// 白造一堆没人读的文件。
pub fn compose_all(apply: bool) -> Vec<ComposeOutcome> {
    let mut out = Vec::new();
    for agent in crate::agents::all() {
        if agent.instruction_file().is_none() {
            continue;
        }
        // 没有专用片段的 agent 不必逐条报告，否则 11 行里 10 行是"没什么可做"
        let outcome = compose_one(agent.id(), apply);
        if outcome.action == "none" {
            continue;
        }
        out.push(outcome);
    }
    out
}

/// 列出所有可接管 agent 的片段与合成状态。纯只读，界面和 CLI 共用。
pub fn list_agent_docs() -> Vec<AgentDoc> {
    let common = doc_path();
    let common_exists = common.is_file();
    let common_text = if common_exists {
        std::fs::read_to_string(&common).unwrap_or_default()
    } else {
        String::new()
    };
    let common_len = common_text.len() as u64;

    // 用户在 agentcenter.toml 里可以把某个 agent 的落点改到别处。
    // 文档页显示的必须是**实际落点**，不能是 agent 内置的默认路径 ——
    // 否则同一页的「映射」表格和「指令文档」标签会对同一个 agent 显示两个目标。
    let cfg = crate::config::Config::load().ok();

    let mut out = Vec::new();
    for agent in crate::agents::all() {
        let Some(default_target) = agent.instruction_file() else {
            continue;
        };
        let id = agent.id();
        let target = cfg
            .as_ref()
            .and_then(|c| c.mappings.iter().find(|m| m.agent == id))
            .map(|m| crate::config::expand_tilde(&m.to))
            .unwrap_or(default_target);
        let specific = specific_path(id);
        let specific_exists = specific.is_file();

        let (composed_path_view, composed_stale, bytes) = if specific_exists {
            let composed = composed_path(id);
            let specific_text = std::fs::read_to_string(&specific).unwrap_or_default();
            let merged = join_parts(&common_text, &specific_text);
            // 待生成 = 合成文件不存在，或者内容与当前片段拼出来的不一致
            let stale = match std::fs::read_to_string(&composed) {
                Ok(existing) => existing != merged,
                Err(_) => true,
            };
            (Some(contract_tilde(&composed)), stale, merged.len() as u64)
        } else {
            (None, false, common_len)
        };

        out.push(AgentDoc {
            agent: id.to_string(),
            label: agent.label().to_string(),
            common_path: contract_tilde(&common),
            target: contract_tilde(&target),
            specific_path: contract_tilde(&specific),
            specific_exists,
            composed_path: composed_path_view,
            composed_stale,
            bytes,
            // 通用母版都没有、也没有专用片段时，这个 agent 什么都读不到
            installed: agent.config_dir().exists(),
        });
    }
    out
}

/// 有没有任何 agent 的合成产物落后于片段内容。界面用它决定要不要提示。
pub fn has_stale(docs: &[AgentDoc]) -> bool {
    docs.iter().any(|d| d.composed_stale)
}
