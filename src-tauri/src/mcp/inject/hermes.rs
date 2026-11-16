//! Hermes 的 MCP 注入器：`$HERMES_HOME/config.yaml` 的 `mcp_servers` 段。
//!
//! ```yaml
//! mcp_servers:
//!   time:
//!     command: uvx
//!     args: ["mcp-server-time"]
//!     timeout: 120
//!   remote:
//!     url: https://example.com/mcp
//!     headers:
//!       Authorization: Bearer xxx
//!     enabled: true
//! ```
//!
//! ## 没有 `type`
//!
//! 由 `command` / `url` 推断。
//!
//! ## 行级编辑，不用 YAML 库
//!
//! `config.yaml` 里有大段注释、`skills.disabled` 长数组、`providers:` 里
//! 各家 provider 的嵌套配置。用 `serde_yaml` 整体重写会把注释全丢掉、
//! 还会把 provider 段落重排成另一个形状。所以这里只按行操作 `mcp_servers` 段。
//!
//! ## 私有字段
//!
//! `enabled`、`timeout`、`connect_timeout`、`tools`、`sampling`、`roots`、`auth`
//! 都是 Hermes 自己的。已有条目只改连接字段（`command`/`args`/`env`/`url`/`headers`），
//! 其余原样。`auth: "oauth"` 丢了会从已授权降级成未认证。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use std::path::PathBuf;

/// Hermes 的 config.yaml 路径。
pub fn config_path() -> PathBuf {
    crate::providers::hermes::config_path()
}

/// Hermes 注入器。
pub struct HermesMcpInjector;

/// 生成一个 server 的 YAML 片段（缩进 2 起）。
///
/// `indent` 是条目名的缩进层级 —— 放在 `mcp_servers:` 下面时是 2。
fn server_yaml(s: &McpServer, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    let inner2 = " ".repeat(indent + 4);
    let mut out = String::new();
    out.push_str(&format!("{pad}{}:\n", s.id));

    match s.transport {
        Transport::Stdio => {
            out.push_str(&format!("{inner}command: {}\n", yaml_scalar(&s.command)));
            if !s.args.is_empty() {
                let args: Vec<String> = s.args.iter().map(|a| yaml_quote(a)).collect();
                out.push_str(&format!("{inner}args: [{}]\n", args.join(", ")));
            }
            if !s.cwd.is_empty() {
                out.push_str(&format!("{inner}cwd: {}\n", yaml_scalar(&s.cwd)));
            }
            if !s.env.is_empty() {
                out.push_str(&format!("{inner}env:\n"));
                for (k, v) in &s.env {
                    out.push_str(&format!("{inner2}{k}: {}\n", yaml_quote(v)));
                }
            }
        }
        Transport::Sse | Transport::Http => {
            out.push_str(&format!("{inner}url: {}\n", yaml_scalar(&s.url)));
            if !s.headers.is_empty() {
                out.push_str(&format!("{inner}headers:\n"));
                for (k, v) in &s.headers {
                    out.push_str(&format!("{inner2}{k}: {}\n", yaml_quote(v)));
                }
            }
        }
    }
    out
}

/// 只给可能被 YAML 误解析的标量加引号。
///
/// 只在值里出现 `: ` 或 ` #`（会被当分隔符）、或以特殊字符开头时才加。
/// URL 里的 `https://` 不该被加引号 —— 那样每个 URL 都变成带引号的，
/// diff 噪音大且和用户手写的风格不一致。
fn yaml_scalar(v: &str) -> String {
    if v.is_empty() {
        return "\"\"".to_string();
    }
    let needs = v.contains(": ")
        || v.contains(" #")
        || v.starts_with(['[', '{', '&', '*', '!', '|', '>', '%', '@', '`'])
        || v.eq_ignore_ascii_case("true")
        || v.eq_ignore_ascii_case("false")
        || v.eq_ignore_ascii_case("null")
        || v.parse::<f64>().is_ok();
    if needs {
        yaml_quote(v)
    } else {
        v.to_string()
    }
}

/// 强制加双引号并转义。
fn yaml_quote(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 找顶层某个块的起止行号。
fn find_block(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.starts_with(&format!("{key}:")))?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        if l.len() - l.trim_start().len() == 0 {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// 在块里找某个 server 的起止行号（条目名缩进 2）。
fn find_entry(lines: &[&str], block: (usize, usize), id: &str) -> Option<(usize, usize)> {
    let (start, end) = block;
    let key = format!("{id}:");
    let entry_start = (start + 1..end).find(|&i| {
        let l = lines[i];
        l.len() - l.trim_start().len() == 2 && l.trim_start().starts_with(&key)
    })?;
    let mut entry_end = end;
    for i in entry_start + 1..end {
        if lines[i].trim().is_empty() || lines[i].trim_start().starts_with('#') {
            continue;
        }
        if lines[i].len() - lines[i].trim_start().len() <= 2 {
            entry_end = i;
            break;
        }
    }
    Some((entry_start, entry_end))
}

impl McpInjector for HermesMcpInjector {
    fn agent(&self) -> &'static str {
        "hermes"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = config_path();
        // Hermes 没装（没有 config.yaml）就不写，也不创建
        if !path.exists() {
            return Ok(Vec::new());
        }

        let orig = std::fs::read_to_string(&path)?;
        let pre_hash = hash_file(&path)?;
        let mut lines: Vec<String> = orig.lines().map(str::to_string).collect();
        let mut summary = Vec::new();
        let mut changed = false;

        // 取消分配的：删掉条目
        for s in servers.iter().filter(|s| !s.agents.iter().any(|a| a == "hermes")) {
            let snapshot: Vec<&str> = lines.iter().map(String::as_str).collect();
            if let Some(block) = find_block(&snapshot, "mcp_servers") {
                if let Some((es, ee)) = find_entry(&snapshot, block, &s.id) {
                    lines.drain(es..ee);
                    summary.push(format!("移除 {}（未分配给 hermes）", s.display_name()));
                    changed = true;
                }
            }
        }

        for s in super::shared::wanted(servers, "hermes") {
            let snapshot: Vec<&str> = lines.iter().map(String::as_str).collect();
            match find_block(&snapshot, "mcp_servers") {
                Some(block) => {
                    if let Some((es, ee)) = find_entry(&snapshot, block, &s.id) {
                        // 已有条目：只替换连接字段那几行，Hermes 自己的
                        // timeout / connect_timeout / tools / auth 全留着
                        let keep: Vec<String> = lines[es + 1..ee]
                            .iter()
                            .filter(|l| {
                                let t = l.trim();
                                !(t.starts_with("command:")
                                    || t.starts_with("args:")
                                    || t.starts_with("cwd:")
                                    || t.starts_with("env:")
                                    || t.starts_with("url:")
                                    || t.starts_with("headers:")
                                    // env / headers 的子行缩进 6
                                    || (l.len() - l.trim_start().len() >= 6
                                        && !t.starts_with("timeout:")
                                        && !t.starts_with("connect_timeout:")
                                        && !t.starts_with("tools:")
                                        && !t.starts_with("sampling:")
                                        && !t.starts_with("roots:")
                                        && !t.starts_with("auth:")))
                            })
                            .cloned()
                            .collect();

                        let mut new_lines: Vec<String> = server_yaml(s, 2).lines().map(String::from).collect();
                        new_lines.extend(keep);

                        if lines[es..ee].to_vec() != new_lines {
                            lines.splice(es..ee, new_lines);
                            summary.push(format!("更新 {}", s.display_name()));
                            changed = true;
                        } else {
                            summary.push(format!("{} 已一致", s.display_name()));
                        }
                    } else {
                        let snippet: Vec<String> =
                            server_yaml(s, 2).lines().map(String::from).collect();
                        let at = block.1;
                        for (off, line) in snippet.into_iter().enumerate() {
                            lines.insert(at + off, line);
                        }
                        summary.push(format!("新增 {}", s.display_name()));
                        changed = true;
                    }
                }
                None => {
                    // 整个 mcp_servers 段都没有，建一个
                    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push("mcp_servers:".to_string());
                    for line in server_yaml(s, 2).lines() {
                        lines.push(line.to_string());
                    }
                    summary.push(format!("创建 mcp_servers 段并新增 {}", s.display_name()));
                    changed = true;
                }
            }
        }

        let content = if changed {
            let mut text = lines.join("\n");
            if orig.ends_with('\n') {
                text.push('\n');
            }
            text
        } else {
            orig
        };

        Ok(vec![McpFilePlan {
            path,
            agent: "hermes".to_string(),
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

    #[test]
    fn urls_are_not_quoted() {
        assert_eq!(yaml_scalar("https://example.com/mcp"), "https://example.com/mcp");
        // 含 ": " 的才要加引号
        assert!(yaml_scalar("a: b").starts_with('"'));
        // 会被当布尔的要加
        assert!(yaml_scalar("true").starts_with('"'));
    }
}
