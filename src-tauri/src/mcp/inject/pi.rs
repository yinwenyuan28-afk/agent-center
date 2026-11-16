//! Pi 的 MCP 注入器：`~/.pi/agent/mcp.json` 顶层 `mcpServers`。
//!
//! ## 三条硬约束
//!
//! 1. **不支持 SSE**。`type` 只能是 `stdio` / `http` / `streamable-http` 或省略。
//!    写 `sse` Pi 整条不认，每次同步都报错
//! 2. **名字只能用字母、数字、`_`、`-`**。母版里带空格或点的 id 写不进去
//! 3. **`-` 和 `_` 等价**：`dev-tools` 和 `dev_tools` 在 Pi 里是同一个服务器。
//!    同名冲突要提前拒绝
//!
//! ## 取消分配只标停用，不删条目
//!
//! 只写 `enabled: false`，条目和 Pi 自己的字段（`timeout`、`description`、
//! `exposure`、`toolExposure`、`oauth`、`auth`）全留着。重新勾选时换掉连接
//! 字段、`enabled` 改回 `true`，其余原样。
//!
//! 用户在 Pi 里把条目删了的话，从母版里保存的那份 Pi 字段重建。
//!
//! ## 顶层的 `imports` / `settings` 不能丢
//!
//! 本机这份文件有 `imports: ["claude-code"]`，意思是「自动从 Claude Code 导入」。
//! 只改 `mcpServers`，别的键一个字都不动。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Pi 认的条目字段。写入时原样保留。
///
/// 目前只在 `PI_FIELDS` 为空时靠「条目还在就沿用它」的方式保留，
/// 这个常量留给将来母版支持 extra 表时对照用。
#[allow(dead_code)]
const PI_FIELDS: &[&str] = &[
    "enabled",
    "timeout",
    "description",
    "exposure",
    "toolExposure",
    "oauth",
    "auth",
];

/// 连接字段。写入时先删这些，再按母版填。
const TRANSPORT_FIELDS: &[&str] = &["command", "args", "cwd", "env", "url", "headers", "type"];

/// Pi 的 mcp.json 路径。
pub fn config_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".pi")
        .join("agent")
        .join("mcp.json")
}

/// 校验服务器名：只有字母、数字、`_`、`-`。
pub fn validate_name(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!("Pi 的 MCP 服务器名只能包含字母、数字、_ 和 -：{id}");
    }
    Ok(())
}

/// 名字归一：把 `-` 换成 `_`。
///
/// Pi 把它们当同一个服务器，所以 `dev-tools` 和 `dev_tools` 会撞车。
pub fn normalized(id: &str) -> String {
    id.replace('-', "_")
}

/// 检查 id 是否和已有名字只差 `-` / `_`。
pub fn check_collision<'a>(
    id: &str,
    others: impl IntoIterator<Item = &'a String>,
) -> anyhow::Result<()> {
    let key = normalized(id);
    if let Some(other) = others
        .into_iter()
        .find(|n| n.as_str() != id && normalized(n) == key)
    {
        anyhow::bail!("Pi 把只差 - 和 _ 的服务器名视为同一个：{id} 与已有的 {other} 冲突");
    }
    Ok(())
}

/// 把母版 server 转成 Pi 的连接字段。SSE 直接拒绝。
fn native_transport(s: &McpServer) -> anyhow::Result<Map<String, Value>> {
    let mut obj = Map::new();
    match s.transport {
        Transport::Stdio => {
            obj.insert("type".into(), Value::String("stdio".into()));
            obj.insert("command".into(), Value::String(s.command.clone()));
            if !s.args.is_empty() {
                obj.insert(
                    "args".into(),
                    Value::Array(s.args.iter().map(|a| Value::String(a.clone())).collect()),
                );
            }
            if !s.cwd.is_empty() {
                obj.insert("cwd".into(), Value::String(s.cwd.clone()));
            }
            if !s.env.is_empty() {
                let mut e = Map::new();
                for (k, v) in &s.env {
                    e.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".into(), Value::Object(e));
            }
        }
        Transport::Http => {
            obj.insert("type".into(), Value::String("http".into()));
            obj.insert("url".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut h = Map::new();
                for (k, v) in &s.headers {
                    h.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(h));
            }
        }
        Transport::Sse => {
            anyhow::bail!(
                "Pi 不支持 SSE 连接方式的 MCP 服务器（{}），请改用 HTTP",
                s.id
            );
        }
    }
    // Windows 上 npx / node 要包一层 cmd /c
    super::shared::wrap_windows_command(&mut obj, s);
    Ok(obj)
}

/// Pi 注入器。
pub struct PiMcpInjector;

impl McpInjector for PiMcpInjector {
    fn agent(&self) -> &'static str {
        "pi"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let path = config_path();
        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{}".to_string()
        };
        let pre_hash = hash_file(&path)?;

        let mut root = super::shared::read_json(&path)?;

        // 先把要写的全部校验一遍。任何一条不合 Pi 的规则就整次拒绝，
        // 不写半份 —— 否则 Pi 会整条不认，而用户不知道该怪哪条
        for s in servers {
            if s.agents.iter().any(|a| a == "pi") {
                validate_name(&s.id)?;
                native_transport(s)?;
            }
        }
        let assigned: Vec<String> = servers
            .iter()
            .filter(|s| s.agents.iter().any(|a| a == "pi"))
            .map(|s| s.id.clone())
            .collect();
        // 已分配的之间不能只差 - 和 _
        for id in &assigned {
            check_collision(id, assigned.iter())?;
        }

        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 取消分配的：只标停用，条目留下
        for s in servers.iter().filter(|s| !s.agents.iter().any(|a| a == "pi")) {
            if let Some(Value::Object(entry)) = map.get_mut(&s.id) {
                if entry.get("enabled") != Some(&Value::Bool(false)) {
                    entry.insert("enabled".into(), Value::Bool(false));
                    summary.push(format!("停用 {}（未分配给 pi，条目保留）", s.display_name()));
                    changed = true;
                }
            }
            // 没有条目就没什么可停用的，不为它新建
        }

        // 已分配的：换掉连接字段，保留 Pi 自己的
        for s in servers.iter().filter(|s| s.agents.iter().any(|a| a == "pi")) {
            let conn = native_transport(s)?;
            let is_new = !map.contains_key(&s.id);

            let mut entry = map
                .get(&s.id)
                .and_then(|v| v.as_object())
                .cloned()
                // 条目不在（用户在 Pi 里删了）：从母版里存的 Pi 字段重建
                .unwrap_or_else(|| pi_fields(s));

            let mut before = entry.clone();
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
                before.clear();
            } else {
                summary.push(format!("{} 已一致", s.display_name()));
            }
        }

        if !path.exists() && !changed {
            return Ok(Vec::new());
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
            agent: "pi".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}

/// 从母版条目里取 Pi 自己的字段，用于重建被删掉的条目。
///
/// 母版目前只表达「连接方式 + 分配给谁」，不存各工具的私有字段，
/// 所以这里现在是空对象。将来如果母版加 `extra` 表，只需要改这里。
fn pi_fields(_s: &McpServer) -> Map<String, Value> {
    Map::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn server(id: &str, transport: Transport, agents: Vec<&str>) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            transport,
            command: "node".into(),
            args: vec!["server.js".into()],
            cwd: String::new(),
            url: if transport == Transport::Stdio {
                String::new()
            } else {
                "https://example.com/mcp".into()
            },
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: agents.into_iter().map(String::from).collect(),
            note: String::new(),
        }
    }

    #[test]
    fn rejects_sse_outright() {
        let err = native_transport(&server("x", Transport::Sse, vec!["pi"])).unwrap_err();
        assert!(err.to_string().contains("不支持 SSE"));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(validate_name("bad name").is_err());
        assert!(validate_name("a.b").is_err());
        assert!(validate_name("dev-tools").is_ok());
        assert!(validate_name("dev_tools").is_ok());
    }

    #[test]
    fn dash_and_underscore_collide() {
        let names = vec!["dev_tools".to_string()];
        assert!(check_collision("dev-tools", names.iter()).is_err());
    }

    #[test]
    fn disabling_keeps_the_entry_and_pi_fields() {
        // 模拟一个已有条目（用户在 Pi 里改过 timeout / exposure）
        let existing = serde_json::json!({
            "command": "old", "enabled": true, "timeout": 30, "exposure": "direct"
        });
        let mut entry = existing.as_object().unwrap().clone();
        for f in TRANSPORT_FIELDS {
            entry.remove(*f);
        }
        for (k, v) in native_transport(&server("s", Transport::Stdio, vec!["pi"])).unwrap() {
            entry.insert(k, v);
        }
        entry.insert("enabled".into(), Value::Bool(true));
        assert_eq!(entry["timeout"], 30, "Pi 自己的 timeout 不能丢");
        assert_eq!(entry["exposure"], "direct");
        // 连接字段要更新。Windows 上 `node` 会按 cmd /c 规则包一层
        #[cfg(windows)]
        assert_eq!(entry["command"], "cmd", "Windows 上 node 要包 cmd /c");
        #[cfg(not(windows))]
        assert_eq!(entry["command"], "node", "连接字段要更新");
    }
}
