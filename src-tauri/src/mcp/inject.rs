//! MCP 注入器。
//!
//! 七个 agent 分三类实现：
//!
//! 1. **同构组**（Claude / pi / Qoder / Cursor）：都是 `{"mcpServers": {id: {...}}}`，
//!    用同一个通用实现 [`McpServersMapInjector`]
//! 2. **TOML 组**（Codex）：`[mcp_servers.<id>]` 表，用 toml_edit 保留注释
//! 3. **特殊组**（opencode 的数组 command + enabled、WorkBuddy 的 disabled + runtime）
//!
//! 所有注入器都遵守同一条原则：**只增改母版声明的 server，绝不删未声明的**。

use super::model::McpServer;
use crate::providers::safe_write::{atomic_write, ensure_first_write_backup, hash_file};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// 一次注入要写的文件计划。
#[derive(Debug, Clone)]
pub struct McpFilePlan {
    pub path: PathBuf,
    pub agent: String,
    pub content: String,
    pub pre_hash: String,
    pub changed: bool,
    pub summary: Vec<String>,
}

/// 注入器统一接口。
pub trait McpInjector {
    fn agent(&self) -> &'static str;
    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>>;
}

/// 判断母版里的某个 server 是否分配给某个 agent。
///
/// 分配关系存在 `agents` 字段里。**没列出的 agent 就是「不该有这个 server」**，
/// 注入时要把它从那个 agent 的配置里删掉。
pub fn assigned_to(s: &McpServer, agent: &str) -> bool {
    s.agents.iter().any(|a| a == agent)
}

/// 从某个 agent 的 JSON map 里移除所有「池子里有、但没分配给这个 agent」的 server。
///
/// 这是分配矩阵语义的核心：勾选 = 写入，取消勾选 = 移除。
///
/// **只删池子里存在的 id**（`servers` 覆盖到的），
/// 池子里没有的（用户手动配的、WorkBuddy 市场连接器）一律不动。
fn prune_unassigned_json(
    map: &mut Map<String, Value>,
    agent: &str,
    servers: &[McpServer],
) -> Vec<String> {
    let to_remove: Vec<String> = servers
        .iter()
        .filter(|s| !assigned_to(s, agent) && map.contains_key(&s.id))
        .map(|s| s.id.clone())
        .collect();

    for id in &to_remove {
        map.remove(id);
    }
    to_remove
}

/// 从某个 agent 的 TOML 表里移除所有「池子里有、但没分配给这个 agent」的 server。
fn prune_unassigned_toml(
    tbl: &mut toml_edit::Table,
    agent: &str,
    servers: &[McpServer],
) -> Vec<String> {
    let to_remove: Vec<String> = servers
        .iter()
        .filter(|s| !assigned_to(s, agent) && tbl.contains_key(&s.id))
        .map(|s| s.id.clone())
        .collect();

    for id in &to_remove {
        tbl.remove(id);
    }
    to_remove
}

/// 把一个 server 转成标准的 `mcpServers` 条目（Claude / pi / Qoder / Cursor 格式）。
pub fn to_standard_entry(s: &McpServer) -> Value {
    let mut obj = Map::new();

    obj.insert("type".into(), Value::String(s.transport.as_str().to_string()));

    if s.is_remote() {
        obj.insert("url".into(), Value::String(s.url.clone()));
        if !s.headers.is_empty() {
            let mut h = Map::new();
            for (k, v) in &s.headers {
                h.insert(k.clone(), Value::String(v.clone()));
            }
            obj.insert("headers".into(), Value::Object(h));
        }
    } else {
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

    Value::Object(obj)
}

/// 标准的 `mcpServers` map 注入器。
///
/// 用同一个实现覆盖 Claude / pi / Qoder / Cursor —— 它们的结构完全一致，
/// 差别只在文件路径。
pub struct McpServersMapInjector {
    pub agent: &'static str,
    pub path: PathBuf,
}

impl McpInjector for McpServersMapInjector {
    fn agent(&self) -> &'static str {
        self.agent
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        // 池子里没有任何 server 时不做任何事（避免把空池子理解成"全删"）
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let orig = if self.path.exists() {
            std::fs::read_to_string(&self.path)?
        } else {
            "{}".to_string()
        };
        let pre = hash_file(&self.path)?;
        let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                self.path.display(),
                e
            )
        })?;

        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", self.path.display()))?;

        if !root_obj.contains_key("mcpServers") {
            root_obj.insert("mcpServers".into(), Value::Object(Map::new()));
        }
        let map = root_obj
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcpServers 不是对象", self.path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 第一步：先移除未分配的（分配矩阵语义）
        let removed = prune_unassigned_json(map, self.agent, servers);
        for id in &removed {
            summary.push(format!("移除 {id}（未分配给 {}）", self.agent));
            changed = true;
        }

        // 第二步：写入已分配的
        for s in servers.iter().filter(|s| assigned_to(s, self.agent)) {
            let new_entry = to_standard_entry(s);

            match map.get(&s.id) {
                Some(existing) => {
                    let same = serde_json::to_string(existing).ok()
                        == serde_json::to_string(&new_entry).ok();
                    if !same {
                        map.insert(s.id.clone(), new_entry);
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                    }
                }
                None => {
                    map.insert(s.id.clone(), new_entry);
                    summary.push(format!("新增 {}", s.display_name()));
                    changed = true;
                }
            }
        }

        // 如果一个 agent 没有分配任何 server，且文件原来不存在，就不写空文件
        if !self.path.exists() && !changed {
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
            path: self.path.clone(),
            agent: self.agent.to_string(),
            content,
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// 构造 Claude 的注入器（`~/.claude.json`）。
pub fn claude() -> Box<dyn McpInjector> {
    Box::new(McpServersMapInjector {
        agent: "claude",
        path: crate::providers::store::home_dir().join(".claude.json"),
    })
}

/// 直接算某个 `mcpServers` 型文件（Claude / pi / Qoder / Cursor）的注入计划。
///
/// 这是给 `agents/<id>.rs` 用的便捷入口 —— agent 实现里不用再包一层
/// `McpServersMapInjector`，直接传路径和 agent 名就行。
pub fn plan_servers_map(
    path: &std::path::PathBuf,
    agent: &'static str,
    servers: &[McpServer],
) -> anyhow::Result<Vec<McpFilePlan>> {
    let inj = McpServersMapInjector {
        agent,
        path: path.clone(),
    };
    inj.plan(servers)
}

/// 构造 Cursor 的注入器。
pub fn cursor() -> Box<dyn McpInjector> {
    Box::new(McpServersMapInjector {
        agent: "cursor",
        path: crate::providers::store::home_dir().join(".cursor").join("mcp.json"),
    })
}

/// 构造 pi 的注入器。
pub fn pi() -> Box<dyn McpInjector> {
    Box::new(McpServersMapInjector {
        agent: "pi",
        path: crate::providers::store::home_dir()
            .join(".pi")
            .join("agent")
            .join("mcp.json"),
    })
}

/// 构造 Qoder 的注入器。
pub fn qoder() -> Box<dyn McpInjector> {
    Box::new(McpServersMapInjector {
        agent: "qoder",
        path: crate::providers::store::home_dir()
            .join(".qoder")
            .join("settings.json"),
    })
}

/// opencode 注入器。
///
/// 两个特殊点：
/// - `command` 是**数组**（命令 + 参数合并）
/// - 用 `enabled` 而不是 `disabled`
pub struct OpencodeMcpInjector;

impl McpInjector for OpencodeMcpInjector {
    fn agent(&self) -> &'static str {
        "opencode"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = crate::providers::store::home_dir()
            .join(".config")
            .join("opencode")
            .join("opencode.json");

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{}".to_string()
        };
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
            anyhow::anyhow!("{} 解析失败（{}），已中止", path.display(), e)
        })?;

        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

        if !root_obj.contains_key("mcp") {
            root_obj.insert("mcp".into(), Value::Object(Map::new()));
        }
        let map = root_obj
            .get_mut("mcp")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp 不是对象", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的
        let removed = prune_unassigned_json(map, "opencode", servers);
        for id in &removed {
            summary.push(format!("移除 {id}（未分配给 opencode）"));
            changed = true;
        }

        // 再写入已分配的
        for s in servers.iter().filter(|s| assigned_to(s, "opencode")) {
            let mut entry = Map::new();

            if s.is_remote() {
                entry.insert("type".into(), Value::String("remote".into()));
                entry.insert("url".into(), Value::String(s.url.clone()));
                if !s.headers.is_empty() {
                    let mut h = Map::new();
                    for (k, v) in &s.headers {
                        h.insert(k.clone(), Value::String(v.clone()));
                    }
                    entry.insert("headers".into(), Value::Object(h));
                }
            } else {
                entry.insert("type".into(), Value::String("local".into()));
                let mut cmd: Vec<Value> = vec![Value::String(s.command.clone())];
                for a in &s.args {
                    cmd.push(Value::String(a.clone()));
                }
                entry.insert("command".into(), Value::Array(cmd));

                if !s.cwd.is_empty() {
                    entry.insert("cwd".into(), Value::String(s.cwd.clone()));
                }
                if !s.env.is_empty() {
                    let mut e = Map::new();
                    for (k, v) in &s.env {
                        e.insert(k.clone(), Value::String(v.clone()));
                    }
                    entry.insert("environment".into(), Value::Object(e));
                }
            }
            entry.insert("enabled".into(), Value::Bool(s.enabled));

            let new_val = Value::Object(entry);
            let same = map
                .get(&s.id)
                .and_then(|v| serde_json::to_string(v).ok())
                == serde_json::to_string(&new_val).ok();

            if !same {
                let is_new = !map.contains_key(&s.id);
                map.insert(s.id.clone(), new_val);
                summary.push(format!(
                    "{} {}",
                    if is_new { "新增" } else { "更新" },
                    s.display_name()
                ));
                changed = true;
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
            agent: "opencode".to_string(),
            content,
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// WorkBuddy 注入器。
///
/// 特殊点：
/// - 用 `disabled`（与 enabled 相反）
/// - 有 `runtime` / `staticEnv` / `disabledTools` 等特有字段，更新时不能丢
/// - 已有条目只改关键字段，保留 WorkBuddy 自己的元数据
pub struct WorkBuddyMcpInjector;

impl McpInjector for WorkBuddyMcpInjector {
    fn agent(&self) -> &'static str {
        "workbuddy"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = crate::providers::store::home_dir()
            .join(".workbuddy")
            .join("connectors")
            .join("default")
            .join("mcp.json");

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{\"mcpServers\":{}}".to_string()
        };
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
            anyhow::anyhow!("{} 解析失败（{}），已中止", path.display(), e)
        })?;

        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

        if !root_obj.contains_key("mcpServers") {
            root_obj.insert("mcpServers".into(), Value::Object(Map::new()));
        }
        let map = root_obj
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcpServers 不是对象", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的。注意这里只删池子里的 id，
        // 83 个 connector: 开头的市场连接器不在池子里，不会被碰。
        let removed = prune_unassigned_json(map, "workbuddy", servers);
        for id in &removed {
            summary.push(format!("移除 {id}（未分配给 workbuddy）"));
            changed = true;
        }

        for s in servers.iter().filter(|s| assigned_to(s, "workbuddy")) {
            let exists = map.contains_key(&s.id);

            if exists {
                // 只改关键字段，保留 runtime / staticEnv / disabledTools 等
                if let Some(obj) = map.get_mut(&s.id).and_then(|v| v.as_object_mut()) {
                    let mut local = false;

                    obj.insert("type".into(), Value::String(s.transport.as_str().to_string()));
                    obj.insert("disabled".into(), Value::Bool(!s.enabled));

                    if s.is_remote() {
                        if obj.get("url").and_then(|v| v.as_str()) != Some(&s.url) {
                            obj.insert("url".into(), Value::String(s.url.clone()));
                            local = true;
                        }
                        if !s.headers.is_empty() {
                            let mut h = Map::new();
                            for (k, v) in &s.headers {
                                h.insert(k.clone(), Value::String(v.clone()));
                            }
                            obj.insert("headers".into(), Value::Object(h));
                            local = true;
                        }
                    } else {
                        if obj.get("command").and_then(|v| v.as_str()) != Some(&s.command) {
                            obj.insert("command".into(), Value::String(s.command.clone()));
                            local = true;
                        }
                        let new_args: Vec<Value> =
                            s.args.iter().map(|a| Value::String(a.clone())).collect();
                        let same_args = obj
                            .get("args")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr == &new_args)
                            .unwrap_or(false);
                        if !same_args {
                            obj.insert("args".into(), Value::Array(new_args));
                            local = true;
                        }
                        if !s.env.is_empty() {
                            let mut e = Map::new();
                            for (k, v) in &s.env {
                                e.insert(k.clone(), Value::String(v.clone()));
                            }
                            obj.insert("env".into(), Value::Object(e));
                            local = true;
                        }
                    }

                    if local {
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                    }
                }
            } else {
                // 新增：用标准结构，但用 disabled 而不是 enabled
                let mut entry = to_standard_entry(s);
                if let Some(obj) = entry.as_object_mut() {
                    obj.remove("type");
                    obj.insert("type".into(), Value::String(s.transport.as_str().to_string()));
                    obj.insert("disabled".into(), Value::Bool(!s.enabled));
                }
                map.insert(s.id.clone(), entry);
                summary.push(format!("新增 {}", s.display_name()));
                changed = true;
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
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// Codex 注入器：`~/.codex/config.toml` 的 `[mcp_servers.*]` 表。
///
/// 用 toml_edit 保留注释和格式。
pub struct CodexMcpInjector;

impl McpInjector for CodexMcpInjector {
    fn agent(&self) -> &'static str {
        "codex"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = crate::providers::store::home_dir()
            .join(".codex")
            .join("config.toml");

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let pre = hash_file(&path)?;

        // 检测行尾风格（toml_edit 不保留 CRLF）
        let uses_crlf = orig.contains("\r\n");
        let had_trailing = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = orig
            .parse()
            .map_err(|e| anyhow::anyhow!("{} 解析失败（{}），已中止", path.display(), e))?;

        if !doc.contains_key("mcp_servers") {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let tbl = doc["mcp_servers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp_servers 不是表", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的（分配矩阵语义）
        let removed = prune_unassigned_toml(tbl, "codex", servers);
        for id in &removed {
            summary.push(format!("移除 {id}（未分配给 codex）"));
            changed = true;
        }

        let wanted: Vec<&McpServer> =
            servers.iter().filter(|s| assigned_to(s, "codex")).collect();

        for s in &wanted {
            let exists = tbl.contains_key(&s.id);

            if exists {
                // 只改关键字段，保留 startup_timeout_sec / env_vars 等
                if let Some(t) = tbl.get_mut(&s.id).and_then(|v| v.as_table_mut()) {
                    let mut local = false;

                    let cur_type = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if cur_type != s.transport.as_str() {
                        t["type"] = toml_edit::value(s.transport.as_str());
                        local = true;
                    }

                    if s.is_remote() {
                        if t.get("url").and_then(|v| v.as_str()) != Some(&s.url) {
                            t["url"] = toml_edit::value(s.url.clone());
                            local = true;
                        }
                    } else {
                        if t.get("command").and_then(|v| v.as_str()) != Some(&s.command) {
                            t["command"] = toml_edit::value(s.command.clone());
                            local = true;
                        }
                        // args 数组
                        let mut arr = toml_edit::Array::new();
                        for a in &s.args {
                            arr.push(a.clone());
                        }
                        let cur_args_same = t
                            .get("args")
                            .and_then(|v| v.as_array())
                            .map(|cur| {
                                cur.len() == s.args.len()
                                    && cur
                                        .iter()
                                        .zip(s.args.iter())
                                        .all(|(a, b)| a.as_str() == Some(b.as_str()))
                            })
                            .unwrap_or(false);
                        if !cur_args_same {
                            t["args"] = toml_edit::value(arr);
                            local = true;
                        }

                        // env 子表
                        if !s.env.is_empty() {
                            if !t.contains_key("env") {
                                t["env"] = toml_edit::Item::Table(toml_edit::Table::new());
                            }
                            if let Some(env_t) = t.get_mut("env").and_then(|v| v.as_table_mut()) {
                                for (k, v) in &s.env {
                                    if env_t.get(k).and_then(|x| x.as_str()) != Some(v) {
                                        env_t[k] = toml_edit::value(v.clone());
                                        local = true;
                                    }
                                }
                            }
                        }
                    }

                    if local {
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                    }
                }
            } else {
                let mut t = toml_edit::Table::new();
                t["type"] = toml_edit::value(s.transport.as_str());

                if s.is_remote() {
                    t["url"] = toml_edit::value(s.url.clone());
                    if !s.headers.is_empty() {
                        let mut h = toml_edit::Table::new();
                        for (k, v) in &s.headers {
                            h[k] = toml_edit::value(v.clone());
                        }
                        t["headers"] = toml_edit::Item::Table(h);
                    }
                } else {
                    t["command"] = toml_edit::value(s.command.clone());
                    let mut arr = toml_edit::Array::new();
                    for a in &s.args {
                        arr.push(a.clone());
                    }
                    if !arr.is_empty() {
                        t["args"] = toml_edit::value(arr);
                    }
                    if !s.env.is_empty() {
                        let mut e = toml_edit::Table::new();
                        for (k, v) in &s.env {
                            e[k] = toml_edit::value(v.clone());
                        }
                        t["env"] = toml_edit::Item::Table(e);
                    }
                }

                tbl.insert(&s.id, toml_edit::Item::Table(t));
                summary.push(format!("新增 {}", s.display_name()));
                changed = true;
            }
        }

        let mut content = doc.to_string();
        if uses_crlf {
            content = content.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if !had_trailing {
            while content.ends_with('\n') || content.ends_with("\r\n") {
                if content.ends_with("\r\n") {
                    content.truncate(content.len() - 2);
                } else {
                    content.truncate(content.len() - 1);
                }
            }
        }

        Ok(vec![McpFilePlan {
            path,
            agent: "codex".to_string(),
            content: if changed { content } else { orig },
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// 直接算 Codex 的 MCP 注入计划（给 `agents/codex.rs` 用）。
pub fn plan_codex(servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(&CodexMcpInjector, servers)
}

/// 直接算 opencode 的 MCP 注入计划。
pub fn plan_opencode(servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(&OpencodeMcpInjector, servers)
}

/// 直接算 WorkBuddy 的 MCP 注入计划。
pub fn plan_workbuddy(servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(&WorkBuddyMcpInjector, servers)
}

/// ZCode MCP 注入器。
///
/// 目标是 `~/.zcode/cli/config.json` 的 **`mcp.servers`**（嵌套两层）。
///
/// 注意几处和别家不同的地方（来自 ZCode 官方文档）：
/// - 键路径是 `mcp.servers.<name>`，不是顶层 `mcpServers`
/// - 文件 schema 是**严格模式**：多一个未知键，整个 server 会被丢弃
/// - `command` 必须是字符串，`args` 必须是数组（不能学 opencode 写数组）
/// - `type` 省略时会按 `command`/`url` 推断，但显式写更稳
pub struct ZCodeMcpInjector;

impl McpInjector for ZCodeMcpInjector {
    fn agent(&self) -> &'static str {
        "zcode"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = crate::providers::store::home_dir()
            .join(".zcode")
            .join("cli")
            .join("config.json");

        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{}".to_string()
        };
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&orig)
            .map_err(|e| anyhow::anyhow!("{} 解析失败（{}），已中止", path.display(), e))?;

        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

        // mcp.servers 两层都要在
        if !root_obj.contains_key("mcp") {
            root_obj.insert("mcp".into(), Value::Object(Map::new()));
        }
        let mcp_obj = root_obj
            .get_mut("mcp")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp 不是对象", path.display()))?;
        if !mcp_obj.contains_key("servers") {
            mcp_obj.insert("servers".into(), Value::Object(Map::new()));
        }
        let map = mcp_obj
            .get_mut("servers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp.servers 不是对象", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的（只删池子里的 id）
        let removed = prune_unassigned_json(map, "zcode", servers);
        for id in &removed {
            summary.push(format!("移除 {id}（未分配给 zcode）"));
            changed = true;
        }

        // 再写入已分配的。用标准 entry，但按 ZCode 的严格 schema 收敛字段
        for s in servers.iter().filter(|s| assigned_to(s, "zcode")) {
            let mut entry = to_standard_entry(s);
            if let Some(obj) = entry.as_object_mut() {
                // type 显式写，避免推断差异
                obj.insert(
                    "type".into(),
                    Value::String(s.transport.as_str().to_string()),
                );
                // enabled：ZCode 支持这个字段
                obj.insert("enabled".into(), Value::Bool(s.enabled));
            }

            let same = map
                .get(&s.id)
                .and_then(|v| serde_json::to_string(v).ok())
                == serde_json::to_string(&entry).ok();

            if !same {
                let is_new = !map.contains_key(&s.id);
                map.insert(s.id.clone(), entry);
                summary.push(format!(
                    "{} {}",
                    if is_new { "新增" } else { "更新" },
                    s.display_name()
                ));
                changed = true;
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
            agent: "zcode".to_string(),
            content,
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// Hermes MCP 注入器。
///
/// 目标是 `$HERMES_HOME/config.yaml` 的 **`mcp_servers`** 段（顶层）。
/// 结构（来自 Hermes 自带的 native-mcp 文档）：
///
/// ```yaml
/// mcp_servers:
///   time:
///     command: uvx
///     args: ["mcp-server-time"]
///     env:
///       KEY: value
///     timeout: 120
///     connect_timeout: 60
/// ```
///
/// 和 provider 一样做行级 YAML 编辑 —— `config.yaml` 里有大段注释和
/// `skills.disabled` 长数组，用 YAML 库整体重写会全部丢失。
pub struct HermesMcpInjector;

impl HermesMcpInjector {
    fn config_path() -> PathBuf {
        crate::providers::hermes::config_path()
    }
}

/// 生成一个 mcp server 的 YAML 片段（缩进 2 起）。
fn hermes_server_yaml(s: &McpServer) -> String {
    let mut out = String::new();
    out.push_str(&format!("  {}:\n", s.id));

    if s.is_remote() {
        out.push_str(&format!("    url: {}\n", s.url));
        if !s.headers.is_empty() {
            out.push_str("    headers:\n");
            for (k, v) in &s.headers {
                out.push_str(&format!("      {k}: {v}\n"));
            }
        }
    } else {
        out.push_str(&format!("    command: {}\n", s.command));
        if !s.args.is_empty() {
            let args: Vec<String> = s
                .args
                .iter()
                .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
                .collect();
            out.push_str(&format!("    args: [{}]\n", args.join(", ")));
        }
        if !s.cwd.is_empty() {
            out.push_str(&format!("    cwd: {}\n", s.cwd));
        }
        if !s.env.is_empty() {
            out.push_str("    env:\n");
            for (k, v) in &s.env {
                out.push_str(&format!("      {k}: \"{}\"\n", v.replace('"', "\\\"")));
            }
        }
    }

    // Hermes 的 timeout 是秒。母版没这个概念，用默认值不写
    out
}

impl McpInjector for HermesMcpInjector {
    fn agent(&self) -> &'static str {
        "hermes"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;
        let mut lines: Vec<String> = orig.lines().map(|s| s.to_string()).collect();
        let mut summary = Vec::new();
        let mut changed = false;

        // 找 mcp_servers 段
        let find_block = |lines: &[String], key: &str| -> Option<(usize, usize)> {
            let start = lines
                .iter()
                .position(|l| l.starts_with(&format!("{key}:")))?;
            let mut end = lines.len();
            for (i, l) in lines.iter().enumerate().skip(start + 1) {
                if l.trim().is_empty() {
                    continue;
                }
                let indent = l.len() - l.trim_start().len();
                if indent == 0 {
                    end = i;
                    break;
                }
            }
            Some((start, end))
        };

        // 先移除未分配的
        for s in servers.iter().filter(|s| !assigned_to(s, "hermes")) {
            if let Some((start, end)) = find_block(&lines, "mcp_servers") {
                let key_line = (start + 1..end).find(|&i| {
                    let l = &lines[i];
                    l.len() - l.trim_start().len() == 2
                        && l.trim_start().starts_with(&format!("{}:", s.id))
                });
                if let Some(kl) = key_line {
                    let mut stop = end;
                    for i in kl + 1..end {
                        if lines[i].trim().is_empty() {
                            continue;
                        }
                        if lines[i].len() - lines[i].trim_start().len() <= 2 {
                            stop = i;
                            break;
                        }
                    }
                    lines.drain(kl..stop);
                    summary.push(format!("移除 {}（未分配给 hermes）", s.id));
                    changed = true;
                }
            }
        }

        // 再写入已分配的
        for s in servers.iter().filter(|s| assigned_to(s, "hermes")) {
            match find_block(&lines, "mcp_servers") {
                Some((start, end)) => {
                    let exists = (start + 1..end).any(|i| {
                        let l = &lines[i];
                        l.len() - l.trim_start().len() == 2
                            && l.trim_start().starts_with(&format!("{}:", s.id))
                    });
                    if exists {
                        summary.push(format!(
                            "{} 已存在，跳过（如需改字段请手工编辑 config.yaml）",
                            s.display_name()
                        ));
                    } else {
                        let snippet = hermes_server_yaml(s);
                        for (off, line) in snippet.lines().enumerate() {
                            lines.insert(end + off, line.to_string());
                        }
                        summary.push(format!("新增 {}", s.display_name()));
                        changed = true;
                    }
                }
                None => {
                    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push("mcp_servers:".to_string());
                    for line in hermes_server_yaml(s).lines() {
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
            pre_hash: pre,
            changed,
            summary,
        }])
    }
}

/// 直接算 ZCode 的 MCP 注入计划。
pub fn plan_zcode(servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(&ZCodeMcpInjector, servers)
}

/// 直接算 Hermes 的 MCP 注入计划。
pub fn plan_hermes(servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(&HermesMcpInjector, servers)
}

/// 返回全部注入器。
///
/// 现在各 agent 的 MCP 注入逻辑由 `agents/<id>.rs` 的
/// [`crate::agents::Agent::mcp_plan`] 提供，这个函数只为兼容
/// 旧的直调场景保留。
pub fn all_injectors() -> Vec<Box<dyn McpInjector>> {
    vec![
        claude(),
        cursor(),
        pi(),
        qoder(),
        Box::new(CodexMcpInjector),
        Box::new(OpencodeMcpInjector),
        Box::new(WorkBuddyMcpInjector),
    ]
}

/// 执行注入：先全部 plan，任一失败就中止，全部成功才写。
///
/// 遍历 `agents::all()` 里声明了 MCP 能力的那些，各算各的计划。
pub fn sync(servers: &[McpServer], dry_run: bool) -> anyhow::Result<super::model::McpSyncReport> {
    let mut report = super::model::McpSyncReport {
        ok: true,
        written: Vec::new(),
        skipped: Vec::new(),
        backups: Vec::new(),
        log: Vec::new(),
    };

    // 第一遍：全部规划
    let mut all_plans: Vec<(String, Vec<McpFilePlan>)> = Vec::new();
    for agent in crate::agents::mcp_capable() {
        let id = agent.id().to_string();
        // 只把分配给这个 agent 的 server 传进去，但注入器需要看到
        // 池子全貌才能判断哪些该移除 —— 所以传全量
        let refs: Vec<&McpServer> = servers.iter().collect();
        match agent.mcp_plan(&refs) {
            Ok(plans) => {
                if !plans.is_empty() {
                    all_plans.push((id, plans));
                }
            }
            Err(e) => {
                // 任何一个 agent 规划失败，整次同步中止
                anyhow::bail!("{} 规划失败，已中止（未写入任何文件）：{e}", id);
            }
        }
    }

    // 第二遍：执行
    for (agent, plans) in &all_plans {
        for plan in plans {
            if !plan.changed {
                report.skipped.push(plan.path.display().to_string());
                continue;
            }

            if dry_run {
                report.written.push(plan.path.display().to_string());
                report
                    .log
                    .push(format!("[预览] {agent}: 将写入 {}", plan.path.display()));
                for s in &plan.summary {
                    report.log.push(format!("        {s}"));
                }
                continue;
            }

            // hash 复查
            let current = hash_file(&plan.path)?;
            if current != plan.pre_hash {
                report.ok = false;
                report.log.push(format!(
                    "[冲突] {agent}: {} 在计划后被修改，已中止",
                    plan.path.display()
                ));
                return Ok(report);
            }

            // 首写备份
            if let Ok(Some(b)) = ensure_first_write_backup(&plan.path) {
                report.backups.push(b.display().to_string());
            }

            match atomic_write(&plan.path, plan.content.as_bytes()) {
                Ok(()) => {
                    report.written.push(plan.path.display().to_string());
                    report
                        .log
                        .push(format!("[写入] {agent}: {}", plan.path.display()));
                    for s in &plan.summary {
                        report.log.push(format!("        {s}"));
                    }
                }
                Err(e) => {
                    report.ok = false;
                    report.log.push(format!("[失败] {agent}: {e}"));
                    return Ok(report);
                }
            }
        }
    }

    Ok(report)
}
