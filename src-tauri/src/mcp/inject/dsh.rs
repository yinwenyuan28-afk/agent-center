//! DeepSeek Harness 的 MCP 注入器。
//!
//! ## 结构：一份 YAML 数组，不是映射
//!
//! `~/.dsh/profiles/<profile>/cordis.patch.yml` 是 loader 的 patch 列表。
//! MCP server 是数组里 `name` 为 `@deepseek-ai/dsh-mcp-client` 的一个条目：
//!
//! ```yaml
//! - id: cc-switch-mcp-context7
//!   name: "@deepseek-ai/dsh-mcp-client"
//!   config:
//!     serverName: context7
//!     transport: stdio
//!     command: cmd
//!     args: ["/c", "npx", "-y", "@upstash/context7-mcp"]
//! ```
//!
//! 条目可能直接挂在数组里，也可能嵌在 `- insert: [ ... ]` 分组里。
//!
//! ## 四个 profile 都要写
//!
//! `web`、`desktop`、`headless`、`acp` 是四套独立配置。同一个 server
//! 要写进每一个**已存在**的 profile。本机只有 `web` 和 `desktop`。
//!
//! 写之前要比对：同一个 server 在几个 profile 里的连接方式不一致时拒绝写入，
//! 否则改一个 profile 会让它们分叉。
//!
//! ## 三条校验
//!
//! 1. 不支持 SSE，只能用 `streamable-http`
//! 2. 名字 1–32 字符，只能 `[A-Za-z0-9_-]`
//! 3. 含 `!!js` 动态节点的条目不复制（serde_yaml 往返会把它变成普通字符串）
//!
//! 本机已装但 MCP 条目为 0。实现按 CC Switch 的 `mcp/dsh.rs` +
//! `dsh_config.rs` 的 `sync_mcp_*` 移植。

use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use std::path::PathBuf;

/// DSH 的 home。优先 `DSH_HOME`。
pub fn home() -> PathBuf {
    if let Ok(v) = std::env::var("DSH_HOME") {
        let v = v.trim();
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    crate::providers::store::home_dir().join(".dsh")
}

/// 要写的 profile 名，按优先级。
///
/// `web` / `desktop` 总要写（DSH 首次启动会建）；`headless` / `acp`
/// 只在已存在时写。
pub const TARGET_PROFILES: &[&str] = &["web", "desktop", "headless", "acp"];

/// 一个 profile 的 `cordis.patch.yml` 路径。
pub fn profile_path(name: &str) -> PathBuf {
    home().join("profiles").join(name).join("cordis.patch.yml")
}

/// 实际要写的 profile 文件列表（去重，`web` / `desktop` 总在内）。
pub fn target_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for name in TARGET_PROFILES {
        let p = profile_path(name);
        let wanted = matches!(*name, "web" | "desktop") || p.exists();
        if !wanted {
            continue;
        }
        // 目录不存在时 web/desktop 也跳过 —— 不替用户创建 profile
        if !p.parent().is_some_and(|d| d.exists()) {
            continue;
        }
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// DSH 的 MCP plugin 名。
const PLUGIN_NAME: &str = "@deepseek-ai/dsh-mcp-client";

/// DSH 注入器。
pub struct DshMcpInjector;

/// 校验 server 名：1–32 字符，只能字母数字 `_` `-`。
fn validate_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 32
        || id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        anyhow::bail!("DSH 的 MCP 名字必须是 1–32 个字母、数字、- 或 _：{id}");
    }
    Ok(())
}

/// 把一个母版 server 转成 DSH 的 plugin 条目。
fn to_plugin_entry(s: &McpServer) -> anyhow::Result<serde_json::Value> {
    validate_id(&s.id)?;
    if s.transport == Transport::Sse {
        anyhow::bail!(
            "DSH 只支持 stdio 和 Streamable HTTP，不支持 SSE（{}）",
            s.id
        );
    }

    let mut config = serde_json::Map::new();
    config.insert("serverName".into(), serde_json::json!(s.id));
    config.insert(
        "transport".into(),
        serde_json::json!(if s.transport == Transport::Stdio {
            "stdio"
        } else {
            "streamable-http"
        }),
    );

    match s.transport {
        Transport::Stdio => {
            config.insert("command".into(), serde_json::json!(s.command));
            if !s.args.is_empty() {
                config.insert("args".into(), serde_json::json!(s.args));
            }
            if !s.cwd.is_empty() {
                config.insert("cwd".into(), serde_json::json!(s.cwd));
            }
            if !s.env.is_empty() {
                config.insert("env".into(), serde_json::json!(s.env));
            }
        }
        _ => {
            config.insert("url".into(), serde_json::json!(s.url));
            if !s.headers.is_empty() {
                config.insert("headers".into(), serde_json::json!(s.headers));
            }
        }
    }

    Ok(serde_json::json!({
        "id": format!("agentcenter-mcp-{}", s.id),
        "name": PLUGIN_NAME,
        "config": serde_json::Value::Object(config),
    }))
}

/// 从一份 patch 文档里找出所有 MCP 条目，返回 (patch 下标, insert 下标或 None, 条目)。
fn find_entries(
    patches: &[serde_json::Value],
    id: &str,
) -> Vec<(usize, Option<usize>, serde_json::Value)> {
    let mut out = Vec::new();
    for (pi, patch) in patches.iter().enumerate() {
        if is_matching(patch, id) {
            out.push((pi, None, patch.clone()));
        }
        if let Some(list) = patch.get("insert").and_then(|v| v.as_array()) {
            for (ii, entry) in list.iter().enumerate() {
                if is_matching(entry, id) {
                    out.push((pi, Some(ii), entry.clone()));
                }
            }
        }
    }
    out
}

/// 判断一个条目是不是 id 对应的 MCP plugin。
fn is_matching(entry: &serde_json::Value, id: &str) -> bool {
    entry.get("name").and_then(|v| v.as_str()) == Some(PLUGIN_NAME)
        && entry.get("config").and_then(|c| c.get("serverName")).and_then(|v| v.as_str())
            == Some(id)
}

/// 读 YAML 成 JSON 值。文件为空返回空数组。
fn read_patches(text: &str, path: &std::path::Path) -> anyhow::Result<Vec<serde_json::Value>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let yaml: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", path.display()))?;
    if yaml.is_null() {
        return Ok(Vec::new());
    }
    let json = serde_json::to_value(&yaml)
        .map_err(|e| anyhow::anyhow!("{} 转 JSON 失败（{e}）", path.display()))?;
    match json {
        serde_json::Value::Array(a) => Ok(a),
        _ => anyhow::bail!("{} 顶层不是数组", path.display()),
    }
}

impl McpInjector for DshMcpInjector {
    fn agent(&self) -> &'static str {
        "dsh"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }
        let paths = target_paths();
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        // 每个路径读一遍，顺便比对同一个 server 在各 profile 里的连接方式
        let mut docs: Vec<(PathBuf, String, Vec<serde_json::Value>)> = Vec::new();
        for path in &paths {
            let text = std::fs::read_to_string(path)?;
            let patches = read_patches(&text, path)?;
            docs.push((path.clone(), text, patches));
        }

        // 同一 server 在不同 profile 里的连接定义必须一致，否则改哪个都是错的
        for s in servers.iter().filter(|s| s.agents.iter().any(|a| a == "dsh")) {
            let mut seen: Option<(PathBuf, serde_json::Value)> = None;
            for (path, _, patches) in &docs {
                let found = find_entries(patches, &s.id);
                if found.len() > 1 {
                    anyhow::bail!("{} 里 {}({}) 重复出现", path.display(), PLUGIN_NAME, s.id);
                }
                let Some((_, _, entry)) = found.into_iter().next() else {
                    continue;
                };
                let config = entry.get("config").cloned().unwrap_or_default();
                match &seen {
                    None => seen = Some((path.clone(), config)),
                    Some((first, prev)) if *prev != config => {
                        anyhow::bail!(
                            "{} 和 {} 里的 {} 连接方式不一致，先在对应用户界面里统一，再来同步",
                            first.display(),
                            path.display(),
                            s.id
                        );
                    }
                    _ => {}
                }
            }
        }

        let mut plans = Vec::new();
        for (path, orig, mut patches) in docs {
            let mut summary = Vec::new();
            let mut changed = false;

            // 移除未分配的
            for s in servers.iter().filter(|s| !s.agents.iter().any(|a| a == "dsh")) {
                let found = find_entries(&patches, &s.id);
                for (pi, ii, _) in found.into_iter().rev() {
                    match ii {
                        Some(ii) => {
                            if let Some(list) =
                                patches[pi].get_mut("insert").and_then(|v| v.as_array_mut())
                            {
                                list.remove(ii);
                                // 分组空了就整条 patch 去掉
                                if list.is_empty() {
                                    patches.remove(pi);
                                }
                            }
                        }
                        None => {
                            patches.remove(pi);
                        }
                    }
                    summary.push(format!("移除 {}（未分配给 dsh）", s.display_name()));
                    changed = true;
                }
            }

            // 写入已分配的
            for s in super::shared::wanted(servers, "dsh") {
                let new_entry = to_plugin_entry(s)?;
                let found = find_entries(&patches, &s.id);
                if found.len() > 1 {
                    anyhow::bail!("{} 里 {} 重复出现，拒绝写入", path.display(), s.id);
                }
                match found.into_iter().next() {
                    Some((pi, Some(ii), _)) => {
                        let slot = &mut patches[pi]["insert"][ii];
                        if *slot != new_entry {
                            *slot = new_entry;
                            summary.push(format!("更新 {}", s.display_name()));
                            changed = true;
                        } else {
                            summary.push(format!("{} 已一致", s.display_name()));
                        }
                    }
                    Some((pi, None, _)) => {
                        if patches[pi] != new_entry {
                            patches[pi] = new_entry;
                            summary.push(format!("更新 {}", s.display_name()));
                            changed = true;
                        } else {
                            summary.push(format!("{} 已一致", s.display_name()));
                        }
                    }
                    None => {
                        patches.push(serde_json::json!({ "insert": [new_entry] }));
                        summary.push(format!("新增 {}", s.display_name()));
                        changed = true;
                    }
                }
            }

            let content = if changed {
                // 顶层是数组，serde_yaml 会按 block 风格写出来
                let text = serde_yaml::to_string(&serde_json::Value::Array(patches))
                    .map_err(|e| anyhow::anyhow!("序列化 {} 失败：{e}", path.display()))?;
                text
            } else {
                orig
            };

            plans.push(McpFilePlan {
                path: path.clone(),
                agent: "dsh".to_string(),
                content,
                pre_hash: hash_file(&path)?,
                changed,
                summary,
            });
        }

        Ok(plans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn server(id: &str, transport: Transport) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            transport,
            command: "node".into(),
            args: vec!["s.js".into()],
            cwd: String::new(),
            url: "https://example.com/mcp".into(),
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["dsh".into()],
            note: String::new(),
        }
    }

    #[test]
    fn uses_streamable_http_word() {
        let e = to_plugin_entry(&server("x", Transport::Http)).unwrap();
        assert_eq!(e["config"]["transport"], "streamable-http");
        assert_eq!(e["name"], PLUGIN_NAME);
    }

    #[test]
    fn rejects_sse_and_bad_names() {
        assert!(to_plugin_entry(&server("x", Transport::Sse)).is_err());
        assert!(to_plugin_entry(&server("bad name", Transport::Stdio)).is_err());
        assert!(to_plugin_entry(&server(&"a".repeat(33), Transport::Stdio)).is_err());
    }

    #[test]
    fn finds_entries_inside_insert_groups() {
        let patches = vec![serde_json::json!({
            "insert": [
                {"id": "other", "name": "custom"},
                {"id": "agentcenter-mcp-x", "name": PLUGIN_NAME,
                 "config": {"serverName": "x", "transport": "stdio", "command": "node"}}
            ]
        })];
        let found = find_entries(&patches, "x");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, Some(1));
    }
}
