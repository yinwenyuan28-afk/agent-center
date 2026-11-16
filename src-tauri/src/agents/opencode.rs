//! opencode。
//!
//! 三个能力：指令文件、provider、MCP。
//!
//! 注意 `auth.json` 在 `~/.local/share/opencode/` 下，不在 `.config/opencode/` 里。
//!
//! 具体注入逻辑在 [`crate::providers::opencode`]。

use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct OpenCode;

impl OpenCode {
    fn config_path() -> PathBuf {
        home_dir()
            .join(".config")
            .join("opencode")
            .join("opencode.json")
    }

    fn auth_path() -> PathBuf {
        home_dir()
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json")
    }
}

impl Agent for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn label(&self) -> &'static str {
        "opencode"
    }

    fn order(&self) -> u32 {
        50
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".config").join("opencode")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(Self::config_path().with_file_name("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir()
            .join(".config")
            .join("opencode")
            .join("skills")
    }

    fn reads_shared_skills(&self) -> bool {
        true
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::config_path().exists() || Self::auth_path().exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path(), Self::auth_path()]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_opencode())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::opencode::OpencodeInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        Self::config_path().exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_opencode())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::opencode::OpencodeMcpInjector.plan(&owned)
    }
}
