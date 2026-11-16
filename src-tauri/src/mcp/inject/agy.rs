//! Antigravity（agy）的 MCP 注入器：`~/.gemini/config/mcp_config.json`。
//!
//! 注意目录：`~/.gemini/config/`，和 Gemini CLI 的 `~/.gemini/settings.json`
//! 不是同一个文件。
//!
//! ## 特殊点
//!
//! - 顶层 `mcpServers`，用 `disabled`（反义），不写 `type`
//! - 老字段 `serverUrl` 读进来要归一成 `url`
//! - **不支持 SSE**
//! - 首次启动会建一个 0 字节的 `mcp_config.json`，要当成「没有配置」
//! - 停用只标 `disabled: true`，保留条目
//!
//! 本机未装，实现按 CC Switch 的 `mcp/agy.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Antigravity 的定制目录。CLI、桌面版和 IDE 共用。
pub fn config_dir() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".gemini")
        .join("config")
}

/// `mcp_config.json` 路径。
pub fn config_path() -> PathBuf {
    config_dir().join("mcp_config.json")
}

/// 连接字段。
const TRANSPORT_FIELDS: &[&str] = &["type", "command", "args", "cwd", "env", "url", "headers"];

/// Antigravity 注入器。
pub struct AgyMcpInjector;

/// 转成 Antigravity 条目。不写 type，不支持 SSE。
fn to_agy_entry(s: &McpServer) -> anyhow::Result<Map<String, Value>> {
    if s.transport == Transport::Sse {
        anyhow::bail!(
            "Antigravity 只支持 stdio 和 Streamable HTTP，不支持 SSE（{}）",
            s.id
        );
    }
    let obj = super::shared::transport_entry(s);
    Ok(obj)
}

impl McpInjector for AgyMcpInjector {
    fn agent(&self) -> &'static str {
        "agy"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        if !path.parent().is_some_and(|p| p.exists()) {
            return Ok(Vec::new());
        }

        // agy 首次启动建的 0 字节文件当作没有配置
        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let pre_hash = hash_file(&path)?;
        let mut root = super::shared::read_json(&path)?;
        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 取消分配的：只标 disabled，条目留下
        for s in servers.iter().filter(|s| !s.agents.iter().any(|a| a == "agy")) {
            if let Some(Value::Object(entry)) = map.get_mut(&s.id) {
                if entry.get("disabled") != Some(&Value::Bool(true)) {
                    entry.insert("disabled".into(), Value::Bool(true));
                    summary.push(format!("停用 {}（未分配给 agy，条目保留）", s.display_name()));
                    changed = true;
                }
            }
        }

        for s in super::shared::wanted(servers, "agy") {
            let conn = to_agy_entry(s)?;
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
            // serverUrl 是 Antigravity 的旧写法，清理掉
            entry.remove("serverUrl");
            for (k, v) in &conn {
                entry.insert(k.clone(), v.clone());
            }
            entry.remove("enabled");
            entry.insert("disabled".into(), Value::Bool(false));

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
            agent: "agy".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn server(transport: Transport) -> McpServer {
        McpServer {
            id: "x".into(),
            name: "x".into(),
            transport,
            command: "node".into(),
            args: Vec::new(),
            cwd: String::new(),
            url: "https://example.com/sse".into(),
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["agy".into()],
            note: String::new(),
        }
    }

    #[test]
    fn rejects_sse() {
        assert!(to_agy_entry(&server(Transport::Sse)).is_err());
        assert!(to_agy_entry(&server(Transport::Http)).is_ok());
    }
}
