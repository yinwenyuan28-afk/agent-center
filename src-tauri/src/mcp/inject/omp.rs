//! Oh My Pi（OMP）的 MCP 注入器：`~/.omp/<profile>/agent/mcp.json`。
//!
//! 结构同 Pi（`type` / `enabled` / 各自有字段），但**停用要同时做两件事**：
//!
//! 1. 条目里写 `enabled: false`
//! 2. 把 id 写进文件顶层的 `disabledServers` 数组，并从 `enabledServers` 里移除
//!
//! 只写 `enabled: false` 不够 —— OMP 会从项目级目录发现同名 server 并把它
//! 重新激活。`disabledServers` 是用户的拒绝名单，优先级最高。
//!
//! 本机未装，实现按 CC Switch 的 `mcp/omp.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// OMP 的数据目录。支持 `OMP_HOME` 覆盖。
pub fn data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("OMP_HOME") {
        let v = v.trim();
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    crate::providers::store::home_dir().join(".omp")
}

/// OMP 的 mcp.json 路径。
///
/// OMP 可以有多套 profile（`~/.omp/<profile>/agent/`），默认用 `default`。
pub fn config_path() -> PathBuf {
    data_dir().join("default").join("agent").join("mcp.json")
}

/// 连接字段。写入时先删这些。
const TRANSPORT_FIELDS: &[&str] = &["type", "command", "args", "cwd", "env", "url", "headers"];

/// Oh My Pi 注入器。
pub struct OmpMcpInjector;

/// 转成 OMP 条目。连接字段之外不动，`transport` 由 OMP 从 command/url 推断。
fn to_omp_entry(s: &McpServer) -> Map<String, Value> {
    super::shared::transport_entry(s)
}

/// 维护一个字符串数组字段里的 id 是否在列。
fn update_list(doc: &mut Value, field: &str, id: &str, include: bool) -> anyhow::Result<()> {
    if doc.get(field).is_none() && !include {
        return Ok(());
    }
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("文档顶层不是对象"))?;
    if !obj.contains_key(field) {
        obj.insert(field.into(), serde_json::json!([]));
    }
    let list = obj
        .get_mut(field)
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("{field} 不是数组"))?;
    list.retain(|v| v.as_str() != Some(id));
    if include {
        list.push(Value::String(id.into()));
    }
    Ok(())
}

impl McpInjector for OmpMcpInjector {
    fn agent(&self) -> &'static str {
        "omp"
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

        // 先把 mcpServers 建好，再拿 map。中间要动顶层别的数组，
        // 所以分两次取借用
        {
            let _ = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;
        }

        let mut summary = Vec::new();
        let mut changed = false;

        // 取消分配的：enabled=false + 进 disabledServers
        let unassigned: Vec<&McpServer> = servers
            .iter()
            .filter(|s| !s.agents.iter().any(|a| a == "omp"))
            .collect();
        for s in &unassigned {
            let has_entry = root["mcpServers"].get(&s.id).is_some();
            if has_entry {
                if let Some(entry) = root["mcpServers"]
                    .get_mut(&s.id)
                    .and_then(|v| v.as_object_mut())
                {
                    if entry.get("enabled") != Some(&Value::Bool(false)) {
                        entry.insert("enabled".into(), Value::Bool(false));
                        changed = true;
                    }
                }
            }
            let before = root["disabledServers"].clone();
            update_list(&mut root, "disabledServers", &s.id, true)?;
            update_list(&mut root, "enabledServers", &s.id, false)?;
            if root["disabledServers"] != before {
                summary.push(format!(
                    "停用 {}（未分配给 omp，写入 disabledServers）",
                    s.display_name()
                ));
                changed = true;
            }
        }

        // 已分配的
        let assigned: Vec<&McpServer> = super::shared::wanted(servers, "omp");
        for s in &assigned {
            let conn = to_omp_entry(s);
            let is_new = root["mcpServers"].get(&s.id).is_none();
            let mut entry = root["mcpServers"]
                .get(&s.id)
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let before = entry.clone();

            for f in TRANSPORT_FIELDS {
                entry.remove(*f);
            }
            for (k, v) in &conn {
                entry.insert(k.clone(), v.clone());
            }
            entry.insert("enabled".into(), Value::Bool(true));

            if entry != before {
                root["mcpServers"][&s.id] = Value::Object(entry);
                summary.push(if is_new {
                    format!("新增 {}", s.display_name())
                } else {
                    format!("更新 {}", s.display_name())
                });
                changed = true;
            } else {
                summary.push(format!("{} 已一致", s.display_name()));
            }

            // 从拒绝名单里摘掉
            update_list(&mut root, "disabledServers", &s.id, false)?;
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
            agent: "omp".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }])
    }
}
