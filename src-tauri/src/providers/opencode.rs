//! opencode 的 provider 注入器。
//!
//! 和 Pi 一样要改两个文件，但路径不同：
//!
//! - `~/.config/opencode/opencode.json`      —— provider 定义
//! - `~/.local/share/opencode/auth.json`     —— key
//!
//! 注意 auth.json 在 `~/.local/share/` 下，不在 `.config/opencode/` 里。
//!
//! ## 和新旧两种 schema 的兼容
//!
//! opencode 有两套写法：新版 `providers`（key 单独存 auth.json）
//! 和已废弃但仍在接受的旧版 `provider`（key 可直接写在 `options.apiKey`）。
//! 这里**统一写新版 `providers`**，因为：
//!
//! - 新版是当前 schema，旧版标注已废弃
//! - key 分离存储更安全，也和我们"唯一 key 来源"的目标一致
//!
//! 如果文件里已经有旧版 `provider` 段，不动它 —— 那是用户手动配的。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct OpencodeInjector;

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

/// 把统一协议映射成 opencode 的 package 字段。
///
/// opencode 用「运行时 provider 包」表达协议。它自己没有单独的
/// responses 包，所以 OpenaiResponses 也走 openai-compatible 包 ——
/// 这在这里是安全的，因为 responses 和 chat 共用同一个兼容层。
fn opencode_package(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "@opencode/ai/providers/anthropic",
        ProviderKind::OpenaiChat
        | ProviderKind::OpenaiResponses
        | ProviderKind::GoogleGenAi => "@opencode/ai/providers/openai-compatible",
    }
}

/// 构造 opencode 的 provider 定义（`providers.<id>` 那一条）。
fn to_opencode_provider(p: &Provider) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(p.name.clone()));
    obj.insert(
        "package".into(),
        Value::String(opencode_package(p.kind_for("opencode")).to_string()),
    );

    // baseURL 在 settings 里，不是顶层
    let mut settings = Map::new();
    settings.insert("baseURL".into(), Value::String(p.base_url.clone()));
    obj.insert("settings".into(), Value::Object(settings));

    // models 是 map，key 是 modelID
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
    obj.insert("models".into(), Value::Object(models));

    Value::Object(obj)
}

/// 构造一个 opencode 模型条目。
///
/// opencode 的模型定义以它的模型目录为准，这里只写名称。
/// 母版填了展示名就用母版的。
fn make_opencode_model(spec: &ModelSpec) -> Value {
    let mut m = Map::new();
    m.insert(
        "name".into(),
        Value::String(spec.display_name().to_string()),
    );
    Value::Object(m)
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

        // 取消分配的 provider：两个文件里都要摘掉
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "opencode"))
            .collect();

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

        if !root.contains_key("providers") {
            root.insert("providers".into(), Value::Object(Map::new()));
        }
        let providers_map = root
            .get_mut("providers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 providers 不是对象", cfg_path.display()))?;

        let mut cfg_summary = Vec::new();
        let mut cfg_changed = false;

        // 先移除未分配的 provider 定义
        for p in &unassigned {
            if providers_map.remove(p.id.as_str()).is_some() {
                cfg_summary.push(format!("移除 {} 的定义（未分配给 opencode）", p.name));
                cfg_changed = true;
            }
        }

        for p in &wanted {
            let new_def = to_opencode_provider(p);

            match providers_map.get(p.id.as_str()) {
                Some(existing) => {
                    // 只比对 baseURL 和 package，不同才改。
                    // models 不覆盖，用户可能调过。
                    let old_base = existing
                        .get("settings")
                        .and_then(|s| s.get("baseURL"))
                        .or_else(|| existing.get("options").and_then(|o| o.get("baseURL")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let old_pkg = existing.get("package").and_then(|v| v.as_str()).unwrap_or("");

                    if old_base != p.base_url || old_pkg != opencode_package(p.kind_for("opencode")) {
                        if let Some(obj) =
                            providers_map.get_mut(p.id.as_str()).and_then(|v| v.as_object_mut())
                        {
                            // settings.baseURL 要保证存在
                            let settings = obj
                                .entry("settings")
                                .or_insert_with(|| Value::Object(Map::new()));
                            if let Some(s) = settings.as_object_mut() {
                                s.insert("baseURL".into(), Value::String(p.base_url.clone()));
                            }
                            obj.insert(
                                "package".into(),
                                Value::String(opencode_package(p.kind_for("opencode")).to_string()),
                            );
                            cfg_summary.push(format!("更新 {} 的 baseURL/package", p.name));
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

        // 先移除未分配的 key
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
                } else if detail.starts_with("更新") {
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
