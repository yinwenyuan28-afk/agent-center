//! Kimi Code 的 MCP 注入器：`~/.kimi-code/mcp.json` 顶层 `mcpServers`。
//!
//! SSE 的写法是 `transport: "sse"` 而不是 `type`；stdio 和 HTTP 都不写 `type`。
//!
//! 私有字段 `deferred`、`toolTimeoutMs` 原样保留。
//!
//! 停用时**删条目**（不像 Pi 那样保留），这一点和 CC Switch 一致。
//!
//! 本机未安装，实现按 CC Switch 的 `mcp/kimi.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Kimi Code 的数据目录。支持 `KIMI_CODE_HOME` 覆盖。
pub fn data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("KIMI_CODE_HOME") {
        let v = v.trim();
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    crate::providers::store::home_dir().join(".kimi-code")
}

/// Kimi Code 的 mcp.json 路径。
pub fn config_path() -> PathBuf {
    data_dir().join("mcp.json")
}

/// 连接字段。写入时先删这些。
const TRANSPORT_FIELDS: &[&str] = &["type", "transport", "command", "args", "cwd", "env", "url", "headers"];

/// Kimi Code 注入器。
pub struct KimiMcpInjector;

/// 转成 Kimi 条目。SSE 用 `transport: "sse"`，其余不写 type。
fn to_kimi_entry(s: &McpServer) -> Map<String, Value> {
    let mut obj = super::shared::transport_entry(s);
    if s.transport == Transport::Sse {
        obj.insert("transport".into(), Value::String("sse".into()));
    }
    obj
}

impl McpInjector for KimiMcpInjector {
    fn agent(&self) -> &'static str {
        "kimi"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        // Kimi 没装就不写
        if !path.parent().is_some_and(|p| p.exists()) {
            return Ok(Vec::new());
        }

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{}".to_string()
        };
        let pre_hash = hash_file(&path)?;
        let mut root = super::shared::read_json(&path)?;
        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        for id in super::shared::prune_unassigned(map, "kimi", servers) {
            summary.push(format!("移除 {id}（未分配给 kimi）"));
            changed = true;
        }

        for s in super::shared::wanted(servers, "kimi") {
            let conn = to_kimi_entry(s);
            let is_new = !map.contains_key(&s.id);
            let mut entry = map
                .get(&s.id)
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let before = entry.clone();

            for f in TRANSPORT_FIELDS {
                entry.remove(*f);
            }
            for (k, v) in &conn {
                entry.insert(k.clone(), v.clone());
            }
            entry.insert("enabled".into(), Value::Bool(true));

            if entry != before {
                map.insert(s.id.clone(), Value::Object(entry));
                summary.push(if is_new {
                    format!("新增 {}", s.display_name())
                } else {
                    format!("更新 {}", s.display_name())
                });
                changed = true;
            } else {
                summary.push(format!("{} 已一致", s.display_name()));
            }
        }

        let content = if changed {
            let mut text = serde_json::to_string_pretty(&root)?;
            text.push('\n');
            text
        } else {
            orig
        };

        Ok(vec![McpFilePlan {
            path,
            agent: "kimi".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}
