//! Codex CLI。
//!
//! 四个能力全有：指令文件（`AGENTS.md`）、provider、MCP、凭证体检。
//!
//! ## provider：key 不进配置文件
//!
//! Codex 的 `[model_providers.<id>]` 只有 `env_key` 字段存**环境变量名**，
//! key 本身必须从进程环境读。所以注入器做两件事：改 `config.toml`
//! 写 provider 表，再生成 `~/.agents/codex-env.ps1` 把 key 设进环境变量。
//! 用户使用 Codex 前要 `. ~/.agents/codex-env.ps1`。
//!
//! 具体逻辑在 [`crate::providers::codex`]。

use super::model::{collect, read_json, RawEntry};
use super::Agent;
use crate::mcp::inject::McpFilePlan;
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Codex;

impl Agent for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn label(&self) -> &'static str {
        "Codex CLI"
    }

    fn order(&self) -> u32 {
        20
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".codex")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".codex").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".codex").join("skills")
    }

    fn reads_shared_skills(&self) -> bool {
        true
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        home_dir().join(".codex").join("config.toml").exists()
    }

    /// Codex 的 wire_api 只接受 responses。
    ///
    /// 别的协议选进来会被强制改写成 responses 并打警告 ——
    /// 与其让用户选了发现没用，不如只列这一个。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        use crate::providers::model::ProviderKind::OpenaiResponses;
        &[OpenaiResponses]
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![
            home_dir().join(".codex").join("config.toml"),
            home_dir().join(".agents").join("codex-env.ps1"),
        ]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_codex())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::codex::CodexInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".codex").join("config.toml").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_codex())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::plan_codex(&owned)
    }

    // ---- 凭证体检 ----

    fn credential_entries(&self) -> Vec<RawEntry> {
        let mut out = Vec::new();
        let path = home_dir().join(".codex").join("auth.json");
        let Some(json) = read_json(&path) else {
            return out;
        };

        collect(
            &mut out,
            "codex",
            "openai",
            "api_key",
            "auth.json · OPENAI_API_KEY",
            json.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
        );

        // OAuth 三件套也纳入体检 —— 它们同样会过期、同样散落
        if let Some(tokens) = json.get("tokens") {
            collect(
                &mut out,
                "codex",
                "openai-oauth",
                "oauth",
                "auth.json · access_token",
                tokens.get("access_token").and_then(|v| v.as_str()),
            );
        }
        out
    }
}
