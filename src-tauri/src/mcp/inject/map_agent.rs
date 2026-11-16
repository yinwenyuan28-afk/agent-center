//! 标准 `mcpServers` 型工具的注入器。
//!
//! 覆盖 Claude Code、Cursor、Qoder 三个同构的工具：都是
//! `{"mcpServers": {id: {...}}}`，差别只有文件路径。
//!
//! ZCode 也是这个形状，但键路径多了两层（`mcp.servers`）且有严格 schema，
//! 所以单独在 [`super::zcode`]。

use super::shared::{prune_unassigned, transport_entry, wanted};
use super::{assigned_to, McpFilePlan, McpInjector};
use crate::mcp::model::McpServer;
use crate::providers::safe_write::hash_file;
use serde_json::Value;
use std::path::PathBuf;

/// 一个「顶层 `mcpServers`」型工具的注入器。
pub struct McpServersMapInjector {
    pub agent: &'static str,
    pub path: PathBuf,
}

/// 给条目补上 `type`。
///
/// Claude / Cursor / Qoder 都要 `type`，值就是 `stdio` / `sse` / `http`。
/// `shared::transport_entry` 故意不带 `type` —— Codex 和 Gemini 不认这个字段，
/// 由各自的注入器决定要不要加。
fn with_type(mut entry: serde_json::Map<String, Value>, s: &McpServer) -> serde_json::Map<String, Value> {
    entry.insert(
        "type".into(),
        Value::String(s.transport.as_str().to_string()),
    );
    entry
}

impl McpInjector for McpServersMapInjector {
    fn agent(&self) -> &'static str {
        self.agent
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        // 池子为空时什么都不做，避免把「母版清空」理解成「全删」
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let orig = if self.path.exists() {
            std::fs::read_to_string(&self.path)?
        } else {
            "{}".to_string()
        };
        let pre_hash = hash_file(&self.path)?;
        let mut root = super::shared::read_json(&self.path)?;

        let map = super::shared::ensure_object(&mut root, &["mcpServers"], &self.path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配的
        for id in prune_unassigned(map, self.agent, servers) {
            summary.push(format!("移除 {id}（未分配给 {}）", self.agent));
            changed = true;
        }

        // 再写入已分配的：只替换连接字段，其余（工具自己的）保留
        for s in wanted(servers, self.agent) {
            match map.get_mut(&s.id).and_then(|v| v.as_object_mut()) {
                Some(existing) => {
                    // 比较前把目标侧的 Windows 包装还原成母版形态。
                    // 文件里是 `cmd /c npx`、母版是 `npx`，直接比会永远「有改动」
                    let current = super::shared::unwrap_windows_command(existing);
                    let desired = with_type(super::shared::transport_entry_unwrapped(s), s);
                    if super::shared::entry_matches(
                        &current,
                        &desired,
                        super::shared::TRANSPORT_FIELDS,
                    ) && current.get("type") == desired.get("type")
                    {
                        summary.push(format!("{} 已一致", s.display_name()));
                    } else {
                        for field in super::shared::TRANSPORT_FIELDS {
                            existing.remove(*field);
                        }
                        for (k, v) in &with_type(super::shared::transport_entry(s), s) {
                            existing.insert(k.clone(), v.clone());
                        }
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    }
                }
                None => {
                    map.insert(s.id.clone(), Value::Object(with_type(transport_entry(s), s)));
                    summary.push(format!("新增 {}", s.display_name()));
                    changed = true;
                }
            }
        }

        // 文件原来不存在、也没有任何改动时，不创建空文件
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
            pre_hash,
            changed,
            summary,
        }])
    }
}

/// Claude Code：`~/.claude.json` 顶层 `mcpServers`。
///
/// 同一份文件里 `projects.<路径>.mcpServers` 是项目级配置，只能改顶层。
pub fn claude() -> McpServersMapInjector {
    McpServersMapInjector {
        agent: "claude",
        path: crate::providers::store::home_dir().join(".claude.json"),
    }
}

/// Cursor：`~/.cursor/mcp.json`。
pub fn cursor() -> McpServersMapInjector {
    McpServersMapInjector {
        agent: "cursor",
        path: crate::providers::store::home_dir()
            .join(".cursor")
            .join("mcp.json"),
    }
}

/// Qoder：`~/.qoder/settings.json` 的 `mcpServers`。
/// 这个文件还有 `enabledPlugins` / `permissions` / `security` / `model` 四个键。
pub fn qoder() -> McpServersMapInjector {
    McpServersMapInjector {
        agent: "qoder",
        path: crate::providers::store::home_dir()
            .join(".qoder")
            .join("settings.json"),
    }
}

/// 给 agent 实现用的便捷入口：直接用某个 map 型注入器算计划。
pub fn plan_map(inj: &McpServersMapInjector, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
    McpInjector::plan(inj, servers)
}

/// 判断某个 id 是否已分配给该 agent（给 agent 实现查用）。
pub fn is_assigned(s: &McpServer, agent: &str) -> bool {
    assigned_to(s, agent)
}
