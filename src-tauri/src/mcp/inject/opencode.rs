//! OpenCode 的 MCP 注入器：`~/.config/opencode/` 下 js 配置的 `mcp` 段。
//!
//! ## OpenCode 不是一个文件，是三个文件依次合并
//!
//! 从 OpenCode 自己的 `app.asar` 里读出的加载顺序（后加载覆盖先加载）：
//!
//! ```text
//! config.json  →  opencode.json  →  opencode.jsonc
//! ```
//!
//! 本机正好是分裂的：`opencode.jsonc` 里有 `provider`（含 apiKey），
//! `opencode.json` 里有 `mcp`。provider 用 jsonc 那份、json 里的是死配置；
//! `mcp` 只存在于 json，所以生效。
//!
//! ## 因此有两条结论
//!
//! 1. **读**的时候要读全部并合并，只看一份会漏（就是现在的 bug：
//!    `scan_opencode` 只读 `opencode.json`）
//! 2. **写**的时候写进「已经有 `mcp` 段的那份」。写到 `.jsonc` 也能生效，
//!    但同一段配置会被拆到两个文件，用户下次打开 `.json` 看不到刚加的 server
//!
//! ## 条目格式
//!
//! | 母版 | OpenCode |
//! | --- | --- |
//! | `stdio` | `type: "local"` |
//! | `command` + `args` | `command` 数组，命令在 `[0]` |
//! | `env` | `environment` |
//! | `sse` / `http` | `type: "remote"` |
//! | `enabled` | `enabled`（不是 disabled） |
//!
//! `.jsonc` 的注释：我们走 `serde_json`，写回会丢注释。本机现在没有注释。

use super::shared::{prune_unassigned, wanted};
use super::{McpFilePlan, McpInjector};
use crate::mcp::model::{McpServer, Transport};
use crate::providers::safe_write::hash_file;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// OpenCode 的配置目录。
pub fn config_dir() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".config")
        .join("opencode")
}

/// 三个候选文件，按加载顺序（先 → 后）。
pub const CONFIG_FILES: &[&str] = &["config.json", "opencode.json", "opencode.jsonc"];

/// 选一个要写的文件。
///
/// 优先「已经有 `mcp` 段」的那份；都没有就写 `opencode.json`（不存在则创建）。
/// 最后才考虑 `.jsonc` —— 它是后加载的，写进去会盖住前面文件里的 `mcp`。
pub fn write_target() -> PathBuf {
    let dir = config_dir();
    for name in ["opencode.json", "opencode.jsonc"] {
        let p = dir.join(name);
        if p.exists() && file_has_mcp(&p) {
            return p;
        }
    }
    // 都没有 mcp 段：优先已有的 json，其次已有 jsonc，最后新建 json
    let json = dir.join("opencode.json");
    if json.exists() {
        return json;
    }
    let jsonc = dir.join("opencode.jsonc");
    if jsonc.exists() {
        return jsonc;
    }
    json
}

/// 某个文件里有没有 `mcp` 段（非空）。
fn file_has_mcp(path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else {
        return false;
    };
    v.get("mcp")
        .and_then(|m| m.as_object())
        .is_some_and(|m| !m.is_empty())
}

/// 把 JSONC 的注释和尾逗号去掉，得到一个能交给 serde_json 的字符串。
///
/// 只做去注释，不做完整 JSONC 解析 —— 条目里可能出现 `//` 开头的字符串，
/// 这里按状态机跳过字符串内部的注释符。
pub fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < bytes.len() {
        let c = bytes[i];

        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }

        // 行注释
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // 块注释
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }

        out.push(c);
        i += 1;
    }

    // 去掉对象/数组末尾多余的逗号
    remove_trailing_commas(&out)
}

/// 去掉 `,` 后面只跟 `}` 或 `]` 的尾逗号。
fn remove_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }

        if c == ',' {
            // 往后看第一个非空白字符
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1; // 丢掉这个逗号
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// 读全部三份文件并按加载顺序合并（后者覆盖前者）。
///
/// 任何一份解析失败都返回错误 —— 我们不能假装它不存在，那样会漏条目。
pub fn read_merged() -> anyhow::Result<Value> {
    let dir = config_dir();
    let mut merged = Value::Object(Map::new());

    for name in CONFIG_FILES {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&strip_jsonc(&text))
            .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", path.display()))?;
        merge_json(&mut merged, v);
    }

    Ok(merged)
}

/// 深合并：对象逐键递归，其他类型直接覆盖。
fn merge_json(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(&k) {
                    Some(existing) => merge_json(existing, v),
                    None => {
                        a.insert(k, v);
                    }
                }
            }
        }
        (a, b) => *a = b,
    }
}

/// OpenCode 注入器。
pub struct OpencodeMcpInjector;

/// 把一个母版 server 转成 OpenCode 条目。
fn to_opencode_entry(s: &McpServer) -> Value {
    let mut entry = Map::new();
    match s.transport {
        Transport::Stdio => {
            entry.insert("type".into(), Value::String("local".into()));
            // OpenCode 的 command 是数组：可执行文件在 [0]，其余是参数。
            // Windows 上 npx / node 要包 cmd /c，由 windows_command_array 统一处理
            let cmd: Vec<Value> = super::shared::windows_command_array(s)
                .into_iter()
                .map(Value::String)
                .collect();
            entry.insert("command".into(), Value::Array(cmd));
            if !s.env.is_empty() {
                let mut e = Map::new();
                for (k, v) in &s.env {
                    e.insert(k.clone(), Value::String(v.clone()));
                }
                entry.insert("environment".into(), Value::Object(e));
            }
        }
        Transport::Sse | Transport::Http => {
            entry.insert("type".into(), Value::String("remote".into()));
            entry.insert("url".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut h = Map::new();
                for (k, v) in &s.headers {
                    h.insert(k.clone(), Value::String(v.clone()));
                }
                entry.insert("headers".into(), Value::Object(h));
            }
        }
    }
    entry.insert("enabled".into(), Value::Bool(s.enabled));
    Value::Object(entry)
}

impl McpInjector for OpencodeMcpInjector {
    fn agent(&self) -> &'static str {
        "opencode"
    }

    fn plan(&self, servers: &[McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        if servers.is_empty() {
            return Ok(Vec::new());
        }

        let path = write_target();
        let orig = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            "{}".to_string()
        };
        let pre_hash = hash_file(&path)?;

        // 写目标文件本身，不能先把三份合并再写回去（那会把别的文件的内容搬进来）
        let mut root: Value = if orig.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(&strip_jsonc(&orig))
                .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", path.display()))?
        };

        let map = super::shared::ensure_object(&mut root, &["mcp"], &path)?;

        let mut summary = Vec::new();
        let mut changed = false;

        for id in prune_unassigned(map, "opencode", servers) {
            summary.push(format!("移除 {id}（未分配给 opencode）"));
            changed = true;
        }

        for s in wanted(servers, "opencode") {
            let new_entry = to_opencode_entry(s);
            match map.get(&s.id) {
                Some(existing) => {
                    // 连接字段由我们管，其余（用户可能加的）保留
                    let mut merged = existing.as_object().cloned().unwrap_or_default();
                    let mut baseline = merged.clone();
                    for f in ["type", "command", "environment", "url", "headers", "enabled"] {
                        merged.remove(f);
                    }
                    if let Some(new_obj) = new_entry.as_object() {
                        for (k, v) in new_obj {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                    if merged != baseline {
                        map.insert(s.id.clone(), Value::Object(merged));
                        summary.push(format!("更新 {}", s.display_name()));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已一致", s.display_name()));
                        baseline.clear();
                    }
                }
                None => {
                    map.insert(s.id.clone(), new_entry);
                    summary.push(format!("新增 {}", s.display_name()));
                    changed = true;
                }
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
    fn strips_line_and_block_comments_but_keeps_strings() {
        let src = r#"{
          // 这是注释
          "a": "http://x//y",  /* 块注释 */
          "b": [1, 2,],
        }"#;
        let out = strip_jsonc(src);
        let v: Value = serde_json::from_str(&out).expect("should parse");
        assert_eq!(v["a"], "http://x//y", "字符串里的 // 不能被当注释");
        assert_eq!(v["b"][1], 2);
    }

    #[test]
    fn merged_read_later_file_wins() {
        let mut base = serde_json::json!({"provider": {"a": 1}, "mcp": {"x": {}}});
        let overlay = serde_json::json!({"provider": {"a": 2}});
        merge_json(&mut base, overlay);
        assert_eq!(base["provider"]["a"], 2);
        assert!(base["mcp"]["x"].is_object(), "另一份的 mcp 要留着");
    }
}
