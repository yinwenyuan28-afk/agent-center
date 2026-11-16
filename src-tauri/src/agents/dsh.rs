//! DSH（DeepSeek Harness）。
//!
//! 三个能力：指令文件、provider、MCP。
//!
//! ## MCP 在哪
//!
//! `~/.dsh/profiles/<profile>/cordis.patch.yml` 是一份 YAML **数组**
//! （loader 的 patch 列表）。MCP server 是数组里 `name` 为
//! `@deepseek-ai/dsh-mcp-client` 的一个条目，可能直接挂在数组里，
//! 也可能嵌在 `- insert: [ ... ]` 分组里。
//!
//! `web`、`desktop`、`headless`、`acp` 四套 profile 各一份文件，
//! 同一个 server 要写进每一个已存在的 profile。
//!
//! 之前这里写着「MCP 段在 cordis.patch.yml 里没有，机制未探明」——
//! 那是错的。当时只 grep 了 `mcp` 字样，没认出 plugin 名是
//! `@deepseek-ai/dsh-mcp-client`。
//!
//! ## provider 的一个已知问题
//!
//! `providers:` 段缩进 4 而不是 0，这个问题记录在
//! `plan/providers-management.md` 的踩坑清单里。
//!
//! 具体注入逻辑在 [`crate::providers::dsh`] 和 [`crate::mcp::inject::dsh`]。

use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Dsh;

impl Dsh {
    fn patch_path() -> PathBuf {
        home_dir()
            .join(".dsh")
            .join("profiles")
            .join("desktop")
            .join("cordis.patch.yml")
    }
}

impl Agent for Dsh {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn label(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn order(&self) -> u32 {
        70
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".dsh")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".dsh").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".dsh").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::patch_path().exists()
            || home_dir().join(".dsh").join(".credentials.yaml").exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![
            Self::patch_path(),
            home_dir().join(".dsh").join(".credentials.yaml"),
        ]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_dsh())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::dsh::DshInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        !crate::mcp::inject::dsh::target_paths().is_empty()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_dsh())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::dsh::DshMcpInjector.plan(&owned)
    }
}
