//! 从各 agent 现有配置里读出 MCP server，用于和母版比对。
//!
//! **全部只读**。各家的结构差异在 `plan/mcp-management.md` 里有完整表格，
//! 这里只做解析和归一化。

use super::model::{McpServer, Transport};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 从某个 agent 读出来的 server。
///
/// 和母版的 [`McpServer`] 区别：这个带 `location` 说明在哪读到的，
/// 且不保证字段完整（比如 opencode 的 command 数组拆不出原始 args）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundServer {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    /// 是否启用
    pub enabled: bool,
    /// 在哪读到的
    pub location: String,
}

/// 一个 agent 的扫描结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAgentScan {
    pub agent: String,
    pub present: bool,
    pub servers: Vec<FoundServer>,
    pub unsupported_reason: Option<String>,
}

fn home() -> PathBuf {
    crate::providers::store::home_dir()
}

fn read_json(p: &PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// 把 JSON 里的一个 server 对象解析成 [`FoundServer`]。
///
/// 兼容几个变体：
/// - `command` 可能是字符串（Claude/pi/Qoder/Codex）或数组（opencode）
/// - `type` 各家写法不同（local/remote、streamableHttp、sse）
/// - 启用状态有的是 `enabled`，有的是 `disabled`（取反）
fn parse_server(id: &str, v: &Value, location: &str) -> FoundServer {
    let obj = v.as_object();

    let get_str = |k: &str| -> String {
        obj.and_then(|o| o.get(k))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };

    // command：字符串或数组
    let (command, args) = match obj.and_then(|o| o.get("command")) {
        Some(Value::String(s)) => {
            let args = obj
                .and_then(|o| o.get("args"))
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            (s.clone(), args)
        }
        // opencode：command 是数组，第一个是命令，其余是参数
        Some(Value::Array(arr)) => {
            let mut it = arr.iter().filter_map(|x| x.as_str());
            let cmd = it.next().unwrap_or("").to_string();
            let rest: Vec<String> = it.map(String::from).collect();
            (cmd, rest)
        }
        _ => (String::new(), Vec::new()),
    };

    let url = get_str("url");

    // transport：没有 type 时按有没有 url 判断
    let raw_type = get_str("type");
    let transport = if !raw_type.is_empty() {
        Transport::parse(&raw_type)
    } else if !url.is_empty() {
        // 默认当 http（sse 已过时）
        Transport::Http
    } else {
        Transport::Stdio
    };

    // 启用状态：enabled 优先，其次 disabled 取反
    let enabled = match obj {
        Some(o) => {
            if o.contains_key("enabled") {
                o.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true)
            } else if o.contains_key("disabled") {
                !o.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)
            } else {
                true
            }
        }
        None => true,
    };

    let name = {
        let n = get_str("name");
        if n.is_empty() {
            id.to_string()
        } else {
            n
        }
    };

    FoundServer {
        id: id.to_string(),
        name,
        transport: transport.as_str().to_string(),
        command,
        args,
        url,
        enabled,
        location: location.to_string(),
    }
}

/// 通用的 `mcpServers` map 扫描（Claude / pi / Qoder / Cursor 都用这个结构）。
fn scan_mcp_servers_map(path: &PathBuf, agent: &str, label: &str) -> McpAgentScan {
    let present = path.exists();
    let mut servers = Vec::new();

    if let Some(json) = read_json(path) {
        if let Some(map) = json.get("mcpServers").and_then(|v| v.as_object()) {
            for (id, v) in map {
                servers.push(parse_server(id, v, &format!("{label} · mcpServers.{id}")));
            }
        }
    }

    McpAgentScan {
        agent: agent.to_string(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// Claude Code：`~/.claude.json` 顶层 `mcpServers`。
pub fn scan_claude() -> McpAgentScan {
    scan_mcp_servers_map(&home().join(".claude.json"), "claude", "~/.claude.json")
}

/// Cursor：`~/.cursor/mcp.json`。
pub fn scan_cursor() -> McpAgentScan {
    scan_mcp_servers_map(&home().join(".cursor").join("mcp.json"), "cursor", "~/.cursor/mcp.json")
}

/// pi：`~/.pi/agent/mcp.json`。
pub fn scan_pi() -> McpAgentScan {
    scan_mcp_servers_map(
        &home().join(".pi").join("agent").join("mcp.json"),
        "pi",
        "~/.pi/agent/mcp.json",
    )
}

/// Qoder：`~/.qoder/settings.json` 的 `mcpServers`。
pub fn scan_qoder() -> McpAgentScan {
    scan_mcp_servers_map(
        &home().join(".qoder").join("settings.json"),
        "qoder",
        "~/.qoder/settings.json",
    )
}

/// Codex：`~/.codex/config.toml` 的 `[mcp_servers.*]`。
pub fn scan_codex() -> McpAgentScan {
    let path = home().join(".codex").join("config.toml");
    let present = path.exists();
    let mut servers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(doc) = text.parse::<toml_edit::DocumentMut>() {
            if let Some(tbl) = doc.get("mcp_servers").and_then(|v| v.as_table()) {
                for (id, v) in tbl {
                    let get_str = |k: &str| {
                        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
                    };
                    let args = v
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    let raw_type = get_str("type");
                    let url = get_str("url");
                    let transport = if !raw_type.is_empty() {
                        Transport::parse(&raw_type)
                    } else if !url.is_empty() {
                        Transport::Http
                    } else {
                        Transport::Stdio
                    };

                    servers.push(FoundServer {
                        id: id.to_string(),
                        name: {
                            let n = get_str("name");
                            if n.is_empty() { id.to_string() } else { n }
                        },
                        transport: transport.as_str().to_string(),
                        command: get_str("command"),
                        args,
                        url,
                        enabled: true,
                        location: format!("config.toml · mcp_servers.{id}"),
                    });
                }
            }
        }
    }

    McpAgentScan {
        agent: "codex".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// opencode：`~/.config/opencode/opencode.json` 的 `mcp` 段。
pub fn scan_opencode() -> McpAgentScan {
    let path = home()
        .join(".config")
        .join("opencode")
        .join("opencode.json");
    let present = path.exists();
    let mut servers = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(map) = json.get("mcp").and_then(|v| v.as_object()) {
            for (id, v) in map {
                servers.push(parse_server(id, v, &format!("opencode.json · mcp.{id}")));
            }
        }
    }

    McpAgentScan {
        agent: "opencode".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// WorkBuddy：`~/.workbuddy/connectors/default/mcp.json`。
///
/// 这个文件有 83 个 connector，全是内置市场目录（`disabled: true`）。
/// 全读进来但标记为未启用，让 diff 能区分「市场目录」和「用户主动启用的」。
pub fn scan_workbuddy() -> McpAgentScan {
    let path = home()
        .join(".workbuddy")
        .join("connectors")
        .join("default")
        .join("mcp.json");
    let present = path.exists();
    let mut servers = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(map) = json.get("mcpServers").and_then(|v| v.as_object()) {
            for (id, v) in map {
                servers.push(parse_server(
                    id,
                    v,
                    &format!("workbuddy/connectors/mcp.json · {id}"),
                ));
            }
        }
    }

    McpAgentScan {
        agent: "workbuddy".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// ZCode / DSH 的 MCP 配置位置未探明。
///
/// 实测：
/// - `~/.zcode/cli/config.json` 只有 `plugins.enabledPlugins`
/// - `~/.dsh/profiles/desktop/cordis.patch.yml` 没有 `mcp` 段
pub fn scan_unknown(agent: &str, dir: &PathBuf, detail: &str) -> McpAgentScan {
    McpAgentScan {
        agent: agent.into(),
        present: dir.exists(),
        servers: Vec::new(),
        unsupported_reason: if dir.exists() {
            Some(detail.to_string())
        } else {
            None
        },
    }
}

/// ZCode：`~/.zcode/cli/config.json` 的 `mcp.servers`（嵌套两层）。
///
/// 注意不是顶层 `mcpServers` —— ZCode 的 schema 和 Claude/pi/Cursor 不同。
/// 也没法和它们共用 `scan_mcp_servers_map`，因为键路径多了两层。
pub fn scan_zcode() -> McpAgentScan {
    let path = home().join(".zcode").join("cli").join("config.json");
    let present = path.exists();
    let mut servers = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(map) = json
            .get("mcp")
            .and_then(|m| m.get("servers"))
            .and_then(|v| v.as_object())
        {
            for (id, v) in map {
                servers.push(parse_server(
                    id,
                    v,
                    &format!("zcode/cli/config.json · mcp.servers.{id}"),
                ));
            }
        }
    }

    McpAgentScan {
        agent: "zcode".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// Hermes：`$HERMES_HOME/config.yaml` 的 `mcp_servers` 段（YAML）。
///
/// 结构（行级解析，不引 YAML 库 —— config.yaml 里有大段注释和长数组，
/// 用库整体重写会全部丢失）：
///
/// ```yaml
/// mcp_servers:
///   time:
///     command: uvx
///     args: ["mcp-server-time"]
///     env:
///       KEY: value
///   remote:
///     url: https://example.com/mcp
///     headers:
///       Authorization: Bearer xxx
/// ```
pub fn scan_hermes() -> McpAgentScan {
    let path = crate::providers::hermes::config_path();
    let present = path.exists();
    let mut servers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = text.lines().collect();
        // 找 mcp_servers: 段
        if let Some(start) = lines.iter().position(|l| l.starts_with("mcp_servers:")) {
            let mut end = lines.len();
            for (i, l) in lines.iter().enumerate().skip(start + 1) {
                if l.trim().is_empty() {
                    continue;
                }
                if l.len() - l.trim_start().len() == 0 {
                    end = i;
                    break;
                }
            }

            let mut cur: Option<(String, FoundServer)> = None;

            fn flush(out: &mut Vec<FoundServer>, cur: &mut Option<(String, FoundServer)>) {
                if let Some((_, s)) = cur.take() {
                    out.push(s);
                }
            }

            for line in &lines[start + 1..end] {
                if line.trim().is_empty() || line.trim_start().starts_with('#') {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                let t = line.trim();

                if indent == 2 && t.ends_with(':') {
                    // 新 server
                    flush(&mut servers, &mut cur);
                    let id = t.trim_end_matches(':').to_string();
                    cur = Some((
                        id.clone(),
                        FoundServer {
                            id: id.clone(),
                            name: id,
                            transport: "stdio".to_string(),
                            command: String::new(),
                            args: Vec::new(),
                            url: String::new(),
                            enabled: true,
                            location: format!("config.yaml · mcp_servers.{}", t.trim_end_matches(':')),
                        },
                    ));
                    continue;
                }

                if let Some((_, s)) = cur.as_mut() {
                    if indent == 4 {
                        if let Some((k, v)) = t.split_once(':') {
                            let k = k.trim();
                            let v = v.trim();
                            match k {
                                "command" => s.command = v.to_string(),
                                "url" => {
                                    s.url = v.to_string();
                                    s.transport = "http".to_string();
                                }
                                "args" => {
                                    // 形如 ["a", "b"]
                                    let inner = v.trim_start_matches('[').trim_end_matches(']');
                                    s.args = inner
                                        .split(',')
                                        .map(|x| x.trim().trim_matches('"').to_string())
                                        .filter(|x| !x.is_empty())
                                        .collect();
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            flush(&mut servers, &mut cur);
        }
    }

    McpAgentScan {
        agent: "hermes".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// 扫描全部支持的 agent。
pub fn scan_all() -> Vec<McpAgentScan> {
    vec![
        scan_claude(),
        scan_codex(),
        scan_cursor(),
        scan_pi(),
        scan_opencode(),
        scan_qoder(),
        scan_workbuddy(),
        scan_zcode(),
        scan_hermes(),
        scan_unknown(
            "dsh",
            &home().join(".dsh"),
            "cordis.patch.yml 里没有 mcp 段，机制未探明",
        ),
    ]
}

/// 把母版 server 转成可比较的形态。
pub fn normalize_master(s: &McpServer) -> FoundServer {
    FoundServer {
        id: s.id.clone(),
        name: s.display_name().to_string(),
        transport: s.transport.as_str().to_string(),
        command: s.command.clone(),
        args: s.args.clone(),
        url: s.url.clone(),
        enabled: s.enabled,
        location: "母版".to_string(),
    }
}

/// 供 diff 使用：把 map 形式的差异压平。
pub fn index_by_id(servers: &[FoundServer]) -> BTreeMap<&str, &FoundServer> {
    servers.iter().map(|s| (s.id.as_str(), s)).collect()
}
