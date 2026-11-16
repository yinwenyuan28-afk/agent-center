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
use crate::mcp::inject::{self as mcp_inject, McpFilePlan, McpInjector};
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
        mcp_inject::pi::PiMcpInjector.plan(&owned)
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

    // ---- token 用量统计 ----

    fn supports_usage(&self) -> bool {
        true
    }

    fn usage_sources(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".pi").join("agent").join("sessions")]
    }

    fn usage_note(&self) -> &'static str {
        "会话文件按项目分目录，子代理会话在子目录里。\
         原始 input 不含缓存，已把 cacheRead / cacheWrite 折算进输入。"
    }

    fn usage_collect(&self) -> Option<crate::usage::AgentUsage> {
        let root = home_dir().join(".pi").join("agent").join("sessions");
        if !root.exists() {
            return Some(crate::usage::AgentUsage::unavailable(
                root.display().to_string(),
                "没有 sessions 目录，说明还没用过 pi",
            ));
        }

        let files = crate::usage::walk_files(&root, ".jsonl");
        let mut records = Vec::new();

        for path in &files {
            // 增量：只读上次之后新追加的字节，旧的直接复用缓存里的结果
            let parsed = crate::usage::cache::cached_jsonl("pi", path, |v, out| {
                parse_pi_line(v, out)
            });
            records.extend(parsed);
        }

        Some(crate::usage::AgentUsage {
            records,
            available: true,
            source: root.display().to_string(),
            note: self.usage_note().to_string(),
            error: String::new(),
        })
    }
}

/// 解析 pi 会话文件里的一行，有助手用量就推一条记录。
///
/// 结构：每行一个事件，`{"type": "message", "message": {...}}`。
/// 只有 `message.role == "assistant"` 带用量，在 `message.usage`：
///
/// ```json
/// {"input":16887,"output":229,"cacheRead":1024,"cacheWrite":0,
///  "reasoning":117,"totalTokens":18140,"cost":{...}}
/// ```
///
/// ## 口径
///
/// pi 的 `input` **不含**缓存 —— 实测 36163 条样本全部满足
/// `totalTokens == input + output + cacheRead + cacheWrite`。
/// 所以 `input_includes_cache = false`。
///
/// `reasoning` 不单独计：pi 的 `totalTokens` 里也没有把它重复加进去，
/// 它是 output 的细分项。
///
/// ## 子目录里的会话
///
/// 除 `<项目>/<会话>.jsonl` 外，还有 `<项目>/<会话>/<hash>/run-0/session.jsonl`
/// 这类子代理会话文件。它们的事件 id 与父文件不重叠（实测 30 个文件零重叠），
/// 所以按文件全扫不会重复计数。
fn parse_pi_line(v: &serde_json::Value, out: &mut Vec<crate::usage::RawUsage>) {
    if v.get("type").and_then(|t| t.as_str()) != Some("message") {
        return;
    }
    let Some(msg) = v.get("message") else { return };
    if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return;
    }
    let Some(usage) = msg.get("usage") else { return };

    let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
    if model.is_empty() {
        return;
    }

    // 时间优先用外层事件时间戳，没有再退到 message 里的毫秒时间戳
    let at_ms = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(crate::usage::iso_to_ms)
        .or_else(|| msg.get("timestamp").and_then(|t| t.as_i64()))
        .or_else(|| v.get("timestamp").and_then(|t| t.as_i64()));
    let Some(at_ms) = at_ms else { return };

    let num = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);

    out.push(crate::usage::RawUsage {
        at_ms,
        model: model.to_string(),
        input: num("input"),
        output: num("output"),
        cache_read: num("cacheRead"),
        cache_write: num("cacheWrite"),
        // pi 的 input 不含缓存，需要加回去
        input_includes_cache: false,
    });
}
