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

    /// Claude Code 本身只说 Anthropic Messages。
    /// OpenAI Chat / Responses 由本地转换听口翻过去，所以这三种都能分给它。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        use crate::providers::model::ProviderKind::{Anthropic, OpenaiChat, OpenaiResponses};
        &[Anthropic, OpenaiChat, OpenaiResponses]
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
        mcp_inject::map_agent::plan_map(&mcp_inject::map_agent::claude(), &owned)
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

    // ---- token 用量统计 ----

    fn supports_usage(&self) -> bool {
        true
    }

    fn usage_sources(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".claude").join("projects")]
    }

    fn usage_note(&self) -> &'static str {
        "会话文件在 ~/.claude/projects/<项目>/<会话>.jsonl。\
         只统计 assistant 消息；报错消息（model 为 <synthetic>）不计入。\
         原始 input 不含缓存，已折算为含缓存。"
    }

    fn usage_collect(&self) -> Option<crate::usage::AgentUsage> {
        let root = home_dir().join(".claude").join("projects");
        if !root.exists() {
            return Some(crate::usage::AgentUsage::unavailable(
                root.display().to_string(),
                "没有会话目录，说明还没用过 Claude Code",
            ));
        }

        let files = crate::usage::walk_files(&root, ".jsonl");
        let mut records = Vec::new();

        for path in &files {
            // 增量：只读上次之后新追加的字节
            let parsed = crate::usage::cache::cached_jsonl("claude", path, |v, out| {
                parse_claude_line(v, out)
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

/// 解析 Claude 会话文件里的一行，有 assistant 用量就推一条记录。
///
/// 结构：每行一个事件，用量在 `type == "assistant"` 的
/// `message.usage` 里。`message.model` 是模型名。
///
/// 两处必须过滤：
/// - `model == "<synthetic>"`：本地生成的报错消息，没有真实调用，用量全 0
/// - 用量全 0 且没有 model：占位记录
///
/// 注意 Claude 的 input **不含**缓存，`cache_read` / `cache_creation`
/// 是独立字段，所以 `input_includes_cache = false`。
fn parse_claude_line(v: &serde_json::Value, out: &mut Vec<crate::usage::RawUsage>) {
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return;
    }
    let Some(msg) = v.get("message") else { return };
    let Some(usage) = msg.get("usage") else { return };

    let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
    // <synthetic> 是 Claude 自己造的报错/提示消息，不消耗模型额度
    if model.is_empty() || model == "<synthetic>" {
        return;
    }

    let num = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);

    let timestamp = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(crate::usage::iso_to_ms);
    let Some(at_ms) = timestamp else { return };

    out.push(crate::usage::RawUsage {
        at_ms,
        model: model.to_string(),
        input: num("input_tokens"),
        output: num("output_tokens"),
        cache_read: num("cache_read_input_tokens"),
        cache_write: num("cache_creation_input_tokens"),
        // Claude 的 input 不含缓存，需要加回去
        input_includes_cache: false,
    });
}
