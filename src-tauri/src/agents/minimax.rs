//! MiniMax Code（MCode）。
//!
//! ## provider：多 provider 并存
//!
//! `~/.minimax/config.yaml` 的 `custom_provider.<key>` 下可以有多家。
//! `defaultModel` 归 MCode 自己管，本工具只读不写。
//!
//! **本机未安装**，实现是按 cc-switch 的 `mcode_config.rs` 移植的。

use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use std::path::PathBuf;

pub struct Minimax;

impl Minimax {
    fn dir() -> PathBuf {
        for var in ["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"] {
            if let Ok(v) = std::env::var(var) {
                let v = v.trim();
                if !v.is_empty() {
                    return PathBuf::from(v);
                }
            }
        }
        crate::providers::store::home_dir().join(".minimax")
    }

    fn config_path() -> PathBuf {
        Self::dir().join("config.yaml")
    }
}

impl Agent for Minimax {
    fn id(&self) -> &'static str {
        "minimax"
    }

    fn label(&self) -> &'static str {
        "MiniMax Code"
    }

    fn order(&self) -> u32 {
        220
    }

    fn config_dir(&self) -> PathBuf {
        Self::dir()
    }

    fn skills_dir(&self) -> PathBuf {
        Self::dir().join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::config_path().exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_minimax())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::minimax::MinimaxInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        crate::mcp::inject::minimax::config_path().exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_minimax())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::minimax::MinimaxMcpInjector.plan(&owned)
    }
}
