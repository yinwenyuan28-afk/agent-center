//! ZCode。
//!
//! 三个能力：指令文件（`AGENTS.md`）、provider、MCP。凭证体检也做。
//!
//! ## MCP 配置位置
//!
//! `~/.zcode/cli/config.json` 的 **`mcp.servers`**（嵌套两层）。
//!
//! 之前这里写着「未探明，不在可分配列表里」，那是错的 —— 当时只查了
//! `v2/config.json`（只有 `provider`）和 `cli/config.json`（本机只有 `plugins`），
//! 没找到就下了结论。实际 ZCode 官方文档明确写的是 `mcp.servers`。
//!
//! 两处易错点：
//! - `.agents/mcp.json` 是**同 scope 的回退**，只在 `.zcode` 里没有 MCP 时才读
//! - 文件 schema 严格：多一个未知键整个 server 被丢弃
//!
//! 具体注入逻辑在 [`crate::providers::zcode`] 和
//! [`crate::mcp::inject::ZCodeMcpInjector`]。

use super::model::{collect, read_json, RawEntry};
use super::Agent;
use crate::mcp::inject::McpFilePlan;
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct ZCode;

impl ZCode {
    fn mcp_config_path() -> PathBuf {
        home_dir().join(".zcode").join("cli").join("config.json")
    }
}

impl Agent for ZCode {
    fn id(&self) -> &'static str {
        "zcode"
    }

    fn label(&self) -> &'static str {
        "ZCode"
    }

    fn order(&self) -> u32 {
        30
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".zcode")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        // ZCode 的全局指令文件固定为 ~/.zcode/AGENTS.md
        Some(home_dir().join(".zcode").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".zcode").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        home_dir()
            .join(".zcode")
            .join("v2")
            .join("config.json")
            .exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        let v2 = home_dir().join(".zcode").join("v2");
        vec![
            v2.join("config.json"),
            v2.join("provider_config.json"),
        ]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_zcode())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::zcode::ZCodeInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        Self::mcp_config_path().exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_zcode())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        crate::mcp::inject::plan_zcode(&owned)
    }

    fn credential_entries(&self) -> Vec<RawEntry> {
        let mut out = Vec::new();
        let dir = home_dir().join(".zcode").join("v2");

        // v2/config.json 的 provider.<id>.options.apiKey
        if let Some(cfg) = read_json(&dir.join("config.json")) {
            if let Some(providers) = cfg.get("provider").and_then(|v| v.as_object()) {
                for (id, p) in providers {
                    let options = p.get("options");
                    let base = options
                        .and_then(|o| o.get("baseURL"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(id.as_str());
                    let key = options
                        .and_then(|o| o.get("apiKey"))
                        .and_then(|v| v.as_str());
                    collect(
                        &mut out,
                        "zcode",
                        base,
                        "api_key",
                        &format!("config.json · {id}"),
                        key,
                    );
                }
            }
        }

        // v2/credentials.json 的键值对（键名带 access_token / api-key 等）
        if let Some(cred) = read_json(&dir.join("credentials.json")) {
            if let Some(obj) = cred.as_object() {
                for (k, v) in obj {
                    let low = k.to_lowercase();
                    // 键名带这些词的才当凭证，避免把普通配置算进来
                    if !(low.contains("access_token")
                        || low.contains("api-key")
                        || low.contains("credential")
                        || low.contains("jwt"))
                    {
                        continue;
                    }
                    // 键名形如 "oauth:zai:access_token"，取前两段作为 provider
                    let provider = k.split(':').take(2).collect::<Vec<_>>().join(":");
                    collect(
                        &mut out,
                        "zcode",
                        &provider,
                        "oauth",
                        &format!("credentials.json · {k}"),
                        v.as_str(),
                    );
                }
            }
        }
        out
    }
}
