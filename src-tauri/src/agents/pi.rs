//! pi。
//!
//! 四个能力全有：指令文件、provider、MCP、凭证体检。
//!
//! provider 定义分散在 `auth.json`（key）和 `models.json`（定义）两个文件，
//! 两个必须一起成功 —— 只写一个会让 provider 有 key 无定义。
//!
//! 具体注入逻辑在 [`crate::providers::pi`]。

use super::model::{collect, read_json, RawEntry};
use super::Agent;
use crate::mcp::inject::{self as mcp_inject, McpFilePlan};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Pi;

impl Agent for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn label(&self) -> &'static str {
        "pi"
    }

    fn order(&self) -> u32 {
        40
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".pi")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        // pi 的 agentDir 是 ~/.pi/agent，指令文件在那一层
        Some(home_dir().join(".pi").join("agent").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".pi").join("agent").join("skills")
    }

    fn reads_shared_skills(&self) -> bool {
        true
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        home_dir().join(".pi").join("agent").join("auth.json").exists()
            || home_dir()
                .join(".pi")
                .join("agent")
                .join("models.json")
                .exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![
            home_dir().join(".pi").join("agent").join("auth.json"),
            home_dir().join(".pi").join("agent").join("models.json"),
        ]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_pi())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::pi::PiInjector.plan(providers)
    }

    // ---- MCP ----

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".pi").join("agent").join("mcp.json").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_pi())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        mcp_inject::plan_servers_map(
            &home_dir().join(".pi").join("agent").join("mcp.json"),
            "pi",
            &owned,
        )
    }

    // ---- 凭证体检 ----

    fn credential_entries(&self) -> Vec<RawEntry> {
        let mut out = Vec::new();
        let path = home_dir().join(".pi").join("agent").join("auth.json");
        let Some(json) = read_json(&path) else {
            return out;
        };
        let Some(obj) = json.as_object() else {
            return out;
        };

        for (provider, v) in obj {
            let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("api_key");
            collect(
                &mut out,
                "pi",
                provider,
                if ty == "oauth" { "oauth" } else { "api_key" },
                &format!("auth.json · {provider}"),
                v.get("key").and_then(|x| x.as_str()),
            );
        }
        out
    }
}
