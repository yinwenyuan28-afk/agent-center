//! MCP 母版与各 agent 现状的差异对比。
//!
//! **完全只读**。和 provider 的 diff 一样，先看清楚再决定改不改。

use super::model::{McpDiffLine, McpDiffReport, McpServer};
use super::scan::{self, McpAgentScan};

/// 比对母版与扫描结果。
pub fn compare(master: &[McpServer], scans: &[McpAgentScan]) -> McpDiffReport {
    let mut lines = Vec::new();
    let mut changed_files = 0usize;

    for scan in scans {
        // 该 agent 的母版意图
        let wanted: Vec<&McpServer> = master
            .iter()
            .filter(|m| m.agents.iter().any(|a| a == &scan.agent))
            .collect();

        if wanted.is_empty() && scan.servers.is_empty() {
            continue;
        }

        // 母版要求的 server
        for m in &wanted {
            let found = scan.servers.iter().find(|s| s.id == m.id);

            match found {
                None => {
                    changed_files += 1;
                    lines.push(McpDiffLine {
                        agent: scan.agent.clone(),
                        path: agent_path(&scan.agent),
                        action: "add".into(),
                        target: m.id.clone(),
                        detail: format!(
                            "新增 {}（{}）",
                            m.display_name(),
                            if m.is_remote() { &m.url } else { &m.command }
                        ),
                    });
                }
                Some(f) => {
                    // 比对关键字段
                    let mut diffs = Vec::new();

                    if f.transport != m.transport.as_str() {
                        diffs.push(format!("协议 {} → {}", f.transport, m.transport.as_str()));
                    }
                    if !m.is_remote() {
                        if f.command != m.command {
                            diffs.push("command 不同".to_string());
                        }
                        if f.args != m.args {
                            diffs.push("args 不同".to_string());
                        }
                    } else if f.url != m.url {
                        diffs.push("url 不同".to_string());
                    }
                    // 启用状态：母版是权威
                    if f.enabled != m.enabled {
                        diffs.push(format!(
                            "启用状态 {} → {}",
                            if f.enabled { "启用" } else { "停用" },
                            if m.enabled { "启用" } else { "停用" }
                        ));
                    }

                    if diffs.is_empty() {
                        lines.push(McpDiffLine {
                            agent: scan.agent.clone(),
                            path: f.location.clone(),
                            action: "unchanged".into(),
                            target: m.id.clone(),
                            detail: "已一致".into(),
                        });
                    } else {
                        changed_files += 1;
                        lines.push(McpDiffLine {
                            agent: scan.agent.clone(),
                            path: f.location.clone(),
                            action: "update".into(),
                            target: m.id.clone(),
                            detail: diffs.join("；"),
                        });
                    }
                }
            }
        }

        // agent 里有、母版没声明的：保持原样不动
        //
        // 这条规则对 WorkBuddy 尤其重要 —— 它有 83 个市场 connector，
        // 都不是用户主动维护的，绝不能因为母版没提到就被清掉。
        for f in &scan.servers {
            if wanted.iter().any(|m| m.id == f.id) {
                continue;
            }
            // 市场目录里的（带 connector: 前缀或 disabled）不单独报，否则刷屏
            if f.id.starts_with("connector:") || !f.enabled {
                continue;
            }
            lines.push(McpDiffLine {
                agent: scan.agent.clone(),
                path: f.location.clone(),
                action: "unchanged".into(),
                target: f.id.clone(),
                detail: "存在但母版未声明，保持原样不动".into(),
            });
        }
    }

    let agent_count = lines
        .iter()
        .filter(|l| l.action != "unchanged")
        .map(|l| l.agent.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    McpDiffReport {
        lines,
        agent_count,
        changed_files,
    }
}

/// 各 agent 的主配置文件路径，用于 diff 展示。
///
/// 从 agent 实现里取。MCP 的文件路径藏在各注入器内部，
/// 这里用一个静态表补齐 —— 但它只在「新增」行里用到，
/// 已经有 server 的情况会显示扫描到的真实 location。
fn agent_path(agent: &str) -> String {
    match agent {
        "claude" => "~/.claude.json".into(),
        // 其余用统一格式拼，避免每加一个 agent 就要改这张表
        other => match crate::agents::find(other) {
            Some(a) => {
                let label = a.label().to_string();
                format!("{label} 的 MCP 配置文件")
            }
            None => other.to_string(),
        },
    }
}

/// 便捷入口。
pub fn run() -> anyhow::Result<McpDiffReport> {
    let master = super::store::load()?;
    let scans = scan::scan_all();
    Ok(compare(&master, &scans))
}
