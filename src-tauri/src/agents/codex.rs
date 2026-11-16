//! Codex CLI。
//!
//! 四个能力全有：指令文件（`AGENTS.md`）、provider、MCP、凭证体检。
//!
//! ## provider：单槽位，key 内联在表里
//!
//! Codex 只有一张第三方表 `[model_providers.custom]`，顶层
//! `model_provider` 指向它。密钥写在表的 `experimental_bearer_token`，
//! 和 cc-switch 的盘上形状一致 —— Codex 0.149 起自定义 provider 不再
//! 从 `auth.json` 读密钥，也不再需要单独的环境变量脚本。
//!
//! 所以这是**单槽位** agent：同一时刻只有一个 provider 生效。
//!
//! 具体逻辑在 [`crate::providers::codex`]。

use super::model::{collect, read_json, RawEntry};
use super::Agent;
use crate::mcp::inject::{McpFilePlan, McpInjector};
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

    /// Codex 一次只能指向一家第三方供应商：`[model_providers.custom]`
    /// 只有一张表，顶层 `model_provider` 只认一个值。
    fn single_slot(&self) -> bool {
        true
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".codex").join("config.toml")]
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
        crate::mcp::inject::codex::CodexMcpInjector::new().plan(&owned)
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

    // ---- token 用量统计 ----

    fn supports_usage(&self) -> bool {
        true
    }

    fn usage_sources(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".codex").join("sessions")]
    }

    fn usage_note(&self) -> &'static str {
        "只读未压缩的 rollout-*.jsonl。更早的历史是 .jsonl.zst 压缩格式，未纳入统计。\
         每条 token_usage_record 是单次请求的量，累加不会重复。原始 input 已含缓存。"
    }

    fn usage_collect(&self) -> Option<crate::usage::AgentUsage> {
        let root = home_dir().join(".codex").join("sessions");
        if !root.exists() {
            return Some(crate::usage::AgentUsage::unavailable(
                root.display().to_string(),
                "没有 sessions 目录，说明还没用过 Codex",
            ));
        }

        // 只认明文 .jsonl。压缩的 .jsonl.zst 后缀不同，
        // 这里自然被排除 —— 后续要支持时改这一行的匹配条件即可
        let files = crate::usage::walk_files(&root, ".jsonl");
        let mut records = Vec::new();

        for path in &files {
            // 增量：只读新追加的字节。解析状态（turn_id → 模型）跟着文件存在缓存里，
            // 否则从文件尾巴读到的 usage 记录会全部找不到模型
            let parsed =
                crate::usage::cache::cached_jsonl_stateful::<CodexState, _>("codex", path, |v, st, out| {
                    parse_codex_line(v, st, out)
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

/// Codex rollout 文件的跨行解析状态。
///
/// ## 为什么这个类型要存到磁盘
///
/// `token_usage_record` 本身不带模型名，模型在同一个文件前面的
/// `turn_context` 行里。增量读取只从文件尾部往后读时，那些 turn_context
/// 早就读过了。不把「turn_id → 模型」传下来，新记录会全部因为
/// 找不到模型被丢掉 —— 那正是最沉默的一种算错。
///
/// 字段名是磁盘格式的一部分，改名等于让所有旧缓存失效（这不会算错，
/// 只会让下次慢一次），但仍要保持稳定。
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct CodexState {
    /// turn_id → 模型名
    turn_model: std::collections::HashMap<String, String>,
    /// 兜底：最近一次见到的模型。有些记录找不到 turn_context
    last_model: String,
}

/// 解析 Codex rollout 文件里的一行，有用量就推一条记录。
///
/// 新版（0.16x 起）每行是 `{"type": "token_usage_record", "payload": {...}}`，
/// 用量在 `payload.usage`。
///
/// ## 累加语义已验证
///
/// `payload.usage` 是**单次请求**的量，不是累计值。实测：
/// 一个 97 条记录的会话里 `sum(每条 usage)` 与最后一条的
/// `thread_token_usage` 完全相等（input 8311231 / output 31731）。
/// 所以直接逐条累加是正确的，不能再叠加 `turn_token_usage`。
///
/// ## 模型名从哪来
///
/// 要靠 `turn_context` 的 `payload.model`。一条 turn_context 之后可能跟着
/// 多条 usage 记录，所以这里维护「当前 turn → 模型」的映射，且这份映射
/// 存在 [`CodexState`] 里跨行、跨进程存活（见上面的说明）。
///
/// Codex 的 `input_tokens` **已含**缓存（`cached_input_tokens` 是它的子集），
/// 所以 `input_includes_cache = true`。
fn parse_codex_line(
    v: &serde_json::Value,
    st: &mut CodexState,
    out: &mut Vec<crate::usage::RawUsage>,
) {
    let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    if kind == "turn_context" {
        let Some(p) = v.get("payload") else { return };
        let turn = p.get("turn_id").and_then(|t| t.as_str()).unwrap_or("");
        let model = p.get("model").and_then(|m| m.as_str()).unwrap_or("");
        if !turn.is_empty() && !model.is_empty() {
            st.turn_model
                .insert(turn.to_string(), model.to_string());
            st.last_model = model.to_string();
        }
        return;
    }

    if kind != "token_usage_record" {
        return;
    }
    let Some(p) = v.get("payload") else { return };
    let Some(usage) = p.get("usage") else { return };

    let turn = p.get("turn_id").and_then(|t| t.as_str()).unwrap_or("");
    let model = st
        .turn_model
        .get(turn)
        .cloned()
        .unwrap_or_else(|| st.last_model.clone());
    // 连模型都拿不到的记录没法归到「哪个模型」，只会计入无法辨认的桶，
    // 与其让界面上出现空模型行，不如跳过
    if model.is_empty() {
        return;
    }

    let timestamp = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(crate::usage::iso_to_ms);
    let Some(at_ms) = timestamp else { return };

    let num = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);

    out.push(crate::usage::RawUsage {
        at_ms,
        model,
        input: num("input_tokens"),
        output: num("output_tokens"),
        cache_read: num("cached_input_tokens"),
        cache_write: num("cache_write_input_tokens"),
        // Codex 的 input_tokens 已包含 cached_input_tokens
        input_includes_cache: true,
    });
}
