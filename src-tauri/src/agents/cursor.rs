//! Cursor。
//!
//! 两个能力：指令文件、MCP。
//!
//! ## provider 为什么不在 `Agent` trait 里
//!
//! Cursor 支持自带密钥（BYOK），但它的配置在 SQLite 库
//! （`state.vscdb`）里，密钥经 Electron safeStorage 加密，且**必须先关掉
//! Cursor 才能写**。这三条让它没法套用 [`super::Agent::provider_plan`]
//! 那套「算出一个文本文件的新内容、交给 safe_write」的模型。
//!
//! 所以走单独一组命令：`agentcenter providers cursor status / plan /
//! apply / reset`，实现在 [`crate::providers::cursor_db`]。这里保持
//! `supports_provider()` 为 false，`providers sync` 不会碰 Cursor。
//!
//! 具体怎么写、为什么要求退出、已实测哪些项，见
//! `plan/provider-config-keys.md` 的 Cursor 一节。
//!
//! Cursor 原生读取 `~/.agents/skills`，但自己的 `~/.cursor/skills`
//! 里也常有独立安装的 skill，那部分仍要收集。

use super::Agent;
use crate::mcp::inject::{self as mcp_inject, McpFilePlan};
use crate::mcp::model::McpServer;
use crate::mcp::scan::{self as mcp_scan, McpAgentScan};
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Cursor;

impl Agent for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn label(&self) -> &'static str {
        "Cursor"
    }

    fn order(&self) -> u32 {
        110
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".cursor")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".cursor").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".cursor").join("skills")
    }

    fn reads_shared_skills(&self) -> bool {
        true
    }

    fn supports_mcp(&self) -> bool {
        true
    }

    fn mcp_ready(&self) -> bool {
        home_dir().join(".cursor").join("mcp.json").exists()
    }

    fn mcp_scan(&self) -> Option<McpAgentScan> {
        Some(mcp_scan::scan_cursor())
    }

    fn mcp_plan(&self, servers: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        let owned: Vec<McpServer> = servers.iter().map(|s| (*s).clone()).collect();
        mcp_inject::map_agent::plan_map(&mcp_inject::map_agent::cursor(), &owned)
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        // 密钥其实存在 state.vscdb 的 `secret://cursorAuth/openAIKey`，
        // 用 Electron safeStorage（DPAPI 主密钥 + AES-256-GCM）加密。
        // 本机实测这条记录是空串 —— 用户从没启用过自带密钥。
        Some("密钥在 state.vscdb 里经 Electron safeStorage 加密，不解密读不出明文")
    }

    // ---- token 用量统计 ----

    fn supports_usage(&self) -> bool {
        true
    }

    fn usage_sources(&self) -> Vec<PathBuf> {
        vec![cursor_state_db()]
    }

    fn usage_note(&self) -> &'static str {
        "估算值，不是实际计费量。Cursor 按请求次数计费，本地不记录单次 token 用量；\
         这里用「上下文窗口快照的相邻差值」推算每日新增输入。\
         上下文压缩会让差值失真（压缩后快照回落），所以只计算递增段。\
         输出 token 本地完全没有记录，恒为 0。"
    }

    fn usage_collect(&self) -> Option<crate::usage::AgentUsage> {
        let db = cursor_state_db();
        if !db.exists() {
            return Some(crate::usage::AgentUsage::unavailable(
                db.display().to_string(),
                "没有 Cursor 的 state.vscdb，说明还没用过 Cursor",
            ));
        }

        match read_cursor_usage(&db) {
            Ok(records) => Some(crate::usage::AgentUsage {
                records,
                available: true,
                source: db.display().to_string(),
                note: self.usage_note().to_string(),
                error: String::new(),
            }),
            Err(e) => Some(crate::usage::AgentUsage::failed(
                db.display().to_string(),
                e.to_string(),
            )),
        }
    }
}

/// Cursor 的全局状态库路径。
///
/// Windows 在 `%APPDATA%\Cursor\User\globalStorage\state.vscdb`，
/// macOS / Linux 在 `~/.config/Cursor/...`。这里只处理 Windows，
/// 和仓库其他 agent 的实现保持一致（都按 Windows 家目录布局）。
fn cursor_state_db() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join("AppData").join("Roaming"))
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb")
}

/// 从 Cursor 的 state.vscdb 估算用量，只读新增的行。
///
/// ## 为什么是估算
///
/// Cursor 不按 token 计费，所以本地 `bubbleId:*` 里那个 `tokenCount`
/// 字段**实测 136139 条全是 `{inputTokens:0, outputTokens:0}`**。
/// 想要任何 token 数字只能靠旁证。
///
/// 唯一有真实数据的是 `contextWindowStatusAtCreation.tokensUsed`
/// （753 条有非零值）：它记录了「这条消息发出时，上下文窗口里有多少 token」。
///
/// ## 估算方法
///
/// 同一个会话（composer）里，这个值随对话推进递增：
///
/// ```text
/// 03:40:38  tokensUsed=94595
/// 03:41:23  tokensUsed=98691   +4096   ← 这条消息带来的新增
/// 03:43:00  tokensUsed=107156  +8465
/// ```
///
/// 相邻两条的差就是「这次请求新增的输入 token」。累加所有正值即得估算总量。
/// 实测 57 个会话里递增 629 次、递减 52 次 —— 递减来自上下文压缩，
/// 那些段跳过（`tokens_used > prev` 才计入）。
///
/// 这是**下界**：首条消息的绝对值没法知道是不是从 0 开始，
/// 压缩掉的上下文也不再计入。
///
/// ## 增量怎么做
///
/// 那个库 4.2 GB，`contextWindowStatusAtCreation` 没有可用索引，
/// 全表扫一次 22~30 秒。但 `cursorDiskKV` 的 rowid 是随插入单调递增的
/// （实测尾部 4000 行里 934/936 的 rowid 与 `createdAt` 同序），
/// 所以只要记住上次读到的最大 rowid，下次 `WHERE rowid > 水位` 就只碰新行，
/// 实测耗时 0.000 秒。
///
/// 缓存里存的是**快照**（每个 composer 的 `tokensUsed` 序列），不是算好的差值。
/// 这样新行乱序到达也不会算错：每次都把所有快照按时间重排、重新算一遍差值。
/// 快照总数不到一千条，重算可以忽略不计。
///
/// `ON CONFLICT REPLACE` 会让更新过的行换个新 rowid，于是被再读一次。
/// 这不会重复计数：同样的 `(时间, tokensUsed)` 再出现一次，差值就是 0。
/// 但**删除**的行不会被发现 —— 缓存会保留它。真要纠正就 `--refresh` 重扫。
fn read_cursor_usage(
    db: &std::path::Path,
) -> Result<Vec<crate::usage::RawUsage>, Box<dyn std::error::Error>> {
    use rusqlite::OpenFlags;

    let mut cache: CursorCache = crate::usage::cache::cache_load("cursor")
        .filter(|c: &CursorCache| c.version == CURSOR_CACHE_VERSION)
        .unwrap_or_default();

    let conn = rusqlite::Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    // 只用 key/value 两列。value 是 JSON 文本（可能很大），
    // 所以用 LIKE 先粗筛，避免把 13 万条完整消息都反序列化。
    // rowid > 水位 走 rowid 的隐式索引，只扫新插入的行
    let mut stmt = conn.prepare(
        "SELECT rowid, key, value FROM cursorDiskKV
         WHERE rowid > ?1
           AND key LIKE 'bubbleId:%'
           AND cast(value AS text) LIKE '%contextWindowStatusAtCreation%'",
    )?;

    let rows = stmt.query_map([cache.watermark], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let mut new_max = cache.watermark;

    for row in rows.flatten() {
        let (rowid, key, value) = row;
        new_max = new_max.max(rowid);

        // 解析失败就跳过这条。这个库里确实有坏 JSON，
        // 一条坏记录不该让整次统计失败
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&value) else {
            continue;
        };

        let Some(cs) = v.get("contextWindowStatusAtCreation") else {
            continue;
        };
        let tokens_used = cs.get("tokensUsed").and_then(|x| x.as_u64()).unwrap_or(0);
        if tokens_used == 0 {
            continue;
        }

        let model = v
            .get("modelInfo")
            .and_then(|m| m.get("modelName"))
            .and_then(|x| x.as_str())
            .unwrap_or("cursor")
            .to_string();

        let Some(at_ms) = v
            .get("createdAt")
            .and_then(|t| t.as_str())
            .and_then(crate::usage::iso_to_ms)
        else {
            continue;
        };

        // key 形如 `bubbleId:<composerId>:<bubbleId>`，取中间那段分组
        let composer = key.split(':').nth(1).unwrap_or("").to_string();
        if composer.is_empty() {
            continue;
        }

        cache
            .snapshots
            .entry(composer)
            .or_default()
            .push(Snapshot {
                at_ms,
                tokens_used,
                model,
            });
    }

    cache.watermark = new_max;

    let records = cache.into_records();

    // 缓存立刻写回（Cursor 一次扫描可能就要几秒，不能指望进程一定活到结尾）。
    // 写失败不影响本次结果 —— 缓存只是加速手段，不是正确性依赖
    crate::usage::cache::cache_store("cursor", &cache);

    Ok(records)
}

/// Cursor 增量扫描的缓存格式版本。字段有不兼容变化时加一。
const CURSOR_CACHE_VERSION: u32 = 1;

/// Cursor 增量扫描的缓存内容。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CursorCache {
    /// 格式版本
    version: u32,
    /// 已扫描到的最大 rowid
    watermark: i64,
    /// composerId → 上下文窗口快照序列
    snapshots: std::collections::HashMap<String, Vec<Snapshot>>,
}

impl Default for CursorCache {
    fn default() -> Self {
        Self {
            version: CURSOR_CACHE_VERSION,
            watermark: 0,
            snapshots: std::collections::HashMap::new(),
        }
    }
}

/// 一条上下文窗口快照。
///
/// 只存原始观测值，不存算好的差值 —— 差值由 [`CursorCache::into_records`]
/// 每次重算，这样新快照乱序到达也不会算错。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Snapshot {
    at_ms: i64,
    tokens_used: u64,
    model: String,
}

impl CursorCache {
    /// 把所有快照按时间重排、算相邻差值，转成用量记录。
    ///
    /// 每次全量重算而不是增量累加：快照总数不到一千条，
    /// 重算的代价远小于维护「增量算差值」时处理乱序、重复、压缩回退的复杂度。
    fn into_records(&self) -> Vec<crate::usage::RawUsage> {
        let mut out = Vec::new();
        for items in self.snapshots.values() {
            let mut sorted: Vec<&Snapshot> = items.iter().collect();
            sorted.sort_by_key(|s| (s.at_ms, s.tokens_used));

            let mut prev: Option<u64> = None;
            for s in sorted {
                if let Some(p) = prev {
                    // 只算递增段：回退说明上下文被压缩了，那部分不该算成新增
                    if s.tokens_used > p {
                        out.push(crate::usage::RawUsage {
                            at_ms: s.at_ms,
                            model: s.model.clone(),
                            input: s.tokens_used - p,
                            // Cursor 本地完全没有输出 token 的记录
                            output: 0,
                            cache_read: 0,
                            cache_write: 0,
                            // delta 是「累计上下文的新增值」，已经包含缓存部分
                            input_includes_cache: true,
                        });
                    }
                }
                prev = Some(s.tokens_used);
            }
        }
        out
    }
}

