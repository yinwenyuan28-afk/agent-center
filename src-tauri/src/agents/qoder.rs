//! Qoder。
//!
//! 两个能力：MCP、skills。用户级没有单一指令文件（只有工作区级），
//! provider 走浏览器 OAuth，本地无明文 key。

use super::Agent;
use crate::mcp::inject::{self as mcp_inject, McpFilePlan};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Qoder;

impl Agent for Qoder {
    fn id(&self) -> &'static str {
        "qoder"
    }

    fn label(&self) -> &'static str {
        "Qoder"
    }

    fn order(&self) -> u32 {
        120
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".qoder")
    }

    // instruction_file 用默认 None：用户级没有单一指令文件

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".qoder").join("skills")
    }

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".qoder").join("settings.json").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_qoder())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        mcp_inject::map_agent::plan_map(&mcp_inject::map_agent::qoder(), &owned)
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
