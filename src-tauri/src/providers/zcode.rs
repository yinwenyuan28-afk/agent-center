//! ZCode 的 provider 注入器。
//!
//! ZCode 现在有两份相关配置：
//!
//! 1. `~/.zcode/v2/config.json` —— 旧版 provider map（模型元数据仍写这里）
//! 2. `~/.zcode/v2/provider_config.json` —— **设置页「自定义供应商」真正读取的列表**
//!
//! 只写 `config.json` 时，文件里能看到条目，但 UI 侧栏不会出现。
//! 本注入器两份都同步。
//!
//! ## 存量的 provider 怎么处理
//!
//! 分配关系存在母版的 `agents` 字段里，所以注入器必须两个方向都执行：
//!
//! - 母版声明的 provider：存在就更新，不存在就新增
//! - 母版里有、但**没**分配给 zcode 的 provider：从配置里移除
//! - 母版里根本没有的 provider（用户手动配的、内置模板）：**保持原样不动**
//!
//! 三个方向靠 base_url 识别。ZCode 的键是 UUID，不能拿母版的 id 直接匹配，
//! 所以这里用端点当身份 —— 同一个端点就是同一个 provider。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub struct ZCodeInjector;

impl ZCodeInjector {
    fn config_path() -> PathBuf {
        super::store::home_dir()
            .join(".zcode")
            .join("v2")
            .join("config.json")
    }

    fn personal_config_path() -> PathBuf {
        super::store::home_dir()
            .join(".zcode")
            .join("v2")
            .join("provider_config.json")
    }
}

/// 把统一的 provider 翻译成 ZCode 旧版 provider 对象。
fn to_zcode_provider(p: &Provider) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(p.name.clone()));
    obj.insert(
        "kind".into(),
        Value::String(
            match p.kind_for("zcode") {
                ProviderKind::Anthropic => "anthropic",
                ProviderKind::OpenaiChat
                | ProviderKind::OpenaiResponses
                | ProviderKind::GoogleGenAi => "openai-compatible",
            }
            .to_string(),
        ),
    );

    let mut options = Map::new();
    options.insert("apiKey".into(), Value::String(p.api_key.clone()));
    options.insert("baseURL".into(), Value::String(p.base_url.clone()));
    options.insert("apiKeyRequired".into(), Value::Bool(true));
    obj.insert("options".into(), Value::Object(options));

    obj.insert("source".into(), Value::String("custom".into()));

    let mut models = Map::new();
    if p.models.is_empty() {
        models.insert("default".into(), make_model_entry(&ModelSpec::new("default")));
    } else {
        for spec in &p.models {
            models.insert(spec.id.clone(), make_model_entry(spec));
        }
    }
    obj.insert("models".into(), Value::Object(models));

    Value::Object(obj)
}

fn personal_api_type(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::OpenaiChat | ProviderKind::GoogleGenAi => "openai-chat-completions",
    }
}

fn model_ids(p: &Provider) -> Vec<String> {
    if p.models.is_empty() {
        vec!["default".to_string()]
    } else {
        p.models.iter().map(|m| m.id.clone()).collect()
    }
}

fn to_personal_provider_rule(provider_id: &str, p: &Provider) -> Value {
    let ids = model_ids(p);
    json!({
        "providerId": provider_id,
        "providerName": p.name,
        "config": {
            "group": "standard-personal",
            "access": {
                "type": "api-key",
                "apiKey": p.api_key,
            },
            "api": {
                "type": personal_api_type(p.kind_for("zcode")),
                "baseUrl": p.base_url,
            },
            "personalModelIds": ids,
            "modelOrder": ids,
        }
    })
}

fn personal_model_rules(provider_id: &str, p: &Provider) -> Vec<Value> {
    let specs: Vec<&ModelSpec> = if p.models.is_empty() {
        Vec::new()
    } else {
        p.models.iter().collect()
    };

    if specs.is_empty() {
        return vec![json!({
            "modelId": "default",
            "providerId": provider_id,
            "config": { "properties": { "contextWindow": 128000 } }
        })];
    }

    specs
        .into_iter()
        .map(|spec| {
            json!({
                "modelId": spec.id,
                "providerId": provider_id,
                "config": {
                    "properties": {
                        "contextWindow": spec.context_or_default()
                    }
                }
            })
        })
        .collect()
}

fn make_model_entry(spec: &ModelSpec) -> Value {
    let mut m = Map::new();

    if let Some(name) = &spec.name {
        m.insert("name".into(), Value::String(name.clone()));
    }

    let mut limit = Map::new();
    limit.insert(
        "context".into(),
        Value::Number(spec.context_or_default().into()),
    );
    limit.insert(
        "output".into(),
        Value::Number(spec.output_or_default().into()),
    );
    m.insert("limit".into(), Value::Object(limit));

    let mut modalities = Map::new();
    modalities.insert(
        "input".into(),
        Value::Array(
            spec.input_or_default()
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    let output_mods = if spec.output_modalities.is_empty() {
        vec!["text".to_string()]
    } else {
        spec.output_modalities.clone()
    };
    modalities.insert(
        "output".into(),
        Value::Array(output_mods.into_iter().map(Value::String).collect()),
    );
    m.insert("modalities".into(), Value::Object(modalities));

    if spec.reasoning_or_infer() {
        let mut reasoning = Map::new();
        reasoning.insert("enabled".into(), Value::Bool(true));
        if !spec.reasoning_efforts.is_empty() {
            reasoning.insert(
                "variants".into(),
                Value::Array(
                    spec.reasoning_efforts
                        .iter()
                        .map(|e| Value::String(e.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(def) = &spec.default_effort {
            reasoning.insert("defaultVariant".into(), Value::String(def.clone()));
        }
        m.insert("reasoning".into(), Value::Object(reasoning));
    }

    Value::Object(m)
}

fn personal_rule_base_url(rule: &Value) -> Option<&str> {
    rule.get("config")?
        .get("api")?
        .get("baseUrl")
        .and_then(|v| v.as_str())
}

fn personal_rule_provider_id(rule: &Value) -> Option<&str> {
    rule.get("providerId").and_then(|v| v.as_str())
}

/// 优先沿用旧 config.json 里同端点的 UUID，保证两份配置键一致。
fn resolve_personal_provider_id(p: &Provider, legacy_provider_map: Option<&Map<String, Value>>) -> String {
    if let Some(map) = legacy_provider_map {
        if let Some((key, _)) = map.iter().find(|(_, v)| {
            v.get("options")
                .and_then(|o| o.get("baseURL"))
                .and_then(|b| b.as_str())
                .map(|b| normalize_url(b) == normalize_url(&p.base_url))
                .unwrap_or(false)
        }) {
            return key.clone();
        }
    }
    p.id.clone()
}

impl Injector for ZCodeInjector {
    fn agent(&self) -> &'static str {
        "zcode"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path(), Self::personal_config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let legacy_path = Self::config_path();
        if !legacy_path.exists() {
            // ZCode 没装，不生成计划
            return Ok(Vec::new());
        }
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let mut plans = Vec::new();
        plans.push(plan_legacy_config(providers, &legacy_path)?);
        plans.push(plan_personal_config(providers, &legacy_path)?);
        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        for plan in plans {
            let wanted: Vec<&Provider> = providers
                .iter()
                .filter(|p| p.agents.iter().any(|a| a == "zcode"))
                .collect();

            for p in wanted {
                let detail = plan
                    .summary
                    .iter()
                    .find(|s| s.contains(&p.name) || s.contains(&p.id))
                    .cloned()
                    .unwrap_or_else(|| "无变化".to_string());

                let action = if detail.starts_with("新增") {
                    DiffAction::Add
                } else if detail.starts_with("更新") || detail.starts_with("移除") {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                };

                out.push(DiffLine {
                    agent: "zcode".to_string(),
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

fn plan_legacy_config(providers: &[Provider], path: &PathBuf) -> anyhow::Result<FilePlan> {
    let original = std::fs::read_to_string(path)?;
    let pre_hash = hash_file(path)?;
    let mut root: Value = serde_json::from_str(&original).map_err(|e| {
        anyhow::anyhow!(
            "{} 解析失败（{}），已中止，不会写入任何文件",
            path.display(),
            e
        )
    })?;

    let mut summary = Vec::new();
    let mut changed = false;

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

    if !root_obj.contains_key("provider") {
        root_obj.insert("provider".into(), Value::Object(Map::new()));
        summary.push("创建 provider 段".to_string());
        changed = true;
    }

    let provider_map = root_obj
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("{} 的 provider 字段不是对象", path.display()))?;

    let unassigned_urls: Vec<String> = providers
        .iter()
        .filter(|p| !p.agents.iter().any(|a| a == "zcode"))
        .map(|p| normalize_url(&p.base_url))
        .collect();

    if !unassigned_urls.is_empty() {
        let to_remove: Vec<String> = provider_map
            .iter()
            .filter(|(_, v)| {
                let base = v
                    .get("options")
                    .and_then(|o| o.get("baseURL"))
                    .and_then(|b| b.as_str())
                    .unwrap_or("");
                unassigned_urls.contains(&normalize_url(base))
            })
            .map(|(k, _)| k.clone())
            .collect();

        for key in &to_remove {
            provider_map.remove(key);
            summary.push(format!("移除 provider {key}（未分配给 zcode）"));
            changed = true;
        }
    }

    let wanted: Vec<&Provider> = providers
        .iter()
        .filter(|p| p.agents.iter().any(|a| a == "zcode"))
        .collect();

    for p in &wanted {
        let existing_key = provider_map.iter().find_map(|(k, v)| {
            let base = v
                .get("options")
                .and_then(|o| o.get("baseURL"))
                .and_then(|b| b.as_str())?;
            if normalize_url(base) == normalize_url(&p.base_url) {
                Some(k.clone())
            } else {
                None
            }
        });

        let new_val = to_zcode_provider(p);

        match existing_key {
            Some(key) => {
                let entry_val = provider_map.get_mut(&key).unwrap();
                let entry = entry_val
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("provider {key} 不是对象"))?;
                let mut updated = false;

                if entry.get("name").and_then(|v| v.as_str()) != Some(&p.name) {
                    entry.insert("name".into(), Value::String(p.name.clone()));
                    updated = true;
                }

                let want_kind = match p.kind_for("zcode") {
                    ProviderKind::Anthropic => "anthropic",
                    _ => "openai-compatible",
                };
                if entry.get("kind").and_then(|v| v.as_str()) != Some(want_kind) {
                    entry.insert("kind".into(), Value::String(want_kind.to_string()));
                    updated = true;
                }

                if let Some(opts) = entry.get_mut("options").and_then(|o| o.as_object_mut()) {
                    let old_key = opts.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
                    if old_key != p.api_key {
                        opts.insert("apiKey".into(), Value::String(p.api_key.clone()));
                        updated = true;
                    }
                    let old_base = opts.get("baseURL").and_then(|v| v.as_str()).unwrap_or("");
                    if old_base != p.base_url {
                        opts.insert("baseURL".into(), Value::String(p.base_url.clone()));
                        updated = true;
                    }
                    if opts.get("apiKeyRequired").and_then(|v| v.as_bool()) != Some(true) {
                        opts.insert("apiKeyRequired".into(), Value::Bool(true));
                        updated = true;
                    }
                }

                let merged = merge_models_into(
                    entry.get_mut("models").and_then(|m| m.as_object_mut()),
                    &p.models,
                );
                if merged {
                    summary.push(format!("更新 {} 的模型元数据", p.name));
                    updated = true;
                }

                if updated {
                    summary.push(format!("更新 {} 的 name/kind/key/baseURL/models", p.name));
                    changed = true;
                } else {
                    summary.push(format!("{} 已是最新", p.name));
                }
            }
            None => {
                let mut key = p.id.clone();
                let mut n = 1;
                while provider_map.contains_key(&key) {
                    key = format!("{}-{}", p.id, n);
                    n += 1;
                }
                provider_map.insert(key.clone(), new_val);
                summary.push(format!("新增 {}（键 {}）", p.name, key));
                changed = true;
            }
        }
    }

    let content = if changed {
        let mut s = serde_json::to_string_pretty(&root)?;
        s.push('\n');
        s
    } else {
        original
    };

    Ok(FilePlan {
        path: contract_tilde(path),
        agent: "zcode".to_string(),
        content,
        pre_hash,
        post_hash: String::new(),
        changed,
        summary,
    })
}

fn plan_personal_config(
    providers: &[Provider],
    legacy_path: &PathBuf,
) -> anyhow::Result<FilePlan> {
    let path = ZCodeInjector::personal_config_path();
    let (original, mut root, existed) = if path.exists() {
        let original = std::fs::read_to_string(&path)?;
        let root: Value = serde_json::from_str(&original).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                path.display(),
                e
            )
        })?;
        (original, root, true)
    } else {
        (
            String::new(),
            json!({
                "schemaVersion": 1,
                "config": {
                    "providerConfigRules": { "providerRules": [] },
                    "modelConfigRules": {
                        "providerModelRules": [],
                        "manualProviderModelRules": []
                    }
                }
            }),
            false,
        )
    };

    let pre_hash = hash_file(&path)?;
    let mut summary = Vec::new();
    let mut changed = !existed;

    let legacy_map = std::fs::read_to_string(legacy_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("provider")
                .and_then(|p| p.as_object())
                .cloned()
        });

    // 先把两份数组 take 出来改，避免对同一棵 JSON 树多次可变借用。
    let mut rules: Vec<Value> = root
        .pointer("/config/providerConfigRules/providerRules")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut model_rules: Vec<Value> = root
        .pointer("/config/modelConfigRules/providerModelRules")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let manual_model_rules = root
        .pointer("/config/modelConfigRules/manualProviderModelRules")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    if root
        .pointer("/config/modelConfigRules/manualProviderModelRules")
        .is_none()
    {
        changed = true;
    }

    let unassigned_urls: Vec<String> = providers
        .iter()
        .filter(|p| !p.agents.iter().any(|a| a == "zcode"))
        .map(|p| normalize_url(&p.base_url))
        .collect();

    if !unassigned_urls.is_empty() {
        let before = rules.len();
        rules.retain(|rule| {
            personal_rule_base_url(rule)
                .map(|b| !unassigned_urls.contains(&normalize_url(b)))
                .unwrap_or(true)
        });
        if rules.len() != before {
            let removed = before - rules.len();
            summary.push(format!("从 provider_config 移除 {removed} 个未分配 provider"));
            changed = true;

            let keep_ids: std::collections::HashSet<String> = rules
                .iter()
                .filter_map(personal_rule_provider_id)
                .map(|s| s.to_string())
                .collect();
            let before_models = model_rules.len();
            model_rules.retain(|r| {
                r.get("providerId")
                    .and_then(|v| v.as_str())
                    .map(|id| keep_ids.contains(id))
                    .unwrap_or(true)
            });
            if model_rules.len() != before_models {
                changed = true;
            }
        }
    }

    let wanted: Vec<&Provider> = providers
        .iter()
        .filter(|p| p.agents.iter().any(|a| a == "zcode"))
        .collect();

    for p in &wanted {
        let provider_id = resolve_personal_provider_id(p, legacy_map.as_ref());
        let want_rule = to_personal_provider_rule(&provider_id, p);

        let existing_idx = rules.iter().position(|rule| {
            personal_rule_base_url(rule)
                .map(|b| normalize_url(b) == normalize_url(&p.base_url))
                .unwrap_or(false)
                || personal_rule_provider_id(rule) == Some(provider_id.as_str())
        });

        match existing_idx {
            Some(idx) => {
                if rules[idx] != want_rule {
                    rules[idx] = want_rule;
                    summary.push(format!("更新 provider_config 中的 {}", p.name));
                    changed = true;
                } else {
                    summary.push(format!("{} 在 provider_config 已是最新", p.name));
                }
            }
            None => {
                rules.push(want_rule);
                summary.push(format!("新增 provider_config 条目 {}（键 {}）", p.name, provider_id));
                changed = true;
            }
        }

        let before_models = model_rules.clone();
        model_rules.retain(|r| {
            r.get("providerId").and_then(|v| v.as_str()) != Some(provider_id.as_str())
        });
        let new_model_rules = personal_model_rules(&provider_id, p);
        model_rules.extend(new_model_rules);
        if model_rules != before_models {
            changed = true;
        }
    }

    if changed {
        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;
        let config = root_obj.entry("config").or_insert_with(|| json!({}));
        let config_obj = config
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 config 不是对象", path.display()))?;

        config_obj.insert(
            "providerConfigRules".into(),
            json!({ "providerRules": rules }),
        );
        config_obj.insert(
            "modelConfigRules".into(),
            json!({
                "providerModelRules": model_rules,
                "manualProviderModelRules": manual_model_rules,
            }),
        );
    }

    let content = if changed {
        let mut s = serde_json::to_string_pretty(&root)?;
        s.push('\n');
        s
    } else if existed {
        original
    } else {
        let mut s = serde_json::to_string_pretty(&root)?;
        s.push('\n');
        s
    };

    Ok(FilePlan {
        path: contract_tilde(&path),
        agent: "zcode".to_string(),
        content,
        pre_hash,
        post_hash: String::new(),
        changed,
        summary,
    })
}

fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

fn merge_models_into(map: Option<&mut Map<String, Value>>, specs: &[ModelSpec]) -> bool {
    let Some(map) = map else {
        return false;
    };
    let mut changed = false;

    for spec in specs {
        match map.get_mut(&spec.id).and_then(|v| v.as_object_mut()) {
            Some(entry) => {
                if spec.context_window.is_some() || spec.max_output_tokens.is_some() {
                    let limit = entry
                        .entry("limit")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Some(l) = limit.as_object_mut() {
                        if let Some(ctx) = spec.context_window {
                            if l.get("context").and_then(|v| v.as_u64()) != Some(ctx) {
                                l.insert("context".into(), Value::Number(ctx.into()));
                                changed = true;
                            }
                        }
                        if let Some(out) = spec.max_output_tokens {
                            if l.get("output").and_then(|v| v.as_u64()) != Some(out) {
                                l.insert("output".into(), Value::Number(out.into()));
                                changed = true;
                            }
                        }
                    }
                }

                if !spec.input_modalities.is_empty() || !spec.output_modalities.is_empty() {
                    let mods = entry
                        .entry("modalities")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Some(m) = mods.as_object_mut() {
                        for (key, list) in [
                            ("input", &spec.input_modalities),
                            ("output", &spec.output_modalities),
                        ] {
                            if list.is_empty() {
                                continue;
                            }
                            let want: Vec<Value> =
                                list.iter().map(|s| Value::String(s.clone())).collect();
                            let same = m.get(key).and_then(|v| v.as_array()) == Some(&want);
                            if !same {
                                m.insert(key.into(), Value::Array(want));
                                changed = true;
                            }
                        }
                    }
                }

                if spec.reasoning.is_some() || !spec.reasoning_efforts.is_empty() {
                    let r = entry
                        .entry("reasoning")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Some(r) = r.as_object_mut() {
                        if let Some(enabled) = spec.reasoning {
                            if r.get("enabled").and_then(|v| v.as_bool()) != Some(enabled) {
                                r.insert("enabled".into(), Value::Bool(enabled));
                                changed = true;
                            }
                        }
                        if !spec.reasoning_efforts.is_empty() {
                            let want: Vec<Value> = spec
                                .reasoning_efforts
                                .iter()
                                .map(|s| Value::String(s.clone()))
                                .collect();
                            let same = r.get("variants").and_then(|v| v.as_array()) == Some(&want);
                            if !same {
                                r.insert("variants".into(), Value::Array(want));
                                changed = true;
                            }
                        }
                        if let Some(def) = &spec.default_effort {
                            if r.get("defaultVariant").and_then(|v| v.as_str()) != Some(def) {
                                r.insert("defaultVariant".into(), Value::String(def.clone()));
                                changed = true;
                            }
                        }
                    }
                }

                if let Some(name) = &spec.name {
                    if entry.get("name").and_then(|v| v.as_str()) != Some(name) {
                        entry.insert("name".into(), Value::String(name.clone()));
                        changed = true;
                    }
                }
            }
            None => {
                map.insert(spec.id.clone(), make_model_entry(spec));
                changed = true;
            }
        }
    }

    changed
}
