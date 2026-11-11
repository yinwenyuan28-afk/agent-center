//! opencode 的 provider 注入器。
//!
//! 和 Pi 一样要改两个文件，但路径不同：
//!
//! - `~/.config/opencode/opencode.json`      —— provider 定义
//! - `~/.local/share/opencode/auth.json`     —— key
//!
//! 注意 auth.json 在 `~/.local/share/` 下，不在 `.config/opencode/` 里。
//!
//! ## V1 / V2 schema
//!
//! OpenCode Desktop 1.x 只认顶层 `provider`（`npm` + `options.baseURL`）。
//! V2（2.x）改用 `providers`（`package` + `settings.baseURL`），并把旧键
//! 标成废弃；在 V1 里写 `providers` 会直接 ConfigInvalidError。
//!
//! 注入器按本机已装版本选择写法：major < 2 走 V1，否则走 V2。
//! 探测失败时默认 V1（当前桌面端主流），避免再把配置写成 OpenCode 拒收。

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
    fn config_path() -> PathBuf {
        super::store::home_dir()
            .join(".config")
            .join("opencode")
            .join("opencode.json")
    }

    fn auth_path() -> PathBuf {
        super::store::home_dir()
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json")
    }
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
    if let Ok(out) = std::process::Command::new("opencode")
        .arg("--version")
        .output()
    {
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
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
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
            obj.insert("options".into(), Value::Object(options));
        }
        Schema::V2 => {
            obj.insert(
                "package".into(),
                Value::String(opencode_package_v2(kind).to_string()),
            );
            let mut settings = Map::new();
            settings.insert("baseURL".into(), Value::String(p.base_url.clone()));
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
        vec![Self::config_path(), Self::auth_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let cfg_path = Self::config_path();
        let auth_path = Self::auth_path();

        if !cfg_path.exists() && !auth_path.exists() {
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

        let mut plans = Vec::new();

        // ---- opencode.json：provider 定义 ----
        let cfg_orig = if cfg_path.exists() {
            std::fs::read_to_string(&cfg_path)?
        } else {
            "{}".to_string()
        };
        let cfg_pre = hash_file(&cfg_path)?;
        let mut cfg: Value = serde_json::from_str(&cfg_orig).map_err(|e| {
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

                    if old_base != p.base_url || old_pkg != want_pkg {
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
                                    }
                                    obj.insert(
                                        "package".into(),
                                        Value::String(want_pkg.to_string()),
                                    );
                                    obj.remove("npm");
                                    obj.remove("options");
                                }
                            }
                            cfg_summary.push(format!("更新 {} 的 baseURL/包名", p.name));
                            cfg_changed = true;
                        }
                    } else {
                        cfg_summary.push(format!("{} 的定义已一致", p.name));
                    }
                }
                None => {
                    providers_map.insert(p.id.clone(), new_def);
                    cfg_summary.push(format!("新增 {} 到 opencode.json", p.name));
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

        plans.push(FilePlan {
            path: contract_tilde(&cfg_path),
            agent: "opencode".to_string(),
            content: cfg_content,
            pre_hash: cfg_pre,
            post_hash: String::new(),
            changed: cfg_changed,
            summary: cfg_summary,
        });

        // ---- auth.json：key ----
        let auth_orig = if auth_path.exists() {
            std::fs::read_to_string(&auth_path)?
        } else {
            "{}".to_string()
        };
        let auth_pre = hash_file(&auth_path)?;
        let mut auth: Value = serde_json::from_str(&auth_orig).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                auth_path.display(),
                e
            )
        })?;

        let auth_obj = auth
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", auth_path.display()))?;

        let mut auth_summary = Vec::new();
        let mut auth_changed = false;

        for p in &unassigned {
            if auth_obj.remove(p.id.as_str()).is_some() {
                auth_summary.push(format!("移除 {} 的 key（未分配给 opencode）", p.name));
                auth_changed = true;
            }
        }

        for p in &wanted {
            match auth_obj
                .get(p.id.as_str())
                .and_then(|e| e.get("key"))
                .and_then(|k| k.as_str())
            {
                Some(existing) if existing == p.api_key => {
                    auth_summary.push(format!("{} 的 key 已一致", p.name));
                }
                Some(_) => {
                    if let Some(e) = auth_obj.get_mut(p.id.as_str()).and_then(|v| v.as_object_mut())
                    {
                        e.insert("key".into(), Value::String(p.api_key.clone()));
                        auth_summary.push(format!("更新 {} 的 key", p.name));
                        auth_changed = true;
                    }
                }
                None => {
                    let mut e = Map::new();
                    // opencode 的 type 是 "api"，注意和 Pi 的 "api_key" 不同
                    e.insert("type".into(), Value::String("api".into()));
                    e.insert("key".into(), Value::String(p.api_key.clone()));
                    auth_obj.insert(p.id.clone(), Value::Object(e));
                    auth_summary.push(format!("新增 {} 到 auth.json", p.name));
                    auth_changed = true;
                }
            }
        }

        let auth_content = if auth_changed {
            let mut s = serde_json::to_string_pretty(&auth)?;
            s.push('\n');
            s
        } else {
            auth_orig
        };

        plans.push(FilePlan {
            path: contract_tilde(&auth_path),
            agent: "opencode".to_string(),
            content: auth_content,
            pre_hash: auth_pre,
            post_hash: String::new(),
            changed: auth_changed,
            summary: auth_summary,
        });

        Ok(plans)
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
