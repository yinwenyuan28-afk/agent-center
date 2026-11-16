//! Gemini CLI。
//!
//! ## provider：单槽位
//!
//! Gemini CLI 只有一份 live 配置：`~/.gemini/.env`（key / 端点 / 模型）
//! 和 `~/.gemini/settings.json`（认证方式 / 模型名）。所以同一时刻只有
//! 一个 provider 生效。
//!
//! **本机未安装**，实现是按 cc-switch 的 `gemini_config.rs` 移植的，
//! 装好之后要按真实文件核对。

use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Gemini;

impl Gemini {
    fn dir() -> PathBuf {
        home_dir().join(".gemini")
    }

    fn env_path() -> PathBuf {
        Self::dir().join(".env")
    }
}

impl Agent for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn label(&self) -> &'static str {
        "Gemini CLI"
    }

    fn order(&self) -> u32 {
        180
    }

    fn config_dir(&self) -> PathBuf {
        Self::dir()
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(Self::dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        Self::dir().join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::env_path().exists() || Self::dir().join("settings.json").exists()
    }

    /// 只有一组 live 环境变量，和一个 provider 生效。
    fn single_slot(&self) -> bool {
        true
    }

    /// Gemini CLI 走自己的协议适配，本工具只写 OpenAI 兼容端点。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        use crate::providers::model::ProviderKind::{Anthropic, OpenaiChat, OpenaiResponses};
        &[Anthropic, OpenaiChat, OpenaiResponses]
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::env_path(), Self::dir().join("settings.json")]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_gemini())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::gemini::GeminiInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        crate::mcp::inject::gemini::config_path().exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_gemini())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::gemini::GeminiMcpInjector.plan(&owned)
    }

    fn credential_entries(&self) -> Vec<crate::agents::RawEntry> {
        let mut out = Vec::new();
        let path = Self::env_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return out;
        };
        let mut key = None;
        let mut base = "官方 Google".to_string();
        for line in text.lines() {
            let t = line.trim();
            if let Some(v) = t.strip_prefix("GEMINI_API_KEY=") {
                key = Some(v.trim().to_string());
            } else if let Some(v) = t.strip_prefix("GOOGLE_GEMINI_BASE_URL=") {
                if !v.trim().is_empty() {
                    base = v.trim().to_string();
                }
            }
        }
        if let Some(k) = key {
            crate::agents::model::collect(
                &mut out,
                "gemini",
                &base,
                "api_key",
                ".env · GEMINI_API_KEY",
                Some(&k),
            );
        }
        out
    }
}
