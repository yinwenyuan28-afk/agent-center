//! 各注入器共用的转换与清理逻辑。

use super::assigned_to;
use crate::mcp::model::{McpServer, Transport};
use serde_json::{Map, Value};
use std::path::Path;

/// 母版 server 的连接字段。写入时先删这些再填新值，其余字段（工具自己的）保留。
pub const TRANSPORT_FIELDS: &[&str] = &["type", "command", "args", "cwd", "env", "url", "headers"];

/// Windows 上需要用 `cmd /c` 包装的命令。
///
/// 这些在 Windows 里实际是 `.cmd` 批处理文件，直接把名字当 `command` 写进去
/// 进程起不来。Claude Code 的 `/doctor` 会明确报
/// `Windows requires 'cmd /c' wrapper to execute npx`。
#[cfg(windows)]
const WINDOWS_WRAP_COMMANDS: &[&str] = &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// 哪些 agent 的 MCP 配置支持「停用但保留条目」。
///
/// 其余 agent 没有这个字段：全局停用的 server 必须从配置里删掉，
/// 否则 Codex / Claude 等会照常加载，界面显示「全局停用」就成了假状态。
fn keeps_disabled_entries(agent: &str) -> bool {
    matches!(agent, "opencode" | "zcode" | "workbuddy" | "pi" | "omp")
}

/// 判断母版条目是否该分配给该 agent。
///
/// `enabled = false` 的条目：
/// - 目标支持停用字段 → 仍然交给注入器，由它写 `enabled = false`
/// - 目标不支持 → 直接排除，注入器会把它从配置里清掉
///
/// 不做这层过滤时，禁用只对少数 agent 生效，其它 agent 会继续注入为可用，
/// 和界面上的「全局停用」相反。
pub fn wanted<'a>(servers: &'a [McpServer], agent: &str) -> Vec<&'a McpServer> {
    let keep_disabled = keeps_disabled_entries(agent);
    servers
        .iter()
        .filter(|s| assigned_to(s, agent) && (s.enabled || keep_disabled))
        .collect()
}

/// 从 JSON 条目推断传输方式。
///
/// 规范里可能没有 `type`（手写的、deeplink 建的都不补）。此时不能一律当 stdio：
/// 那样 url 型服务器会被写成没有 url 的 stdio 条目，目标工具直接拒绝整份配置。
pub fn transport_of(entry: &Value) -> &'static str {
    let has = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    match entry.get("type").and_then(Value::as_str) {
        Some("sse") => "sse",
        Some("http") | Some("streamable-http") | Some("streamableHttp") => "http",
        Some("stdio") | Some("local") => "stdio",
        Some(_) => "stdio",
        None if !has("command") && has("url") => "http",
        None => "stdio",
    }
}

/// 只含连接字段，**不做 Windows 包装**。
///
/// 用来和「已经从文件里还原过的现状」做比较。两边都不包装，才可比。
pub fn transport_entry_unwrapped(s: &McpServer) -> Map<String, Value> {
    let mut obj = Map::new();
    match s.transport {
        Transport::Stdio => {
            if !s.command.is_empty() {
                obj.insert("command".into(), Value::String(s.command.clone()));
            }
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
                let mut m = Map::new();
                for (k, v) in &s.env {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".into(), Value::Object(m));
            }
        }
        Transport::Sse | Transport::Http => {
            obj.insert("url".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut m = Map::new();
                for (k, v) in &s.headers {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(m));
            }
        }
    }
    obj
}

/// 判断目标侧的连接字段是否已经和母版一致。
///
/// 两边都先过 [`unwrap_windows_command`] 归一化再比。母版里可能存的是
/// 已包装的 `cmd /c npx`，文件里也是；也可能一边包了另一边没包。
/// 只比「最内层命令 + 参数」，就都不受形态影响。
///
/// 目标侧多出来的字段（`env_vars`、`tools` 之类）不参与比较 ——
/// 那些是工具自己的，我们不负责。
pub fn entry_matches(
    current: &Map<String, Value>,
    desired: &Map<String, Value>,
    fields: &[&str],
) -> bool {
    let a_norm = unwrap_windows_command(current);
    let b_norm = unwrap_windows_command(desired);
    for f in fields {
        let a = a_norm.get(*f);
        let b = b_norm.get(*f);
        // 空对象和「没有这个键」视为相同：Claude 的 `mcp add` 会写 `env: {}`，
        // 母版里没有 env，两者语义一样
        let a_empty_obj = a.and_then(Value::as_object).is_some_and(|o| o.is_empty());
        let b_empty_obj = b.and_then(Value::as_object).is_some_and(|o| o.is_empty());
        let same = match (a, b) {
            (None, None) => true,
            (None, Some(_)) if b_empty_obj => true,
            (Some(_), None) if a_empty_obj => true,
            (Some(x), Some(y)) => x == y,
            _ => false,
        };
        if !same {
            return false;
        }
    }
    true
}

/// 把母版 server 转成一个「只有连接字段」的 JSON 对象。
///
/// 各注入器在这个基础上改字段名（`env` → `environment`、`url` → `httpUrl` 等）。
pub fn transport_entry(s: &McpServer) -> Map<String, Value> {
    let mut obj = Map::new();
    match s.transport {
        Transport::Stdio => {
            if !s.command.is_empty() {
                obj.insert("command".into(), Value::String(s.command.clone()));
            }
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
                let mut m = Map::new();
                for (k, v) in &s.env {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".into(), Value::Object(m));
            }
        }
        Transport::Sse | Transport::Http => {
            obj.insert("url".into(), Value::String(s.url.clone()));
            if !s.headers.is_empty() {
                let mut m = Map::new();
                for (k, v) in &s.headers {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".into(), Value::Object(m));
            }
        }
    }
    wrap_windows_command(&mut obj, s);
    obj
}

/// 判断一个已存在的条目是不是「已经是包装过的形态」。
///
/// 母版里存的是**未包装**的 `command = "npx"`，目标文件里存的是包装过的
/// `command = "cmd"` + `args = ["/c", "npx", ...]`。如果每次都按母版重算一遍
/// 写回去，结果虽然一样，但读回来的 `cmd /c` 会被再包一层 ——
/// `cmd /c cmd /c npx`，越同步越深。
///
/// 所以比较时要把目标侧还原成母版形态：剥掉 `cmd /c` 前缀。
pub fn unwrap_windows_command(entry: &Map<String, Value>) -> Map<String, Value> {
    let mut out = entry.clone();
    let is_cmd = out
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.eq_ignore_ascii_case("cmd") || c.eq_ignore_ascii_case("cmd.exe"));
    if !is_cmd {
        return out;
    }
    let Some(args) = out.get("args").and_then(Value::as_array) else {
        return out;
    };
    // 至少要 [ "/c", "命令" ]
    if args.len() < 2 || args.first().and_then(Value::as_str) != Some("/c") {
        return out;
    }
    let inner = args[1].as_str().unwrap_or("").to_string();
    let rest: Vec<Value> = args[2..].to_vec();
    let name = Path::new(&inner)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| inner.clone());
    if !WINDOWS_WRAP_COMMANDS
        .iter()
        .any(|c| name.eq_ignore_ascii_case(c))
    {
        return out;
    }
    out.insert("command".into(), Value::String(inner));
    out.insert("args".into(), Value::Array(rest));
    out
}

/// Windows 上把 `npx args...` 改写成 `cmd /c npx args...`。
///
/// 只处理 stdio；已经是 `cmd` 的不重复包；命令名取 `file_stem`，
/// 所以 `npx.cmd` 也能识别。非 Windows 平台是空实现。
#[cfg(windows)]
pub fn wrap_windows_command(obj: &mut Map<String, Value>, s: &McpServer) {
    if s.transport != Transport::Stdio {
        return;
    }
    let mut current = obj.get("command").and_then(Value::as_str).unwrap_or("").to_string();
    // 母版里可能就是包装过的形态（用户手写的），先还原到最内层
    if current.eq_ignore_ascii_case("cmd") || current.eq_ignore_ascii_case("cmd.exe") {
        let unwrapped = unwrap_windows_command(obj);
        current = unwrapped
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        *obj = unwrapped;
    }
    if current.is_empty() {
        // 母版里没写 command，不能凭空造一个
        obj.remove("command");
        return;
    }
    let name = Path::new(&current)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&current);
    if !WINDOWS_WRAP_COMMANDS
        .iter()
        .any(|c| name.eq_ignore_ascii_case(c))
    {
        obj.insert("command".into(), Value::String(current));
        return;
    }
    let mut args = vec![Value::String("/c".into()), Value::String(current)];
    if let Some(existing) = obj.get("args").and_then(Value::as_array) {
        args.extend(existing.iter().cloned());
    }
    obj.insert("command".into(), Value::String("cmd".into()));
    obj.insert("args".into(), Value::Array(args));
}

#[cfg(not(windows))]
pub fn wrap_windows_command(obj: &mut Map<String, Value>, s: &McpServer) {
    obj.insert(
        "command".into(),
        Value::String(s.command.clone()),
    );
}

/// 从 JSON map 里移除「母版里声明了、但没分配给这个 agent」的条目。
///
/// **只删母版里存在的 id**。用户手动配的、WorkBuddy 市场连接器不在母版里，
/// 永远不被碰。返回被删的 id。
pub fn prune_unassigned(
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

/// 从 TOML 表里移除未分配的条目。
pub fn prune_unassigned_toml(
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

/// 读 JSON 文件。不存在返回 `{}`，解析失败返回带文件名的错误。
pub fn read_json(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("{} 解析失败（{e}），已中止", path.display()))
}

/// 取一个 JSON 对象里的子对象。不存在就建空对象。
pub fn ensure_object<'a>(
    root: &'a mut Value,
    path: &[&str],
    file: &Path,
) -> anyhow::Result<&'a mut Map<String, Value>> {
    let mut cur = root;
    for (i, key) in path.iter().enumerate() {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 {} 不是对象", file.display(), key))?;
        if !obj.contains_key(*key) {
            obj.insert((*key).into(), Value::Object(Map::new()));
        }
        cur = obj.get_mut(*key).expect("just inserted");
        if i + 1 == path.len() && !cur.is_object() {
            return Err(anyhow::anyhow!(
                "{} 的 {} 不是对象",
                file.display(),
                path.join(".")
            ));
        }
    }
    Ok(cur.as_object_mut().expect("checked"))
}

/// 取「按 Windows 规则包装后」的完整命令数组。
///
/// 给 `command` 要写成数组的工具（OpenCode）用。母版里可能已经是包装过的
/// `cmd /c npx ...`，也可能只是 `npx ...` —— 先归一化到最内层再包装一次，
/// 保证结果稳定。返回 `[可执行文件, 参数...]`。
pub fn windows_command_array(s: &McpServer) -> Vec<String> {
    let obj = transport_entry(s);
    let head = obj
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or(&s.command)
        .to_string();
    let mut out = vec![head];
    if let Some(arr) = obj.get("args").and_then(Value::as_array) {
        out.extend(arr.iter().filter_map(|x| x.as_str().map(String::from)));
    }
    out
}

/// 报告用：把「新增 / 更新 / 已一致 / 移除」翻译成统一措辞。
pub fn summary_line(kind: &str, s: &McpServer) -> String {
    format!("{} {}", kind, s.display_name())
}
