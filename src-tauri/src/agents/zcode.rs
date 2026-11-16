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
use crate::mcp::inject::{McpFilePlan, McpInjector};
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
        crate::mcp::inject::zcode::ZCodeMcpInjector.plan(&owned)
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

    // ---- token 用量统计 ----

    fn supports_usage(&self) -> bool {
        true
    }

    fn usage_sources(&self) -> Vec<PathBuf> {
        vec![home_dir().join(".zcode").join("cli").join("db").join("db.sqlite")]
    }

    fn usage_note(&self) -> &'static str {
        "数据在 ~/.zcode/cli/db/db.sqlite 的 model_usage 表，按模型调用逐条记录。\
         只统计 status=completed 的记录（取消和报错的用量是 0）。\
         原始 input 已含缓存读取。"
    }

    fn usage_collect(&self) -> Option<crate::usage::AgentUsage> {
        let db = home_dir().join(".zcode").join("cli").join("db").join("db.sqlite");
        if !db.exists() {
            return Some(crate::usage::AgentUsage::unavailable(
                db.display().to_string(),
                "没有 db.sqlite，说明还没用过 ZCode CLI",
            ));
        }

        match read_zcode_usage(&db) {
            Ok(records) => Some(crate::usage::AgentUsage {
                records,
                available: true,
                source: db.display().to_string(),
                note: self.usage_note().to_string(),
                error: String::new(),
            }),
            // 库被占用、schema 变了都会走到这里。返回可用但带 error，
            // 让界面显示「数据源在，但读失败」，而不是说成没装
            Err(e) => Some(crate::usage::AgentUsage::failed(
                db.display().to_string(),
                e.to_string(),
            )),
        }
    }
}

/// 从 ZCode 的 sqlite 读用量。
///
/// ## 为什么只读打开
///
/// 这个库是 ZCode 正在用的活库（1 GB，WAL 模式）。写锁会干扰它，
/// 所以用 `SQLITE_OPEN_READ_ONLY`。只读模式下 SQLite 仍能读 WAL 里
/// 尚未 checkpoint 的数据，不会漏掉最新记录。
///
/// ## 为什么不用 `mode=ro` URI
///
/// rusqlite 的 `open_with_flags` 已经能表达只读，URI 方式还要
/// 手拼转义路径，在 Windows 上遇到反斜杠容易出错。
///
/// ## 口径
///
/// 表的 `input_tokens` **已含** `cache_read_input_tokens`（实测
/// `max(cache_read - input) = 0`，且 `computed_total_tokens` 恒等于
/// `input_tokens + output_tokens`），所以 `input_includes_cache = true`。
fn read_zcode_usage(
    db: &std::path::Path,
) -> Result<Vec<crate::usage::RawUsage>, Box<dyn std::error::Error>> {
    use rusqlite::OpenFlags;

    let conn = rusqlite::Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    // 只取本次统计用得到的四列。不带 WHERE 时间条件：窗口过滤在
    // 内存里做，这样进程内缓存对切换时间窗有效，不用反复查库。
    let mut stmt = conn.prepare(
        "SELECT started_at, model_id, input_tokens, output_tokens,
                cache_read_input_tokens, cache_creation_input_tokens
         FROM model_usage
         WHERE status = 'completed' AND model_id <> ''",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(crate::usage::RawUsage {
            at_ms: r.get::<_, i64>(0)?,
            model: r.get::<_, String>(1)?,
            input: r.get::<_, i64>(2)?.max(0) as u64,
            output: r.get::<_, i64>(3)?.max(0) as u64,
            cache_read: r.get::<_, i64>(4)?.max(0) as u64,
            cache_write: r.get::<_, i64>(5)?.max(0) as u64,
            // 已含缓存，不能再加一遍
            input_includes_cache: true,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
