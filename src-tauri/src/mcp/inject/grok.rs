//! Grok Build 的 MCP 注入器：`~/.grok/config.toml` 的 `[mcp_servers.<id>]`。
//!
//! 结构和 Codex 几乎一样，但**请求头字段是 `headers`，不是 `http_headers`**。
//! 两个工具共用同一套转换时，Grok 必须把字段名改回来。
//!
//! 同样不写 `type`，由 `command` / `url` 推断。
//!
//! 本机未安装，实现按 CC Switch 的 `mcp/grokbuild.rs` 移植。

use super::shared::{prune_unassigned_toml, wanted};
use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use std::path::PathBuf;

/// stdio 专属字段。写远程条目时要清掉。
const STDIO_FIELDS: &[&str] = &["command", "args", "cwd", "env", "env_vars"];
/// 远程专属字段。写 stdio 条目时要清掉。
const REMOTE_FIELDS: &[&str] = &["url", "headers", "http_headers", "bearer_token_env_var"];

/// Grok Build 的 config.toml 路径。
pub fn config_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".grok")
        .join("config.toml")
}

/// Grok 注入器。
pub struct GrokMcpInjector;

/// 转成 Grok 的 `[mcp_servers.<id>]` 表。不写 type，头字段用 `headers`。
fn to_grok_table(s: &McpServer) -> toml_edit::Table {
    let mut t = toml_edit::Table::new();
    match s.transport {
        Transport::Stdio => {
            t["command"] = toml_edit::value(s.command.clone());
            let mut arr = toml_edit::Array::new();
            for a in &s.args {
                arr.push(a.clone());
            }
            if !arr.is_empty() {
                t["args"] = toml_edit::value(arr);
            }
            if !s.cwd.is_empty() {
                t["cwd"] = toml_edit::value(s.cwd.clone());
            }
            if !s.env.is_empty() {
                let mut e = toml_edit::Table::new();
                for (k, v) in &s.env {
                    e[k] = toml_edit::value(v.clone());
                }
                t["env"] = toml_edit::Item::Table(e);
            }
        }
        Transport::Sse | Transport::Http => {
            t["url"] = toml_edit::value(s.url.clone());
            if !s.headers.is_empty() {
                // Grok 认 headers（Codex 认 http_headers，两者不同）
                let mut h = toml_edit::Table::new();
                for (k, v) in &s.headers {
                    h[k] = toml_edit::value(v.clone());
                }
                t["headers"] = toml_edit::Item::Table(h);
            }
        }
    }
    t
}

impl McpInjector for GrokMcpInjector {
    fn agent(&self) -> &'static str {
        "grok"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        // Grok 没装就不写
        if !path.parent().is_some_and(|p| p.exists()) {
            return Ok(Vec::new());
        }

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let pre_hash = hash_file(&path)?;
        let uses_crlf = orig.contains("\r\n");
        let had_trailing = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = if orig.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            orig.parse()
                .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", path.display()))?
        };

        if !doc.contains_key("mcp_servers") {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let tbl = doc["mcp_servers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp_servers 不是表", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        for id in prune_unassigned_toml(tbl, "grok", servers) {
            summary.push(format!("移除 {id}（未分配给 grok）"));
            changed = true;
        }

        for s in wanted(servers, "grok") {
            let src = to_grok_table(s);
            match tbl.get_mut(&s.id).and_then(|i| i.as_table_mut()) {
                Some(t) => {
                    // toml_edit::Table 没有 PartialEq，比较序列化后的形状
                    let before = t.to_string();
                    // 只清「另一种传输方式」的专属字段，保留 Grok 自己的
                    let to_clear: &[&str] = match s.transport {
                        Transport::Stdio => REMOTE_FIELDS,
                        _ => STDIO_FIELDS,
                    };
                    for f in to_clear {
                        t.remove(*f);
                    }
                    t.remove("type");
                    for (k, v) in src.iter() {
                        t[k] = v.clone();
                    }
                    if t.to_string() != before {
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                    }
                }
                None => {
                    tbl.insert(&s.id, toml_edit::Item::Table(src));
                    summary.push(format!("新增 {}", s.display_name()));
                    changed = true;
                }
            }
        }

        let mut content = doc.to_string();
        if uses_crlf {
            content = content.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if !had_trailing {
            while content.ends_with("\r\n") || content.ends_with('\n') {
                if content.ends_with("\r\n") {
                    content.truncate(content.len() - 2);
                } else {
                    content.truncate(content.len() - 1);
                }
            }
        }

        Ok(vec![McpFilePlan {
            path,
            agent: "grok".to_string(),
            content: if changed { content } else { orig },
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

    #[test]
    fn remote_uses_headers_not_http_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), "Bearer x".into());
        let s = McpServer {
            id: "r".into(),
            name: "r".into(),
            transport: Transport::Http,
            command: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            url: "https://example.com/mcp".into(),
            headers,
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["grok".into()],
            note: String::new(),
        };
        let t = to_grok_table(&s);
        assert!(t.contains_key("headers"), "Grok 认 headers");
        assert!(!t.contains_key("http_headers"), "Grok 不认 http_headers");
        assert!(!t.contains_key("type"), "不写 type");
    }
}
