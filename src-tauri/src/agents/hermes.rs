//! Hermes Agent（`~/.hermes` 或 `$HERMES_HOME`）。
//!
//! 「Hermes」这个名字在本机对应两个东西：
//!
//! 1. `~/.hermes/` —— 一个空目录，只有两个备份脚本
//! 2. `C:\dev\ide\hermes-agent\Hermes Agent CN Desktop` —— 真正在用的桌面版，
//!    `HERMES_HOME` 指向它的 `data\hermes-home`
//!
//! 所以配置目录优先读 `HERMES_HOME` 环境变量，取不到才回落到 `~/.hermes`。
//!
//! ## 能管什么
//!
//! `config.yaml` 里有完整结构：
//!
//! - `providers.<id>` —— provider 定义（name / base_url / api_mode / models / api_key）
//! - `mcp_servers.<id>` —— MCP server（command/args/env 或 url/headers）
//! - `model.provider` + `model.default` —— 当前在用的那个
//!
//! 所以 provider 和 MCP 都能管。具体注入逻辑在
//! [`crate::providers::hermes`] 和 [`crate::mcp::inject::HermesMcpInjector`]。

use super::Agent;
use crate::mcp::inject::McpFilePlan;
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Hermes;

impl Hermes {
    /// 配置根目录。优先 `HERMES_HOME`，否则 `~/.hermes`。
    fn home() -> PathBuf {
        if let Ok(v) = std::env::var("HERMES_HOME") {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
        home_dir().join(".hermes")
    }

    fn config_path() -> PathBuf {
        Self::home().join("config.yaml")
    }
}

impl Agent for Hermes {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn label(&self) -> &'static str {
        "Hermes Agent"
    }

    fn order(&self) -> u32 {
        80
    }

    fn config_dir(&self) -> PathBuf {
        Self::home()
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        // Hermes 用 config.yaml + SOUL/IDENTITY 体系，没有单独的 AGENTS.md
        None
    }

    fn skills_dir(&self) -> PathBuf {
        Self::home().join("skills")
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
        Some(crate::providers::scan::scan_hermes())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::hermes::HermesInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        Self::config_path().exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_hermes())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::plan_hermes(&owned)
    }
}
