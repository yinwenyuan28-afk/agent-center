//! WorkBuddy。
//!
//! 三个能力：provider、MCP、skills（但没有单一指令文件 ——
//! 它是 SOUL/IDENTITY/USER/MEMORY 一整套）。
//!
//! 具体注入逻辑在 [`crate::providers::workbuddy`] 和
//! [`crate::mcp::inject::WorkBuddyMcpInjector`]。

use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct WorkBuddy;

impl WorkBuddy {
    fn models_path() -> PathBuf {
        home_dir().join(".workbuddy").join("models.json")
    }
}

impl Agent for WorkBuddy {
    fn id(&self) -> &'static str {
        "workbuddy"
    }

    fn label(&self) -> &'static str {
        "WorkBuddy"
    }

    fn order(&self) -> u32 {
        60
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".workbuddy")
    }

    // instruction_file 用默认的 None：WorkBuddy 没有单一指令文件

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".workbuddy").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::models_path().exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::models_path()]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_workbuddy())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::workbuddy::WorkBuddyInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir()
            .join(".workbuddy")
            .join("connectors")
            .join("default")
            .join("mcp.json")
            .exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_workbuddy())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::workbuddy::WorkBuddyMcpInjector.plan(&owned)
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
