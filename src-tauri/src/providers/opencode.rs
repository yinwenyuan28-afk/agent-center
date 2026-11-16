//! opencode 的 provider 注入器。
//!
//! 目标文件是 `~/.config/opencode/opencode.jsonc` **或** `opencode.json`，
//! 按 opencode 自己的优先级选：`.jsonc` 存在就用 `.jsonc`，否则用 `.json`。
//!
//! 早期实现固定写 `opencode.json`。cc-switch 核对后修正：本机同时存在
//! 两个文件（`.jsonc` 里有 `shell` 设置，`.json` 里有 provider），
//! 写错文件会让 opencode 读到的 provider 和用户看到的不是同一份。
//!
//! ## 密钥内联，不碰 auth.json
//!
//! 密钥写在 provider 条目里：
//!
//! - V1：`provider.<id>.options.apiKey`
//! - V2：`providers.<id>.settings.apiKey`
//!
//! 早期实现把 key 写进 `~/.local/share/opencode/auth.json`。cc-switch 核对
//! 后确认那个文件是 opencode 自己的登录凭据，不归注入器管；两处都写会
//! 让 opencode 不知道该用哪个。
//!
//! ## V1 / V2 schema
//!
//! OpenCode Desktop 1.x 只认顶层 `provider`（`npm` + `options.baseURL`）。
//! V2（2.x）改用 `providers`（`package` + `settings.baseURL`），并把旧键
//! 标成废弃；在 V1 里写 `providers` 会直接 ConfigInvalidError。
//!
//! 注入器按本机已装版本选择写法：major < 2 走 V1，否则走 V2。
//! 探测失败时默认 V1（当前桌面端主流），避免再把配置写成 OpenCode 拒收。
//!
//! ## 注释保留的边界
//!
//! `.jsonc` 允许注释，但本工具用 `serde_json` 读写 —— 带注释的文件会
//! 解析失败并中止（不会写坏）。要做保留注释的编辑需要引入 JSONC 文档
//! 层，当前不在这一步范围内；`diff` 会明确报解析失败，用户能看出来。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct OpencodeInjector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Schema {
    /// 顶层 `provider`，字段 `npm` / `options`
    V1,
    /// 顶层 `providers`，字段 `package` / `settings`
    V2,
}

impl OpencodeInjector {
    /// 按 opencode 自己的优先级选配置文件：`.jsonc` 优先，其次 `.json`，
    /// 两个都不存在时用 `.json`（新建）。
    fn config_path() -> PathBuf {
        let dir = super::store::home_dir()
            .join(".config")
            .join("opencode");
        let jsonc = dir.join("opencode.jsonc");
        if jsonc.exists() {
            return jsonc;
        }
        dir.join("opencode.json")
    }

    /// 两个候选路径，供 `targets()` 展示（安装信息、diff 报告用）。
    fn candidate_paths() -> Vec<PathBuf> {
        let dir = super::store::home_dir()
            .join(".config")
            .join("opencode");
        vec![dir.join("opencode.jsonc"), dir.join("opencode.json")]
    }
}

/// 去掉 JSONC 注释和尾随逗号，让 `serde_json` 能解析。
///
/// 只做字符串/数组边界内的扫描，不改变字符串字面量里的 `//`。
/// 这是最小实现：保留内容语义，输出到磁盘时会丢掉注释 —— 用户能接受
/// 的唯一替代是完全不写（opencode 的整个 provider 都改不了）。
///
/// 本模块的注入器和 OpenClaw 共用这一份。
pub(crate) fn strip_jsonc(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
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
            i += 2;
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        out.push(c);
        i += 1;
    }
    // 去掉 } 或 ] 前多余的逗号（尾随逗号）
    let mut cleaned = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if *ch == ',' {
            let rest = chars[idx + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(rest, Some('}') | Some(']')) {
                continue;
            }
        }
        cleaned.push(*ch);
    }
    cleaned
}

/// 探测本机 OpenCode 该用哪套 schema。
fn detect_schema() -> Schema {
    if let Some(major) = installed_major_version() {
        if major >= 2 {
            return Schema::V2;
        }
        return Schema::V1;
    }
    Schema::V1
}

/// 尽量读出已装 OpenCode 的主版本号。读不到就返回 None。
fn installed_major_version() -> Option<u32> {
    // Windows 桌面端：读 OpenCode.exe 的文件版本
    #[cfg(windows)]
    {
        if let Some(v) = windows_desktop_file_version() {
            return Some(v);
        }
    }

    // PATH 上的 CLI：`opencode --version`
    // 走 crate::proc：主程序无控制台，裸 Command 会弹黑窗口。
    if let Ok(out) = crate::proc::output("opencode", &["--version"], None) {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(m) = parse_major(text.trim()) {
            return Some(m);
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if let Some(m) = parse_major(err.trim()) {
            return Some(m);
        }
    }
    None
}

#[cfg(windows)]
fn windows_desktop_file_version() -> Option<u32> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let exe = PathBuf::from(local)
        .join("Programs")
        .join("@opencode-aidesktop")
        .join("OpenCode.exe");
    if !exe.exists() {
        return None;
    }
    // PowerShell 比额外链 winapi 轻：只在注入时跑一次。
    let path = exe.display().to_string().replace('\'', "''");
    let script = format!("(Get-Item -LiteralPath '{path}').VersionInfo.FileVersion");
    let out = crate::proc::output("powershell", &["-NoProfile", "-Command", &script], None).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_major(text.trim())
}

fn parse_major(s: &str) -> Option<u32> {
    // 接受 "1.18.2" / "v2.0.27" / "opencode 1.18.2" 等
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let digits: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// V2：运行时 provider 包。
fn opencode_package_v2(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "@opencode/ai/providers/anthropic",
        ProviderKind::OpenaiChat
        | ProviderKind::OpenaiResponses
        | ProviderKind::GoogleGenAi => "@opencode/ai/providers/openai-compatible",
    }
}

/// V1：AI SDK npm 包。responses 走 `@ai-sdk/openai`，其余兼容层走 openai-compatible。
fn opencode_npm_v1(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "@ai-sdk/anthropic",
        ProviderKind::OpenaiResponses => "@ai-sdk/openai",
        ProviderKind::OpenaiChat | ProviderKind::GoogleGenAi => "@ai-sdk/openai-compatible",
    }
}

fn make_opencode_model(spec: &ModelSpec) -> Value {
    let mut m = Map::new();
    m.insert(
        "name".into(),
        Value::String(spec.display_name().to_string()),
    );
    Value::Object(m)
}

fn models_object(p: &Provider) -> Value {
    let mut models = Map::new();
    if p.models.is_empty() {
        models.insert(
            "default".into(),
            make_opencode_model(&ModelSpec::new("default")),
        );
    } else {
        for spec in &p.models {
            models.insert(spec.id.clone(), make_opencode_model(spec));
        }
    }
    Value::Object(models)
}

fn to_opencode_provider(p: &Provider, schema: Schema) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(p.name.clone()));
    let kind = p.kind_for("opencode");

    match schema {
        Schema::V1 => {
            obj.insert(
                "npm".into(),
                Value::String(opencode_npm_v1(kind).to_string()),
            );
            let mut options = Map::new();
            options.insert("baseURL".into(), Value::String(p.base_url.clone()));
            // key 内联在条目里。opencode 支持 `{env:NAME}` 引用，但本工具
            // 统一写明文，与 cc-switch 的盘上形状一致。
            options.insert("apiKey".into(), Value::String(p.api_key.clone()));
            obj.insert("options".into(), Value::Object(options));
        }
        Schema::V2 => {
            obj.insert(
                "package".into(),
                Value::String(opencode_package_v2(kind).to_string()),
            );
            let mut settings = Map::new();
            settings.insert("baseURL".into(), Value::String(p.base_url.clone()));
            settings.insert("apiKey".into(), Value::String(p.api_key.clone()));
            obj.insert("settings".into(), Value::Object(settings));
        }
    }

    obj.insert("models".into(), models_object(p));
    Value::Object(obj)
}

fn top_key(schema: Schema) -> &'static str {
    match schema {
        Schema::V1 => "provider",
        Schema::V2 => "providers",
    }
}

fn read_base_url(existing: &Value) -> &str {
    existing
        .get("settings")
        .and_then(|s| s.get("baseURL"))
        .or_else(|| existing.get("options").and_then(|o| o.get("baseURL")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn read_pkg(existing: &Value) -> &str {
    existing
        .get("package")
        .or_else(|| existing.get("npm"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// 读条目里的 key：V1 在 `options.apiKey`，V2 在 `settings.apiKey`。
fn read_api_key(existing: &Value) -> &str {
    existing
        .get("settings")
        .and_then(|s| s.get("apiKey"))
        .or_else(|| existing.get("options").and_then(|o| o.get("apiKey")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn expected_pkg(p: &Provider, schema: Schema) -> &'static str {
    let kind = p.kind_for("opencode");
    match schema {
        Schema::V1 => opencode_npm_v1(kind),
        Schema::V2 => opencode_package_v2(kind),
    }
}

impl Injector for OpencodeInjector {
    fn agent(&self) -> &'static str {
        "opencode"
    }

    fn targets(&self) -> Vec<PathBuf> {
        Self::candidate_paths()
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let cfg_path = Self::config_path();

        if !cfg_path.exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "opencode"))
            .collect();

        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "opencode"))
            .collect();

        let schema = detect_schema();
        let key = top_key(schema);

        // ---- opencode.json(c)：provider 定义 + key ----
        let cfg_orig = std::fs::read_to_string(&cfg_path)?;
        let cfg_pre = hash_file(&cfg_path)?;
        // 文件可能是 JSONC（带注释/尾随逗号）。先剥掉再交给 serde。
        let cfg_json = strip_jsonc(&cfg_orig);
        let mut cfg: Value = serde_json::from_str(&cfg_json).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                cfg_path.display(),
                e
            )
        })?;

        let root = cfg
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", cfg_path.display()))?;

        let mut cfg_summary = Vec::new();
        let mut cfg_changed = false;

        // V1 拒收顶层 `providers`；V2 虽仍接受旧 `provider` 但会忽略。
        // 按当前 schema 清掉对立键，避免 OpenCode 直接报 ConfigInvalidError。
        match schema {
            Schema::V1 => {
                if root.remove("providers").is_some() {
                    cfg_summary.push("移除不被 V1 接受的顶层 providers".to_string());
                    cfg_changed = true;
                }
            }
            Schema::V2 => {
                // 不动用户手写的旧 provider 段；只保证我们写在 providers 下
            }
        }

        if !root.contains_key(key) {
            root.insert(key.into(), Value::Object(Map::new()));
            cfg_changed = true;
        }
        let providers_map = root
            .get_mut(key)
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 {key} 不是对象", cfg_path.display()))?;

        for p in &unassigned {
            if providers_map.remove(p.id.as_str()).is_some() {
                cfg_summary.push(format!("移除 {} 的定义（未分配给 opencode）", p.name));
                cfg_changed = true;
            }
        }

        for p in &wanted {
            let new_def = to_opencode_provider(p, schema);
            let want_pkg = expected_pkg(p, schema);

            match providers_map.get(p.id.as_str()) {
                Some(existing) => {
                    let old_base = read_base_url(existing);
                    let old_pkg = read_pkg(existing);
                    let old_key = read_api_key(existing);

                    if old_base != p.base_url
                        || old_pkg != want_pkg
                        || old_key != p.api_key.as_str()
                    {
                        if let Some(obj) =
                            providers_map.get_mut(p.id.as_str()).and_then(|v| v.as_object_mut())
                        {
                            match schema {
                                Schema::V1 => {
                                    let options = obj
                                        .entry("options")
                                        .or_insert_with(|| Value::Object(Map::new()));
                                    if let Some(o) = options.as_object_mut() {
                                        o.insert(
                                            "baseURL".into(),
                                            Value::String(p.base_url.clone()),
                                        );
                                        o.insert(
                                            "apiKey".into(),
                                            Value::String(p.api_key.clone()),
                                        );
                                    }
                                    obj.insert(
                                        "npm".into(),
                                        Value::String(want_pkg.to_string()),
                                    );
                                    // 清掉误写的 V2 字段，避免混用
                                    obj.remove("package");
                                    obj.remove("settings");
                                }
                                Schema::V2 => {
                                    let settings = obj
                                        .entry("settings")
                                        .or_insert_with(|| Value::Object(Map::new()));
                                    if let Some(s) = settings.as_object_mut() {
                                        s.insert(
                                            "baseURL".into(),
                                            Value::String(p.base_url.clone()),
                                        );
                                        s.insert(
                                            "apiKey".into(),
                                            Value::String(p.api_key.clone()),
                                        );
                                    }
                                    obj.insert(
                                        "package".into(),
                                        Value::String(want_pkg.to_string()),
                                    );
                                    obj.remove("npm");
                                    obj.remove("options");
                                }
                            }
                            cfg_summary.push(format!("更新 {} 的 baseURL/包名/key", p.name));
                            cfg_changed = true;
                        }
                    } else {
                        cfg_summary.push(format!("{} 的定义已一致", p.name));
                    }
                }
                None => {
                    providers_map.insert(p.id.clone(), new_def);
                    cfg_summary.push(format!("新增 {} 到 opencode", p.name));
                    cfg_changed = true;
                }
            }
        }

        let cfg_content = if cfg_changed {
            let mut s = serde_json::to_string_pretty(&cfg)?;
            s.push('\n');
            s
        } else {
            cfg_orig
        };

        Ok(vec![FilePlan {
            path: contract_tilde(&cfg_path),
            agent: "opencode".to_string(),
            content: cfg_content,
            pre_hash: cfg_pre,
            post_hash: String::new(),
            changed: cfg_changed,
            summary: cfg_summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "opencode"))
            .collect();

        for plan in &plans {
            for p in &wanted {
                let detail = plan
                    .summary
                    .iter()
                    .find(|s| s.contains(&p.name))
                    .cloned()
                    .unwrap_or_else(|| "无变化".to_string());

                let action = if detail.starts_with("新增") {
                    DiffAction::Add
                } else if detail.starts_with("更新") || detail.starts_with("移除不被") {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                };

                out.push(DiffLine {
                    agent: "opencode".to_string(),
                    path: plan.path.clone(),
                    action,
                    target: p.id.clone(),
                    detail,
                });
            }
        }
        Ok(out)
    }
}
