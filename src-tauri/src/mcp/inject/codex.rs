//! Codex 的 MCP 注入器：`~/.codex/config.toml` 的 `[mcp_servers.<id>]`。
//!
//! ## 两条实地核实过的硬规则，写错会静默失效
//!
//! **1. 不写 `type`。** Codex 由 `command`（stdio）或 `url`（streamable HTTP）
//! 推断传输方式。写了 `type` 会被当成未知字段，`codex doctor --json` 报：
//!
//! ```text
//! Codex is ignoring 4 unrecognized configuration settings.
//!   user (...config.toml): `mcp_servers.context7.type` is ignored.
//! ```
//!
//! `codex mcp add` 自己写出来的条目里也确实没有 `type`。
//!
//! **2. 请求头字段是 `http_headers`，不是 `headers`。** 写 `headers` 不报错，
//! 但被静默忽略 —— 远程 server 的 `Authorization` 就丢了，表现为 401。
//!
//! 读取时两个都认（老文件里可能已经有 `headers`）。
//!
//! ## 专属字段
//!
//! `env_vars`（透传的环境变量**名**列表，和 `env` 是两回事）、
//! `startup_timeout_sec`、`tool_timeout_sec`、`bearer_token_env_var`、
//! `env_http_headers` 都是 Codex 自己的，写入时原样保留。
//!
//! 一个传输方式的专属字段不能出现在另一种里：写了 `command` 又写 `url`
//! 会让整份配置拒绝加载（`url is not supported for stdio`）。

use super::shared::{prune_unassigned_toml, wanted};
use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use std::path::PathBuf;

/// stdio 条目专属字段。写 url 型时要从条目里清掉。
const STDIO_FIELDS: &[&str] = &["command", "args", "cwd", "env", "env_vars", "shell"];
/// 远程条目专属字段。写 stdio 型时要从条目里清掉。
const REMOTE_FIELDS: &[&str] = &["url", "http_headers", "headers", "bearer_token_env_var", "env_http_headers"];

/// Codex 的 MCP 注入器。
pub struct CodexMcpInjector {
    pub path: PathBuf,
}

impl CodexMcpInjector {
    pub fn new() -> Self {
        Self {
            path: crate::providers::store::home_dir()
                .join(".codex")
                .join("config.toml"),
        }
    }
}

impl Default for CodexMcpInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// 把母版 server 写成一个 Codex 的 `[mcp_servers.<id>]` 表。
///
/// 不写 `type`；HTTP 用 `http_headers`。
///
/// **不做 Windows `cmd /c` 包装。** 用 `codex mcp add npxdirect -- npx -y pkg`
/// 实地验过，Codex 自己写出来的是 `command = "npx"`，不包。跟着它走，
/// 免得平白多一层外壳进程。这和 Claude / Gemini 那几家不同，见
/// `plan/mcp-management.md` 的通则 1。
fn to_codex_table(s: &McpServer) -> toml_edit::Table {
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
                // 必须叫 http_headers。写 headers 会被 Codex 静默忽略
                let mut h = toml_edit::Table::new();
                for (k, v) in &s.headers {
                    h[k] = toml_edit::value(v.clone());
                }
                t["http_headers"] = toml_edit::Item::Table(h);
            }
        }
    }
    t
}

/// 把母版 server 的连接字段写进一个已有的 Codex 条目，保留其余字段。
///
/// 返回是否有改动。
fn merge_into_table(t: &mut toml_edit::Table, s: &McpServer) -> bool {
    // toml_edit::Table 没有 PartialEq，比较序列化后的形状
    let before = t.to_string();

    // 只清「另一种传输方式」的专属字段，再清 type。
    //
    // 不能两种都清：`env_vars`、`startup_timeout_sec` 是 Codex 自己的字段，
    // stdio 条目里必须留着。全清会把用户在 Codex 里设的透传列表和超时洗掉。
    let to_clear: &[&str] = match s.transport {
        Transport::Stdio => REMOTE_FIELDS,
        _ => STDIO_FIELDS,
    };
    for f in to_clear {
        t.remove(*f);
    }
    // `type` 一律不写。老配置里可能残留，清掉
    t.remove("type");
    // 裸 `headers` 是 Codex 不认的旧写法（现在叫 http_headers）。
    // 它是远程字段，但写远程时也要清 —— 否则新旧两个头表同时存在，
    // Codex 只认 http_headers，旧的会被报成未知字段
    t.remove("headers");

    let src = to_codex_table(s);
    for (k, v) in src.iter() {
        t[k] = v.clone();
    }

    t.to_string() != before
}

impl McpInjector for CodexMcpInjector {
    fn agent(&self) -> &'static str {
        "codex"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let orig = if self.path.exists() {
            std::fs::read_to_string(&self.path)?
        } else {
            String::new()
        };
        let pre_hash = hash_file(&self.path)?;

        // toml_edit 不保留 CRLF，记下来最后还原
        let uses_crlf = orig.contains("\r\n");
        let had_trailing = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = orig
            .parse()
            .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", self.path.display()))?;

        // 历史错误格式：[mcp.servers] 多一层。清掉并迁移到标准位置
        let mut migrated = false;
        if let Some(mcp_tbl) = doc.get_mut("mcp").and_then(|i| i.as_table_like_mut()) {
            if mcp_tbl.remove("servers").is_some() {
                migrated = true;
            }
        }

        if !doc.contains_key("mcp_servers") {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let tbl = doc["mcp_servers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 mcp_servers 不是表", self.path.display()))?;

        let mut summary = Vec::new();
        let mut changed = migrated;
        if migrated {
            summary.push("清理历史错误格式 [mcp.servers]，迁移到 [mcp_servers]".to_string());
        }

        for id in prune_unassigned_toml(tbl, "codex", servers) {
            summary.push(format!("移除 {id}（未分配给 codex）"));
            changed = true;
        }

        for s in wanted(servers, "codex") {
            match tbl.get_mut(&s.id).and_then(|i| i.as_table_mut()) {
                Some(existing) => {
                    if merge_into_table(existing, s) {
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                    }
                }
                None => {
                    tbl.insert(&s.id, toml_edit::Item::Table(to_codex_table(s)));
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
            path: self.path.clone(),
            agent: "codex".to_string(),
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
    use crate::mcp::model::Transport;
    use std::collections::BTreeMap;

    /// 建一个用完即删的临时目录。
    ///
    /// 项目没引 `tempfile` crate，这里手动建删即可 —— 测试里只需要一个
    /// 不会被别的东西碰到的路径。
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "agentcenter-codex-mcp-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&p).expect("建临时目录");
            Self(p)
        }
        fn join(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn stdio(id: &str) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            transport: Transport::Stdio,
            command: "cmd".into(),
            args: vec!["/c".into(), "npx".into()],
            cwd: String::new(),
            url: String::new(),
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["codex".into()],
            note: String::new(),
        }
    }

    fn remote(id: &str) -> McpServer {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), "Bearer x".into());
        McpServer {
            id: id.into(),
            name: id.into(),
            transport: Transport::Http,
            command: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            url: "https://example.com/mcp".into(),
            headers,
            env: BTreeMap::new(),
            enabled: true,
            agents: vec!["codex".into()],
            note: String::new(),
        }
    }

    #[test]
    fn never_writes_type_because_codex_ignores_it() {
        let tmp = TempDir::new("codex");
        let path = tmp.join("config.toml");
        let inj = CodexMcpInjector { path: path.clone() };
        let plans = McpInjector::plan(&inj, &[stdio("s")]).unwrap();
        let text = &plans[0].content;
        assert!(!text.contains("type ="), "不该写 type：{text}");
    }

    #[test]
    fn remote_uses_http_headers_not_headers() {
        let tmp = TempDir::new("codex");
        let path = tmp.join("config.toml");
        let inj = CodexMcpInjector { path: path.clone() };
        let plans = McpInjector::plan(&inj, &[remote("r")]).unwrap();
        let text = &plans[0].content;
        assert!(text.contains("http_headers"), "必须写 http_headers：{text}");
        // 只允许 http_headers，不能同时出现裸 headers
        assert!(
            !text.lines().any(|l| l.trim_start().starts_with("headers ")),
            "不该同时写 headers：{text}"
        );
    }

    #[test]
    fn keeps_codex_own_fields_when_updating() {
        let tmp = TempDir::new("codex");
        let path = tmp.join("config.toml");
        std::fs::write(
            &path,
            "[mcp_servers.s]\ncommand = \"old\"\nenv_vars = [\"PATH\"]\nstartup_timeout_sec = 120\n",
        )
        .unwrap();
        let inj = CodexMcpInjector { path: path.clone() };
        let plans = McpInjector::plan(&inj, &[stdio("s")]).unwrap();
        let text = &plans[0].content;
        assert!(text.contains("env_vars"), "env_vars 要保留：{text}");
        assert!(
            text.contains("startup_timeout_sec = 120"),
            "超时要保留：{text}"
        );
        assert!(text.contains("command = \"cmd\""), "command 要更新：{text}");
    }

    #[test]
    fn migrates_legacy_nested_table() {
        let tmp = TempDir::new("codex");
        let path = tmp.join("config.toml");
        std::fs::write(&path, "[mcp.servers.old]\ncommand = \"x\"\n").unwrap();
        let inj = CodexMcpInjector { path: path.clone() };
        let plans = McpInjector::plan(&inj, &[stdio("s")]).unwrap();
        let text = &plans[0].content;
        assert!(!text.contains("[mcp.servers"), "旧格式要清掉：{text}");
        assert!(text.contains("[mcp_servers.s]"), "要写标准位置：{text}");
    }

    #[test]
    fn second_sync_is_idempotent() {
        let tmp = TempDir::new("codex");
        let path = tmp.join("config.toml");
        let inj = CodexMcpInjector { path: path.clone() };
        let first = McpInjector::plan(&inj, &[stdio("s"), remote("r")]).unwrap();
        std::fs::write(&path, &first[0].content).unwrap();
        let second = McpInjector::plan(&inj, &[stdio("s"), remote("r")]).unwrap();
        assert!(!second[0].changed, "第二次同步不该有改动：{:?}", second[0].summary);
    }
}
