//! MiniMax Code 的 MCP 注入器：`~/.minimax/mcp.json` 顶层 `mcpServers`。
//!
//! 结构和 Claude 同构，多一个 `enabled: true`。停用时删条目。
//!
//! 本机未安装，实现按 CC Switch 的 `mcp/mcode.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::providers::safe_write::hash_file;
use serde_json::Value;
use std::path::PathBuf;

/// MiniMax Code 的数据目录。支持 `MINIMAX_DATA_DIR` / `MAVIS_DATA_DIR` 覆盖。
pub fn data_dir() -> PathBuf {
    for var in ["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    crate::providers::store::home_dir().join(".minimax")
}

/// MiniMax Code 的 mcp.json 路径。
pub fn config_path() -> PathBuf {
    data_dir().join("mcp.json")
}

/// MiniMax Code 注入器。
pub struct MinimaxMcpInjector;

impl McpInjector for MinimaxMcpInjector {
    fn agent(&self) -> &'static str {
        "minimax"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
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

        for id in super::shared::prune_unassigned(map, "minimax", servers) {
            summary.push(format!("移除 {id}（未分配给 minimax）"));
            changed = true;
        }

        for s in super::shared::wanted(servers, "minimax") {
            let mut conn = super::shared::transport_entry(s);
            conn.insert("enabled".into(), Value::Bool(true));
            let is_new = !map.contains_key(&s.id);

            let mut entry = map
                .get(&s.id)
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let before = entry.clone();
            for f in super::shared::TRANSPORT_FIELDS {
                entry.remove(*f);
            }
            entry.remove("enabled");
            for (k, v) in &conn {
                entry.insert(k.clone(), v.clone());
            }

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
            agent: "minimax".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}
