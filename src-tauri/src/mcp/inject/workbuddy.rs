//! WorkBuddy 的 MCP 注入器：`~/.workbuddy/connectors/default/mcp.json`。
//!
//! ## 和别家的差异
//!
//! | | WorkBuddy |
//! | --- | --- |
//! | 启用字段 | `disabled`（反义），不是 `enabled` |
//! | `type` 词表 | `stdio` / `sse` / `http` / `streamableHttp` / `streamable-http` 都出现过 |
//! | 请求头 | `headers` 和 `staticHeaders` 两个都见过 |
//! | 私有字段 | `runtime`、`staticEnv`、`npmRegistry`、`disabledTools`、`timeout` |
//!
//! ## 绝不碰 `connector:` 前缀的条目
//!
//! 本机有 83 个，全是内置市场目录、全部 `disabled: true`。它们不在母版里，
//! 注入器的清理逻辑只看母版声明过的 id，所以永远不碰这一批。
//!
//! CC Switch 没有这个工具的注入器，这部分是我们自己核实的。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::providers::safe_write::hash_file;
use serde_json::Value;
use std::path::PathBuf;

/// WorkBuddy 的 mcp.json 路径。
pub fn config_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".workbuddy")
        .join("connectors")
        .join("default")
        .join("mcp.json")
}

/// WorkBuddy 注入器。
pub struct WorkBuddyMcpInjector;

/// 连接字段。写入时先删这些，保留 `runtime` / `staticEnv` / `disabledTools` 等。
const TRANSPORT_FIELDS: &[&str] = &["type", "command", "args", "cwd", "env", "url", "headers"];

impl McpInjector for WorkBuddyMcpInjector {
    fn agent(&self) -> &'static str {
        "workbuddy"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let orig = std::fs::read_to_string(&path)?;
        let pre_hash = hash_file(&path)?;
        let mut root = super::shared::read_json(&path)?;
        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的。只删母版里的 id，83 个 connector 不在其中
        for id in super::shared::prune_unassigned(map, "workbuddy", servers) {
            summary.push(format!("移除 {id}（未分配给 workbuddy）"));
            changed = true;
        }

        for s in super::shared::wanted(servers, "workbuddy") {
            let is_new = !map.contains_key(&s.id);

            if is_new {
                let mut entry = super::shared::transport_entry(s);
                // WorkBuddy 用 disabled（反义）
                entry.insert("disabled".into(), Value::Bool(!s.enabled));
                map.insert(s.id.clone(), Value::Object(entry));
                summary.push(format!("新增 {}", s.display_name()));
                changed = true;
                continue;
            }

            // 已有条目：只改连接字段，runtime / staticEnv / disabledTools 等留着
            let Some(obj) = map.get_mut(&s.id).and_then(|v| v.as_object_mut()) else {
                continue;
            };
            let before = obj.clone();

            for f in TRANSPORT_FIELDS {
                obj.remove(*f);
            }
            // staticHeaders 是 WorkBuddy 独有的头写法。母版给了 headers 时用它，
            // 没给就保留原来的 staticHeaders
            let had_static = obj.contains_key("staticHeaders");
            for (k, v) in super::shared::transport_entry(s) {
                if k == "headers" && had_static {
                    obj.insert("staticHeaders".into(), v);
                } else {
                    obj.insert(k, v);
                }
            }
            obj.insert("disabled".into(), Value::Bool(!s.enabled));

            if *obj != before {
                summary.push(format!("更新 {}", s.display_name()));
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
            agent: "workbuddy".to_string(),
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
    use crate::mcp::model::Transport;
    use std::collections::BTreeMap;

    fn remote(id: &str) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            transport: Transport::Http,
            command: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            url: "https://example.com/mcp".into(),
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["workbuddy".into()],
            note: String::new(),
        }
    }

    #[test]
    fn only_touches_master_ids_and_keeps_market_connectors() {
        // 直接验证 prune_unassigned 的语义：母版里没有的 id 一个都不删。
        // 这是 83 个 connector 安全的唯一保障
        let mut map = serde_json::json!({
            "connector:linear-mcp": {"url": "https://mcp.linear.app/mcp", "disabled": true},
            "connector:github": {"url": "https://api.githubcopilot.com/mcp/", "disabled": true},
            "mine": {"command": "old"}
        });
        // 母版里有 mine，但只分配给了别的 agent → 要删
        let mut only_elsewhere = remote("mine");
        only_elsewhere.agents = vec!["claude".into()];

        let obj = map.as_object_mut().unwrap();
        let removed =
            super::super::shared::prune_unassigned(obj, "workbuddy", &[only_elsewhere]);
        assert!(removed.contains(&"mine".to_string()), "母版里没分配的要删");
        assert!(obj.contains_key("connector:linear-mcp"), "市场连接器不能碰");
        assert!(obj.contains_key("connector:github"), "市场连接器不能碰");
    }

    #[test]
    fn uses_disabled_instead_of_enabled() {
        let mut entry = super::super::shared::transport_entry(&remote("x"));
        entry.insert("disabled".into(), Value::Bool(false));
        assert!(entry.contains_key("disabled"));
        assert!(!entry.contains_key("enabled"));
        assert!(!entry.contains_key("type"), "WorkBuddy 的 type 由字段推断");
    }
}
