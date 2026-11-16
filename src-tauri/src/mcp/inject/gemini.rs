//! Gemini CLI 的 MCP 注入器：`~/.gemini/settings.json` 顶层 `mcpServers`。
//!
//! ## 三个和别家不同的地方
//!
//! 1. **没有 `type` 字段**，靠字段名推断传输方式：
//!    有 `command` 是 stdio，有 `httpUrl` 是 HTTP，有 `url` 是 SSE
//! 2. **HTTP 用 `httpUrl`，不是 `url`**。写 `url` 会被 Gemini 当成 SSE
//! 3. **`timeout` 只收毫秒，且只有一个**。Codex / Claude 那种
//!    `startup_timeout_sec` / `tool_timeout_sec` 要换算：取最大值。
//!    都没设时用默认（启动 10 000、工具 60 000），所以最终是 60 000
//!
//! 本机未安装，实现按 CC Switch 的 `mcp/gemini.rs` + `gemini_mcp.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// 默认启动超时（毫秒）
const DEFAULT_STARTUP_MS: u64 = 10_000;
/// 默认工具调用超时（毫秒）
const DEFAULT_TOOL_MS: u64 = 60_000;

/// Gemini 的 settings.json 路径。
pub fn config_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".gemini")
        .join("settings.json")
}

/// Gemini 注入器。
pub struct GeminiMcpInjector;

/// 把一个母版 server 转成 Gemini 条目。
fn to_gemini_entry(s: &McpServer) -> Map<String, Value> {
    let mut obj = Map::new();
    match s.transport {
        Transport::Stdio => {
            // Gemini 不写 type，靠 command 推断。Windows 上 npx / node 要包 cmd /c
            let mut cmd = super::shared::windows_command_array(s);
            let head = if cmd.is_empty() {
                String::new()
            } else {
                cmd.remove(0)
            };
            obj.insert("command".into(), Value::String(head));
            if !cmd.is_empty() {
                obj.insert(
                    "args".into(),
                    Value::Array(cmd.into_iter().map(Value::String).collect()),
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
            // HTTP 必须叫 httpUrl，写 url 会被当成 SSE
            obj.insert("httpUrl".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut h = Map::new();
                for (k, v) in &s.headers {
                    h.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(h));
            }
        }
        Transport::Sse => {
            obj.insert("url".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut h = Map::new();
                for (k, v) in &s.headers {
                    h.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(h));
            }
        }
    }
    // 只有一个 timeout，收毫秒
    obj.insert(
        "timeout".into(),
        Value::Number(DEFAULT_STARTUP_MS.max(DEFAULT_TOOL_MS).into()),
    );
    obj
}

impl McpInjector for GeminiMcpInjector {
    fn agent(&self) -> &'static str {
        "gemini"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        // Gemini 没装就不写，也不创建目录
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

        for id in super::shared::prune_unassigned(map, "gemini", servers) {
            summary.push(format!("移除 {id}（未分配给 gemini）"));
            changed = true;
        }

        for s in super::shared::wanted(servers, "gemini") {
            let new_entry = Value::Object(to_gemini_entry(s));
            match map.get(&s.id) {
                Some(existing) if *existing == new_entry => {
                    summary.push(format!("{} 已一致", s.display_name()));
                }
                Some(_) => {
                    map.insert(s.id.clone(), new_entry);
                    summary.push(format!("更新 {}", s.display_name()));
                    changed = true;
                }
                None => {
                    map.insert(s.id.clone(), new_entry);
                    summary.push(format!("新增 {}", s.display_name()));
                    changed = true;
                }
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
            agent: "gemini".to_string(),
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
            agents: vec!["gemini".into()],
            note: String::new(),
        }
    }

    #[test]
    fn http_uses_http_url_not_url() {
        let e = to_gemini_entry(&remote("r"));
        assert!(e.contains_key("httpUrl"), "HTTP 要写 httpUrl");
        assert!(!e.contains_key("url"), "不能写 url，那会被当成 SSE");
        assert!(!e.contains_key("type"), "Gemini 不认 type");
    }

    #[test]
    fn timeout_is_one_millisecond_field() {
        let e = to_gemini_entry(&remote("r"));
        assert_eq!(e["timeout"], 60_000);
    }
}
