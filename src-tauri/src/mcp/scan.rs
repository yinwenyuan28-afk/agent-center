//! 从各 agent 现有配置里读出 MCP server，用于和母版比对。
//!
//! **全部只读**。各家的结构差异在 `plan/mcp-management.md` 里有完整表格，
//! 这里只做解析和归一化。
//!
//! 一个工具对应一个扫描函数，和 `inject` 目录一一对应。

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
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// 把 JSON 里的一个 server 对象解析成 [`FoundServer`]。
///
/// 兼容的变体：
/// - `command` 可能是字符串（Claude/pi/Qoder/Codex）或数组（opencode）
/// - `type` 各家写法不同（local/remote、streamableHttp、sse）
/// - 启用状态有的是 `enabled`，有的是 `disabled`（取反）
/// - Gemini 的 HTTP 用 `httpUrl` 而不是 `url`
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

    // url：Gemini 的 HTTP 叫 httpUrl，Antigravity 的旧写法叫 serverUrl
    let mut url = get_str("url");
    if url.is_empty() {
        url = get_str("httpUrl");
    }
    if url.is_empty() {
        url = get_str("serverUrl");
    }

    // transport：type 优先，其次 transport（Kimi 的 SSE 写法），最后按字段推断
    let raw_type = {
        let t = get_str("type");
        if !t.is_empty() {
            t
        } else {
            get_str("transport")
        }
    };
    let transport = if !raw_type.is_empty() {
        Transport::parse(&raw_type)
    } else if !url.is_empty() {
        // 没有 type 但有 url：默认当 http（sse 已过时）
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

/// 通用的 `mcpServers` map 扫描。
fn scan_map(path: &PathBuf, agent: &str, label: &str) -> McpAgentScan {
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
///
/// 同一份文件里 `projects.*.mcpServers` 是项目级的，这里只读顶层。
pub fn scan_claude() -> McpAgentScan {
    scan_map(&home().join(".claude.json"), "claude", "~/.claude.json")
}

/// Cursor：`~/.cursor/mcp.json`。
pub fn scan_cursor() -> McpAgentScan {
    scan_map(
        &home().join(".cursor").join("mcp.json"),
        "cursor",
        "~/.cursor/mcp.json",
    )
}

/// Qoder：`~/.qoder/settings.json` 的 `mcpServers`。
pub fn scan_qoder() -> McpAgentScan {
    scan_map(
        &home().join(".qoder").join("settings.json"),
        "qoder",
        "~/.qoder/settings.json",
    )
}

/// Pi：`~/.pi/agent/mcp.json`。
pub fn scan_pi() -> McpAgentScan {
    scan_map(
        &home().join(".pi").join("agent").join("mcp.json"),
        "pi",
        "~/.pi/agent/mcp.json",
    )
}

/// WorkBuddy：`~/.workbuddy/connectors/default/mcp.json`。
///
/// 这个文件有 83 个 connector，全是内置市场目录（`disabled: true`）。
/// 全读进来但标记为未启用，让 diff 能区分「市场目录」和「用户主动启用的」。
pub fn scan_workbuddy() -> McpAgentScan {
    scan_map(
        &home()
            .join(".workbuddy")
            .join("connectors")
            .join("default")
            .join("mcp.json"),
        "workbuddy",
        "~/.workbuddy/connectors/default/mcp.json",
    )
}

/// Kimi Code：`~/.kimi-code/mcp.json`。
pub fn scan_kimi() -> McpAgentScan {
    scan_map(
        &crate::mcp::inject::kimi::config_path(),
        "kimi",
        "~/.kimi-code/mcp.json",
    )
}

/// MiniMax Code：`~/.minimax/mcp.json`。
pub fn scan_minimax() -> McpAgentScan {
    scan_map(
        &crate::mcp::inject::minimax::config_path(),
        "minimax",
        "~/.minimax/mcp.json",
    )
}

/// GitHub Copilot CLI：`~/.copilot/mcp-config.json`。
///
/// 禁用在 `settings.json` 的 `disabledMcpServers` 数组里，
/// 不在条目本身。
pub fn scan_copilot() -> McpAgentScan {
    let path = crate::mcp::inject::copilot::config_path();
    let mut scan = scan_map(&path, "copilot", "~/.copilot/mcp-config.json");

    // 把 disabledMcpServers 里的标成未启用
    let settings = crate::mcp::inject::copilot::settings_path();
    if let Some(json) = read_json(&settings) {
        let disabled: Vec<String> = json
            .get("disabledMcpServers")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for s in &mut scan.servers {
            if disabled.contains(&s.id) {
                s.enabled = false;
            }
        }
    }
    scan
}

/// Antigravity：`~/.gemini/config/mcp_config.json`。
pub fn scan_agy() -> McpAgentScan {
    scan_map(
        &crate::mcp::inject::agy::config_path(),
        "agy",
        "~/.gemini/config/mcp_config.json",
    )
}

/// Oh My Pi：`~/.omp/default/agent/mcp.json`。
///
/// 除了条目自己的 `enabled`，还要看顶层 `disabledServers` 有没有它。
pub fn scan_omp() -> McpAgentScan {
    let path = crate::mcp::inject::omp::config_path();
    let mut scan = scan_map(&path, "omp", "~/.omp/default/agent/mcp.json");

    if let Some(json) = read_json(&path) {
        let disabled: Vec<String> = json
            .get("disabledServers")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for s in &mut scan.servers {
            if disabled.contains(&s.id) {
                s.enabled = false;
            }
        }
    }
    scan
}

/// Codex：`~/.codex/config.toml` 的 `[mcp_servers.*]`。
///
/// 读两种格式：标准 `[mcp_servers]` 和历史错误的 `[mcp.servers]`。
pub fn scan_codex() -> McpAgentScan {
    let path = home().join(".codex").join("config.toml");
    let present = path.exists();
    let mut servers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(doc) = text.parse::<toml_edit::DocumentMut>() {
            // 标准位置优先，再读历史错误位置
            let mut tables: Vec<(&str, &toml_edit::Table)> = Vec::new();
            if let Some(t) = doc.get("mcp_servers").and_then(|v| v.as_table()) {
                tables.push(("mcp_servers", t));
            }
            if let Some(t) = doc
                .get("mcp")
                .and_then(|v| v.get("servers"))
                .and_then(|v| v.as_table())
            {
                tables.push(("mcp.servers", t));
            }

            for (prefix, tbl) in tables {
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

                    // Codex 不写 type；有 url 就是 streamable HTTP
                    let url = get_str("url");
                    let raw_type = get_str("type");
                    let transport = if !raw_type.is_empty() {
                        Transport::parse(&raw_type)
                    } else if !url.is_empty() {
                        Transport::Http
                    } else {
                        Transport::Stdio
                    };

                    // 已存在同名时标准位置优先
                    if servers.iter().any(|s: &FoundServer| s.id == *id) {
                        continue;
                    }

                    servers.push(FoundServer {
                        id: id.to_string(),
                        name: {
                            let n = get_str("name");
                            if n.is_empty() {
                                id.to_string()
                            } else {
                                n
                            }
                        },
                        transport: transport.as_str().to_string(),
                        command: get_str("command"),
                        args,
                        url,
                        enabled: true,
                        location: format!("config.toml · {prefix}.{id}"),
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

/// opencode：`~/.config/opencode/` 下三份文件合并后的 `mcp` 段。
///
/// **必须合并读**：`config.json` → `opencode.json` → `opencode.jsonc`
/// 依次覆盖。只看一份会漏 —— 本机 `.jsonc` 里有 provider、
/// `.json` 里有 mcp，两份都得读。
pub fn scan_opencode() -> McpAgentScan {
    let dir = crate::mcp::inject::opencode::config_dir();
    let present = crate::mcp::inject::opencode::CONFIG_FILES
        .iter()
        .any(|name| dir.join(name).exists());
    let mut servers = Vec::new();

    if let Ok(merged) = crate::mcp::inject::opencode::read_merged() {
        if let Some(map) = merged.get("mcp").and_then(|v| v.as_object()) {
            for (id, v) in map {
                servers.push(parse_server(id, v, &format!("opencode · mcp.{id}")));
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

/// Grok Build：`~/.grok/config.toml`，结构同 Codex。
pub fn scan_grok() -> McpAgentScan {
    let path = crate::mcp::inject::grok::config_path();
    let mut scan = scan_toml_mcp(&path, "grok", "~/.grok/config.toml");
    // Grok 的请求头字段是 headers，不是 http_headers；其余同 Codex
    scan.agent = "grok".into();
    scan
}

/// 读一个 `[mcp_servers.*]` 形状的 TOML 文件。
fn scan_toml_mcp(path: &PathBuf, agent: &str, label: &str) -> McpAgentScan {
    let present = path.exists();
    let mut servers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(path) {
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
                    let url = get_str("url");
                    let raw_type = get_str("type");
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
                            if n.is_empty() {
                                id.to_string()
                            } else {
                                n
                            }
                        },
                        transport: transport.as_str().to_string(),
                        command: get_str("command"),
                        args,
                        url,
                        enabled: true,
                        location: format!("{label} · mcp_servers.{id}"),
                    });
                }
            }
        }
    }

    McpAgentScan {
        agent: agent.into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// Gemini CLI：`~/.gemini/settings.json` 顶层 `mcpServers`。
///
/// 没有 `type`；HTTP 用 `httpUrl`、SSE 用 `url`。`parse_server` 都认。
pub fn scan_gemini() -> McpAgentScan {
    scan_map(
        &crate::mcp::inject::gemini::config_path(),
        "gemini",
        "~/.gemini/settings.json",
    )
}

/// ZCode：`~/.zcode/cli/config.json` 的 `mcp.servers`（嵌套两层）。
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

/// Hermes：`$HERMES_HOME/config.yaml` 的 `mcp_servers` 段。
///
/// 行级解析，不引 YAML 库 —— config.yaml 里有大段注释和长数组。
pub fn scan_hermes() -> McpAgentScan {
    let path = crate::mcp::inject::hermes::config_path();
    let present = path.exists();
    let mut servers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(start) = lines.iter().position(|l| l.starts_with("mcp_servers:")) {
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

            let mut cur: Option<FoundServer> = None;
            let mut in_sub = false;

            for line in &lines[start + 1..end] {
                if line.trim().is_empty() || line.trim_start().starts_with('#') {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                let t = line.trim();

                if indent == 2 && t.ends_with(':') {
                    if let Some(s) = cur.take() {
                        servers.push(s);
                    }
                    let id = t.trim_end_matches(':').to_string();
                    cur = Some(FoundServer {
                        id: id.clone(),
                        name: id.clone(),
                        transport: "stdio".to_string(),
                        command: String::new(),
                        args: Vec::new(),
                        url: String::new(),
                        enabled: true,
                        location: format!("config.yaml · mcp_servers.{id}"),
                    });
                    in_sub = false;
                    continue;
                }

                if let Some(s) = cur.as_mut() {
                    if indent == 4 {
                        in_sub = t.ends_with(':');
                        if let Some((k, v)) = t.split_once(':') {
                            let k = k.trim();
                            let v = v.trim();
                            match k {
                                "command" => s.command = v.to_string(),
                                "url" => {
                                    s.url = v.to_string();
                                    s.transport = "http".to_string();
                                }
                                "enabled" => s.enabled = v != "false",
                                "args" => {
                                    let inner =
                                        v.trim_start_matches('[').trim_end_matches(']');
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
                let _ = in_sub;
            }
            if let Some(s) = cur.take() {
                servers.push(s);
            }
        }
    }

    McpAgentScan {
        agent: "hermes".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// DSH：`~/.dsh/profiles/<profile>/cordis.patch.yml` 里的
/// `name: "@deepseek-ai/dsh-mcp-client"` 条目。
///
/// 多个 profile 之间同名 server 只报一次（第一个读到的算）。
pub fn scan_dsh() -> McpAgentScan {
    let paths = crate::mcp::inject::dsh::target_paths();
    let present = !paths.is_empty();
    let mut servers: Vec<FoundServer> = Vec::new();

    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
            continue;
        };
        let Ok(json) = serde_json::to_value(&yaml) else {
            continue;
        };
        let Some(patches) = json.as_array() else {
            continue;
        };

        for patch in patches {
            let mut candidates: Vec<&Value> = Vec::new();
            if patch.get("name").and_then(|v| v.as_str()) == Some("@deepseek-ai/dsh-mcp-client") {
                candidates.push(patch);
            }
            if let Some(list) = patch.get("insert").and_then(|v| v.as_array()) {
                for entry in list {
                    if entry.get("name").and_then(|v| v.as_str())
                        == Some("@deepseek-ai/dsh-mcp-client")
                    {
                        candidates.push(entry);
                    }
                }
            }

            for entry in candidates {
                let Some(config) = entry.get("config") else {
                    continue;
                };
                let Some(id) = config.get("serverName").and_then(|v| v.as_str()) else {
                    continue;
                };
                if servers.iter().any(|s| s.id == id) {
                    continue;
                }
                let transport = match config.get("transport").and_then(|v| v.as_str()) {
                    Some("streamable-http") => Transport::Http,
                    _ => Transport::Stdio,
                };
                let args = config
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let enabled = entry.get("disabled").and_then(|v| v.as_bool()) != Some(true);
                servers.push(FoundServer {
                    id: id.to_string(),
                    name: id.to_string(),
                    transport: transport.as_str().to_string(),
                    command: config
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    args,
                    url: config
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    enabled,
                    location: format!(
                        "{} · dsh-mcp-client.{id}",
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("patch")
                    ),
                });
            }
        }
    }

    McpAgentScan {
        agent: "dsh".into(),
        present,
        servers,
        unsupported_reason: None,
    }
}

/// 扫描全部支持的 agent。
///
/// 顺序和 `agents::mcp_capable()` 一致，方便 diff 里对照。
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
        scan_dsh(),
        scan_gemini(),
        scan_grok(),
        scan_kimi(),
        scan_minimax(),
        scan_copilot(),
        scan_agy(),
        scan_omp(),
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
