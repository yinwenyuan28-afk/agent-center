//! MCP 分配矩阵：server 池 × agent 的勾选关系。
//!
//! 这是用户真正想要的核心视图——不是「同步」，而是
//! 「我配好的所有 MCP server 里，每个 agent 启用哪些」。
//!
//! 数据来源有两部分：
//! - **分配关系**存在母版每个 server 的 `agents` 字段里
//! - **池子**是母版里的全部 server
//!
//! **列来自 `agents::all()` 里支持 MCP 注入的那些**，
//! 这个模块不再自己维护 agent 清单。

use super::model::McpServer;
use super::scan;
use crate::agents;
// 这三个结构定义在 types.rs 里，和前端契约一一对应；
// 这里 pub use 出去，让 commands 能通过 mcp::matrix::McpMatrix 引用
pub use crate::types::{AgentColumn, MatrixRow, McpMatrix};

/// 构建分配矩阵。
pub fn build() -> anyhow::Result<McpMatrix> {
    let servers = super::store::load()?;
    let scans = scan::scan_all();

    // 每个 agent 是否可用。优先用扫描结果里的 present，
    // 扫描不到（如未声明 mcp 能力）就看配置文件在不在。
    let columns: Vec<AgentColumn> = agents::mcp_capable()
        .iter()
        .map(|a| {
            let id = a.id();
            let scan = scans.iter().find(|s| s.agent == id);
            let available = scan.map(|s| s.present).unwrap_or_else(|| a.mcp_ready());
            let note = scan
                .and_then(|s| s.unsupported_reason.clone())
                .unwrap_or_default();
            AgentColumn {
                agent: id.to_string(),
                label: a.label().to_string(),
                available,
                note,
                single_slot: a.single_slot(),
                supported_kinds: a
                    .supported_kinds()
                    .iter()
                    .map(|k| k.as_str().to_string())
                    .collect(),
            }
        })
        .collect();

    let empty = servers.is_empty();

    let rows: Vec<MatrixRow> = servers
        .iter()
        .map(|s: &McpServer| MatrixRow {
            id: s.id.clone(),
            name: s.display_name().to_string(),
            transport: s.transport.as_str().to_string(),
            target: if s.is_remote() {
                s.url.clone()
            } else {
                format!("{} {}", s.command, s.args.join(" "))
                    .trim()
                    .to_string()
            },
            enabled: s.enabled,
            assigned: s.agents.clone(),
            note: s.note.clone(),
        })
        .collect();

    Ok(McpMatrix {
        columns,
        rows,
        empty,
    })
}

/// 设置某个 server 在某个 agent 的启用状态。
///
/// 这是矩阵格子点击的处理：勾选就往 `agents` 里加，取消就移除。
/// **只改母版**，不碰 agent 配置——要生效需再执行注入。
pub fn set_assignment(server_id: &str, agent: &str, on: bool) -> anyhow::Result<()> {
    let mut servers = super::store::load()?;

    let Some(s) = servers.iter_mut().find(|s| s.id == server_id) else {
        anyhow::bail!("母版里没有 server：{server_id}");
    };

    if on {
        if !s.agents.iter().any(|a| a == agent) {
            s.agents.push(agent.to_string());
        }
    } else {
        s.agents.retain(|a| a != agent);
    }

    super::store::save(&servers)
}

/// 批量设置：把某个 server 的分配列表整体替换。
///
/// 界面上一行有多个勾选框，一次保存整行的分配关系更可靠，
/// 避免多次单点写入造成中间态。
pub fn set_row(server_id: &str, agents: &[String]) -> anyhow::Result<()> {
    let mut servers = super::store::load()?;

    let Some(s) = servers.iter_mut().find(|s| s.id == server_id) else {
        anyhow::bail!("母版里没有 server：{server_id}");
    };

    // 去重并过滤掉不认识的 agent 名
    let known: Vec<&'static str> = agents::mcp_capable().iter().map(|a| a.id()).collect();
    let mut clean: Vec<String> = Vec::new();
    for a in agents {
        if known.contains(&a.as_str()) && !clean.contains(a) {
            clean.push(a.clone());
        }
    }

    s.agents = clean;
    super::store::save(&servers)
}

/// 从各 agent 的现有配置导入 server 到池子。
///
/// 这是首次使用的引导步骤：扫描所有 agent，把找到的 server 合并成池子，
/// 并记录每个 server 在哪些 agent 里出现过（作为初始分配）。
///
/// **只加不覆盖**：池子里已有的 id 只合并 agents，不改配置。
pub fn import_from_agents() -> anyhow::Result<Vec<String>> {
    let mut servers = super::store::load()?;
    let scans = scan::scan_all();
    let mut added = Vec::new();

    for scan in &scans {
        for f in &scan.servers {
            // 跳过 WorkBuddy 的市场连接器（connector: 前缀，全是 disabled）
            if f.id.starts_with("connector:") {
                continue;
            }
            // 跳过未启用的
            if !f.enabled {
                continue;
            }

            match servers.iter_mut().find(|s| s.id == f.id) {
                Some(existing) => {
                    // 已存在：只合并 agent 归属，不改配置
                    if !existing.agents.iter().any(|a| a == &scan.agent) {
                        existing.agents.push(scan.agent.clone());
                    }
                }
                None => {
                    // 新建：从扫描结果还原
                    let transport = super::model::Transport::parse(&f.transport);
                    servers.push(McpServer {
                        id: f.id.clone(),
                        name: f.name.clone(),
                        transport,
                        command: f.command.clone(),
                        args: f.args.clone(),
                        cwd: String::new(),
                        url: f.url.clone(),
                        headers: Default::default(),
                        env: Default::default(),
                        enabled: true,
                        agents: vec![scan.agent.clone()],
                        note: format!("从 {} 导入", f.location),
                    });
                    added.push(f.id.clone());
                }
            }
        }
    }

    if !added.is_empty() {
        super::store::save(&servers)?;
    }

    Ok(added)
}
