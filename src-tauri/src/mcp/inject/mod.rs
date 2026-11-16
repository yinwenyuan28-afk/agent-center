//! MCP 注入器。
//!
//! 一个工具一个文件，共用的转换与清理逻辑在 [`shared`]。
//!
//! 每个注入器只做三件事：读现有文件 → 只替换连接字段 → 写回。
//! 工具自己的字段（`timeout`、`tools`、`env_vars`、`exposure` 之类）原样保留，
//! 这是「用户在工具里改过的设置不被洗掉」的前提。各工具的私有字段清单见
//! `plan/mcp-management.md`。
//!
//! ## 取消分配的两派
//!
//! 目标工具认「停用」字段时只标停用、留下条目（Pi / Oh My Pi / Antigravity），
//! 不认时删条目（Claude / Codex / OpenCode / Gemini / Grok / Hermes / Kimi / MiniMax）。
//!
//! ## 只操作母版里有的 id
//!
//! 用户手动配的、WorkBuddy 的 83 个市场连接器都不在母版里，
//! 任何注入器的清理逻辑都不会碰到它们。

pub mod agy;
pub mod codex;
pub mod copilot;
pub mod dsh;
pub mod gemini;
pub mod grok;
pub mod hermes;
pub mod kimi;
pub mod map_agent;
pub mod minimax;
pub mod omp;
pub mod opencode;
pub mod pi;
pub mod shared;
pub mod workbuddy;
pub mod zcode;

use super::model::{McpServer, McpSyncReport};
use crate::providers::safe_write::{atomic_write, ensure_first_write_backup, hash_file};
use std::path::PathBuf;

/// 一次注入要写的文件计划。
#[derive(Debug, Clone)]
pub struct McpFilePlan {
    pub path: PathBuf,
    pub agent: String,
    pub content: String,
    /// 规划时文件的 hash。写入前重算比对，变了就中止
    pub pre_hash: String,
    pub changed: bool,
    pub summary: Vec<String>,
}

/// 注入器统一接口。
pub trait McpInjector {
    fn agent(&self) -> &'static str;
    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>>;
}

/// 母版里的某个 server 是否分配给某个 agent。
///
/// **没列出的 agent 就是「不该有这个 server」**，注入时要把它从那个
/// agent 的配置里清掉。
pub fn assigned_to(s: &McpServer, agent: &str) -> bool {
    s.agents.iter().any(|a| a == agent)
}

/// 执行注入：先全部 plan，任一失败就中止，全部成功才写。
pub fn sync(servers: &[McpServer], dry_run: bool) -> anyhow::Result<McpSyncReport> {
    let mut report = McpSyncReport {
        ok: true,
        written: Vec::new(),
        skipped: Vec::new(),
        backups: Vec::new(),
        log: Vec::new(),
    };

    // 池子为空时不做任何事。否则「母版清空」会被理解成「把所有 agent 的 MCP 全删」
    if servers.is_empty() {
        report.log.push("母版里没有 server，不写任何文件".to_string());
        return Ok(report);
    }

    // 第一遍：全部规划。任一 agent 失败就整次中止，避免写一半
    let mut all_plans: Vec<(String, Vec<McpFilePlan>)> = Vec::new();
    for agent in crate::agents::mcp_capable() {
        let id = agent.id().to_string();
        let refs: Vec<&McpServer> = servers.iter().collect();
        match agent.mcp_plan(&refs) {
            Ok(plans) => {
                if !plans.is_empty() {
                    all_plans.push((id, plans));
                }
            }
            Err(e) => {
                anyhow::bail!("{} 规划失败，已中止（未写入任何文件）：{e}", id);
            }
        }
    }

    // 第二遍：执行
    for (agent, plans) in &all_plans {
        for plan in plans {
            if !plan.changed {
                report.skipped.push(plan.path.display().to_string());
                continue;
            }

            if dry_run {
                report.written.push(plan.path.display().to_string());
                report
                    .log
                    .push(format!("[预览] {agent}: 将写入 {}", plan.path.display()));
                for line in &plan.summary {
                    report.log.push(format!("        {line}"));
                }
                continue;
            }

            // hash 复查：计划之后文件被外部改过就中止
            let current = hash_file(&plan.path)?;
            if current != plan.pre_hash {
                report.ok = false;
                report.log.push(format!(
                    "[冲突] {agent}: {} 在计划后被修改，已中止",
                    plan.path.display()
                ));
                return Ok(report);
            }

            if let Ok(Some(b)) = ensure_first_write_backup(&plan.path) {
                report.backups.push(b.display().to_string());
            }

            match atomic_write(&plan.path, plan.content.as_bytes()) {
                Ok(()) => {
                    report.written.push(plan.path.display().to_string());
                    report
                        .log
                        .push(format!("[写入] {agent}: {}", plan.path.display()));
                    for line in &plan.summary {
                        report.log.push(format!("        {line}"));
                    }
                }
                Err(e) => {
                    report.ok = false;
                    report.log.push(format!("[失败] {agent}: {e}"));
                    return Ok(report);
                }
            }
        }
    }

    Ok(report)
}
