//! ZCode 的 MCP 注入器：`~/.zcode/cli/config.json` 的 `mcp.servers`。
//!
//! 键路径比别家多两层：`mcp.servers.<name>`，不是顶层 `mcpServers`。
//!
//! ## schema 严格，四处写法必须对
//!
//! | 别家写法 | ZCode 要的 |
//! | --- | --- |
//! | `environment` | `env` |
//! | `http_headers` | `headers` |
//! | `enable` | `enabled` |
//! | `type: "remote"` | `type: "http"` |
//!
//! CLI 直接读文件时会自动迁移这些旧写法，但桌面版走另一条解析路径、**不迁移**。
//! 所以写的时候一律用新写法。
//!
//! `command` 必须是字符串。写成数组（OpenCode 那种）会让 Settings → MCP 崩，
//! 报 `command.trim is not a function`。
//!
//! ## 回退文件
//!
//! `~/.agents/mcp.json` 是同作用域的回退：只有主文件里完全没有 MCP server
//! 时才读它，而且是顶层 `mcpServers`。我们只写主文件，不去猜这个状态。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// ZCode 的 CLI 配置路径。
pub fn config_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".zcode")
        .join("cli")
        .join("config.json")
}

/// ZCode 注入器。
pub struct ZCodeMcpInjector;

impl McpInjector for ZCodeMcpInjector {
    fn agent(&self) -> &'static str {
        "zcode"
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

        // 先确保 mcp.servers 两层都在，再取 map
        {
            let _ = super::shared::ensure_object(&mut root, &["mcp", "servers"], &path)?;
        }
        let map: &mut Map<String, Value> = root["mcp"]["servers"]
            .as_object_mut()
            .expect("ensure_object 刚建好");

        let mut summary = Vec::new();
        let mut changed = false;

        for id in super::shared::prune_unassigned(map, "zcode", servers) {
            summary.push(format!("移除 {id}（未分配给 zcode）"));
            changed = true;
        }

        for s in super::shared::wanted(servers, "zcode") {
            let mut conn = super::shared::transport_entry(s);
            // 显式写 type，避免推断差异。ZCode 的词表里 http 就叫 http
            conn.insert(
                "type".into(),
                Value::String(s.transport.as_str().to_string()),
            );
            conn.insert("enabled".into(), Value::Bool(s.enabled));

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
            agent: "zcode".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}
