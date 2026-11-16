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
                        // Windows 上注入器会把 `npx` 包成 `cmd /c npx`。
                        // 扫描读到的两侧可能一边包了一边没包，直接比会永远报
                        // 「command 不同」，所以先还原到最内层再比
                        let (fc, fa) = unwrap_cmd(&f.command, &f.args);
                        let (mc, ma) = unwrap_cmd(&m.command, &m.args);
                        if fc != mc {
                            diffs.push("command 不同".to_string());
                        }
                        if fa != ma {
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

/// 各 agent 的主配置文件路径，用于 diff 的「新增」行。
///
/// 已有的 server 会显示扫描到的真实 location，这里只补「还没写进去」的情况。
/// 路径必须和对应注入器的写入目标一致，否则界面会指错文件。
fn agent_path(agent: &str) -> String {
    match agent {
        "claude" => "~/.claude.json".into(),
        "codex" => "~/.codex/config.toml".into(),
        "opencode" => {
            // OpenCode 是三份配置合并读、写进已经有 mcp 段的那份
            let p = crate::mcp::inject::opencode::write_target();
            p.display().to_string()
        }
        "pi" => "~/.pi/agent/mcp.json".into(),
        "zcode" => "~/.zcode/cli/config.json · mcp.servers".into(),
        "workbuddy" => "~/.workbuddy/connectors/default/mcp.json".into(),
        "cursor" => "~/.cursor/mcp.json".into(),
        "qoder" => "~/.qoder/settings.json".into(),
        "hermes" => crate::mcp::inject::hermes::config_path().display().to_string(),
        "dsh" => "~/.dsh/profiles/<profile>/cordis.patch.yml".into(),
        "gemini" => "~/.gemini/settings.json".into(),
        "grok" => "~/.grok/config.toml".into(),
        "kimi" => "~/.kimi-code/mcp.json".into(),
        "minimax" => "~/.minimax/mcp.json".into(),
        "copilot" => "~/.copilot/mcp-config.json".into(),
        "agy" => "~/.gemini/config/mcp_config.json".into(),
        "omp" => "~/.omp/default/agent/mcp.json".into(),
        // 新增 agent 但忘了补这张表时，至少显示它的展示名而不是内部 id
        other => match crate::agents::find(other) {
            Some(a) => {
                let label = a.label().to_string();
                format!("{label} 的 MCP 配置文件")
            }
            None => other.to_string(),
        },
    }
}

/// 把 `cmd /c npx ...` 还原成 `npx ...`。
///
/// 注入器在 Windows 上会给 npx / npm / node 这类命令包一层 `cmd /c`。
/// 母版和扫描结果可能一边包了、一边没包 —— 比之前还原到同一形态。
fn unwrap_cmd(command: &str, args: &[String]) -> (String, Vec<String>) {
    let is_cmd = command.eq_ignore_ascii_case("cmd") || command.eq_ignore_ascii_case("cmd.exe");
    if !is_cmd || args.len() < 2 || args[0] != "/c" {
        return (command.to_string(), args.to_vec());
    }
    let inner = args[1].clone();
    (inner, args[2..].to_vec())
}

/// 便捷入口。
pub fn run() -> anyhow::Result<McpDiffReport> {
    let master = super::store::load()?;
    let scans = scan::scan_all();
    Ok(compare(&master, &scans))
}
