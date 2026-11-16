//! GitHub Copilot CLI 的 MCP 注入器：`~/.copilot/mcp-config.json` 顶层 `mcpServers`。
//!
//! ## 原生格式
//!
//! - stdio 写作 `"type": "local"`（`command` / `args` / `env`），`args` 必须存在
//! - 远程是 `http` 或 `sse`（`url` / `headers`）
//! - 每项都带 `tools` 过滤，默认 `["*"]`
//!
//! ## 停用要额外记东西
//!
//! Copilot 停用一个 server 的方式是**删掉条目**，并在
//! `~/.copilot/settings.json` 的 `disabledMcpServers` 数组里记名字。
//!
//! 删之前要把 `tools`、`timeout` 两个 Copilot 私有字段存到
//! `~/.agents/copilot-mcp-options.json`，重新启用时还回去 ——
//! 否则用户精心裁剪的工具白名单会被放宽成 `["*"]`。
//!
//! 本机 `~/.copilot` 只有 `logs/`，未初始化。实现按 CC Switch 的 `mcp/copilot.rs` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Copilot 的目录。
pub fn copilot_dir() -> PathBuf {
    crate::providers::store::home_dir().join(".copilot")
}

/// `mcp-config.json` 路径。
pub fn config_path() -> PathBuf {
    copilot_dir().join("mcp-config.json")
}

/// `settings.json` 路径（`disabledMcpServers` 在里面）。
pub fn settings_path() -> PathBuf {
    copilot_dir().join("settings.json")
}

/// 停用前记下的私有字段存放在这里。
///
/// 放 `~/.agents/` 而不是 `~/.copilot/`：那是我们的工作目录，
/// 放进去不会让 Copilot 误读。
pub fn options_path() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join("copilot-mcp-options.json")
}

/// 只属于 Copilot 的字段。
const NATIVE_OPTION_FIELDS: &[&str] = &["tools", "timeout"];

/// 连接字段。
const TRANSPORT_FIELDS: &[&str] = &["type", "command", "args", "cwd", "env", "url", "headers"];

/// Copilot 注入器。
pub struct CopilotMcpInjector;

/// 转成 Copilot 条目。stdio 用 `type: "local"` 且 `args` 必须有。
fn to_copilot_entry(s: &McpServer) -> Map<String, Value> {
    let mut obj = super::shared::transport_entry(s);
    match s.transport {
        Transport::Stdio => {
            obj.insert("type".into(), Value::String("local".into()));
            // Copilot 要求 args 存在，空就写 []
            obj.entry("args".to_string()).or_insert_with(|| Value::Array(Vec::new()));
        }
        Transport::Sse => {
            obj.insert("type".into(), Value::String("sse".into()));
        }
        Transport::Http => {
            obj.insert("type".into(), Value::String("http".into()));
        }
    }
    obj
}

/// 读停用前记下的选项。文件不存在当空，坏了就报错。
fn read_options(path: &std::path::Path) -> anyhow::Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = std::fs::read_to_string(path)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(m),
        _ => anyhow::bail!("{} 内容无效", path.display()),
    }
}

impl McpInjector for CopilotMcpInjector {
    fn agent(&self) -> &'static str {
        "copilot"
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
        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &path)?;

        let options_path = options_path();
        let mut options = read_options(&options_path)?;
        let mut options_changed = false;

        let mut summary = Vec::new();
        let mut changed = false;

        // 取消分配的：删条目，但先把 Copilot 自己的字段记下来
        for s in servers.iter().filter(|s| !s.agents.iter().any(|a| a == "copilot")) {
            if let Some(Value::Object(entry)) = map.remove(&s.id) {
                let kept: Map<String, Value> = entry
                    .into_iter()
                    .filter(|(k, _)| NATIVE_OPTION_FIELDS.contains(&k.as_str()))
                    .collect();
                if !kept.is_empty() {
                    options.insert(s.id.clone(), Value::Object(kept));
                    options_changed = true;
                    summary.push(format!(
                        "停用 {}（未分配给 copilot，已记下 tools/timeout）",
                        s.display_name()
                    ));
                } else {
                    summary.push(format!("移除 {id}（未分配给 copilot）", id = s.id));
                }
                changed = true;
            }
        }

        for s in super::shared::wanted(servers, "copilot") {
            let conn = to_copilot_entry(s);
            let is_new = !map.contains_key(&s.id);

            // 条目在就用它；不在就用停用前记下的字段重建
            let saved = options.remove(&s.id);
            if saved.is_some() {
                options_changed = true;
            }
            let mut entry = map
                .get(&s.id)
                .and_then(|v| v.as_object())
                .cloned()
                .or_else(|| saved.and_then(|v| v.as_object().cloned()))
                .unwrap_or_default();
            let before = entry.clone();

            for f in TRANSPORT_FIELDS {
                entry.remove(*f);
            }
            for (k, v) in &conn {
                entry.insert(k.clone(), v.clone());
            }
            // 工具过滤默认全部，和 `copilot mcp add` 一致
            entry
                .entry("tools".to_string())
                .or_insert_with(|| serde_json::json!(["*"]));

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

        let mut plans = vec![McpFilePlan {
            path,
            agent: "copilot".to_string(),
            content,
            pre_hash,
            changed,
            summary,
        }];

        // 停用前记下的 tools / timeout 单独作为一个文件计划。
        // 不能在 plan 里直接写盘 —— plan 必须是只读的，同步的其余部分
        // 也靠这个前提做「先全部规划、任一失败就中止」
        if options_changed {
            let options_orig = if options_path.exists() {
                std::fs::read_to_string(&options_path).unwrap_or_default()
            } else {
                String::new()
            };
            let mut text = serde_json::to_string_pretty(&Value::Object(options))?;
            text.push('\n');
            if text != options_orig {
                plans.push(McpFilePlan {
                    path: options_path.clone(),
                    agent: "copilot".to_string(),
                    content: text,
                    pre_hash: hash_file(&options_path)?,
                    changed: true,
                    summary: vec!["记录停用 server 的 tools / timeout".to_string()],
                });
            }
        }

        Ok(plans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn stdio(id: &str) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            transport: Transport::Stdio,
            command: "node".into(),
            args: Vec::new(),
            cwd: String::new(),
            url: String::new(),
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["copilot".into()],
            note: String::new(),
        }
    }

    #[test]
    fn stdio_uses_local_type_and_always_has_args() {
        let e = to_copilot_entry(&stdio("s"));
        assert_eq!(e["type"], "local");
        assert!(e.contains_key("args"), "args 必须存在，空也要写 []");
    }
}
