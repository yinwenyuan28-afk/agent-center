//! Claude Code。
//!
//! 四个能力全有：指令文件（`CLAUDE.md`）、provider、MCP、凭证体检。
//!
//! ## provider：只有一个槽位
//!
//! Claude Code 没有 provider 列表的概念，只有一组 `ANTHROPIC_*` 环境变量
//! 决定当前端点。所以 [`Agent::single_slot`] 返回 true ——
//! 界面上勾某个 provider 的 Claude 时，会自动把其他 provider 上的 claude
//! 摘掉，保证母版里只有一个 provider 声明注入 claude。
//!
//! 具体注入逻辑在 [`crate::providers::claude`]，那里维护着接管的键清单。

use super::model::{collect, read_json, RawEntry};
use super::Agent;
use crate::mcp::inject::{self as mcp_inject, McpFilePlan};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Claude;

impl Agent for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn label(&self) -> &'static str {
        "Claude Code"
    }

    fn order(&self) -> u32 {
        10
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".claude")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        // Claude Code 用 CLAUDE.md，不读 AGENTS.md
        Some(home_dir().join(".claude").join("CLAUDE.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".claude").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        home_dir().join(".claude").join("settings.json").exists()
    }

    /// Claude 只有 ANTHROPIC_* 一套环境变量，只认 Anthropic Messages。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        use crate::providers::model::ProviderKind::Anthropic;
        &[Anthropic]
    }

    fn single_slot(&self) -> bool {
        true
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".claude").join("settings.json")]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_claude())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::claude::ClaudeInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".claude.json").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_claude())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        mcp_inject::plan_servers_map(&home_dir().join(".claude.json"), "claude", &owned)
    }

    // ---- 凭证体检 ----

    fn credential_entries(&self) -> Vec<RawEntry> {
        let mut out = Vec::new();
        let path = home_dir().join(".claude").join("settings.json");
        let Some(json) = read_json(&path) else {
            return out;
        };
        let Some(env) = json.get("env") else {
            return out;
        };

        let base = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .unwrap_or("官方 Anthropic");

        collect(
            &mut out,
            "claude",
            base,
            "api_key",
            "settings.json · ANTHROPIC_AUTH_TOKEN",
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
        );
        collect(
            &mut out,
            "claude",
            base,
            "api_key",
            "settings.json · ANTHROPIC_API_KEY",
            env.get("ANTHROPIC_API_KEY").and_then(|v| v.as_str()),
        );
        out
    }
}
