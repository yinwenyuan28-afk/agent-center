//! Cursor。
//!
//! 两个能力：指令文件、MCP。provider 的 key 存在系统凭据库，
//! 不在配置文件里，所以不纳入 provider 管理。
//!
//! Cursor 原生读取 `~/.agents/skills`，但自己的 `~/.cursor/skills`
//! 里也常有独立安装的 skill，那部分仍要收集。

use super::Agent;
use crate::mcp::inject::{self as mcp_inject, McpFilePlan};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Cursor;

impl Agent for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn label(&self) -> &'static str {
        "Cursor"
    }

    fn order(&self) -> u32 {
        110
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".cursor")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".cursor").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".cursor").join("skills")
    }

    fn reads_shared_skills(&self) -> bool {
        true
    }

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".cursor").join("mcp.json").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_cursor())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        mcp_inject::plan_servers_map(
            &home_dir().join(".cursor").join("mcp.json"),
            "cursor",
            &owned,
        )
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("密钥存在系统凭据库中，不在配置文件里")
    }
}
