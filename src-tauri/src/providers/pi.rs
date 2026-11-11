//! Pi 的 provider 注入器。
//!
//! Pi 的 provider 定义分散在三个文件里，这里只改其中两个：
//!
//! - `~/.pi/agent/auth.json`    —— 存 key，结构 `{ "<id>": { "type": "api_key", "key": "..." } }`
//! - `~/.pi/agent/models.json`  —— 存 provider 定义与模型列表
//!
//! `settings.json` 的 `defaultProvider` / `defaultModel` **不改** ——
//! 那是用户当前在用的选择，替用户切 provider 是越权。
//!
//! ## 两个文件的改动要一起成功
//!
//! 如果只写了 auth.json 没写 models.json，provider 有 key 但没定义，
//! Pi 启动时找不到模型。所以这两个文件在同一个 `plan` 里返回，
//! 由上层统一做"全部成功或全部不写"的保证。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct PiInjector;

impl PiInjector {
    fn dir() -> PathBuf {
        super::store::home_dir().join(".pi").join("agent")
    }

    fn auth_path() -> PathBuf {
        Self::dir().join("auth.json")
    }

    fn models_path() -> PathBuf {
        Self::dir().join("models.json")
    }
}

/// 把统一协议翻译成 Pi 的 `api` 字段值。
///
/// Pi 的词表最完整，四种协议都能直接表达，是一对一映射。
fn pi_api(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChat => "openai-completions",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GoogleGenAi => "google-generative-ai",
    }
}

/// 构造 Pi 的 provider 定义（models.json 里的那一条）。
fn to_pi_provider(p: &Provider) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(p.name.clone()));
    obj.insert("baseUrl".into(), Value::String(p.base_url.clone()));
    obj.insert("api".into(), Value::String(pi_api(p.kind_for("pi")).to_string()));

    // models 数组。母版给的是模型规格，填的字段用它，没填的走默认。
    let mut arr = Vec::new();
    if p.models.is_empty() {
        arr.push(make_pi_model(&ModelSpec::new("default")));
    } else {
        for spec in &p.models {
            arr.push(make_pi_model(spec));
        }
    }
    obj.insert("models".into(), Value::Array(arr));

    Value::Object(obj)
}

/// 构造一个 Pi 模型条目。
///
/// 只写母版里明确填了的字段。`cost`、`thinkingLevelMap` 这些
/// Pi 特有结构，母版没填就不写 —— 写了反而会覆盖 Pi 目录里的真值。
fn make_pi_model(spec: &ModelSpec) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(spec.id.clone()));
    m.insert(
        "name".into(),
        Value::String(spec.display_name().to_string()),
    );

    if spec.reasoning_or_infer() {
        m.insert("reasoning".into(), Value::Bool(true));
    } else if spec.reasoning == Some(false) {
        m.insert("reasoning".into(), Value::Bool(false));
    }

    m.insert(
        "input".into(),
        Value::Array(
            spec.input_or_default()
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    m.insert(
        "contextWindow".into(),
        Value::Number(spec.context_or_default().into()),
    );
    m.insert(
        "maxTokens".into(),
        Value::Number(spec.output_or_default().into()),
    );

    // 推理档位：Pi 用 thinkingLevelMap，值是和 null 的映射。
    // 我们只知道哪些档开了，其余置 null
    if !spec.reasoning_efforts.is_empty() {
        let mut map = Map::new();
        for level in ["off", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let val = if spec.reasoning_efforts.iter().any(|e| e == level) {
                Value::String(level.to_string())
            } else {
                Value::Null
            };
            map.insert(level.to_string(), val);
        }
        m.insert("thinkingLevelMap".into(), Value::Object(map));
    }

    // 价格
    if let Some(cost) = &spec.cost {
        let mut c = Map::new();
        if let Some(v) = cost.input {
            c.insert("input".into(), Value::from(v));
        }
        if let Some(v) = cost.output {
            c.insert("output".into(), Value::from(v));
        }
        if let Some(v) = cost.cache_read {
            c.insert("cacheRead".into(), Value::from(v));
        }
        if let Some(v) = cost.cache_write {
            c.insert("cacheWrite".into(), Value::from(v));
        }
        m.insert("cost".into(), Value::Object(c));
    }

    Value::Object(m)
}

/// 把母版里的模型元数据按 id 合并进 Pi 的 models 数组。
///
/// 三条规则：
/// - 母版有、配置里没有 → 追加一条
/// - 两边都有 → 只覆盖母版里**明确填了**的字段
/// - 配置里有、母版没列 → 保留（Pi 的 models-store 会从远端目录发现模型）
///
/// 返回是否发生了改动。
fn merge_pi_models(provider: &mut Value, specs: &[ModelSpec]) -> bool {
    let Some(obj) = provider.as_object_mut() else {
        return false;
    };
    let Some(arr) = obj.get_mut("models").and_then(|v| v.as_array_mut()) else {
        // 没有 models 数组：整体补上
        let new: Vec<Value> = specs.iter().map(make_pi_model).collect();
        if new.is_empty() {
            return false;
        }
        obj.insert("models".into(), Value::Array(new));
        return true;
    };

    let mut changed = false;
    for spec in specs {
        let idx = arr
            .iter()
            .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(&spec.id));

        match idx {
            Some(i) => {
                let Some(m) = arr[i].as_object_mut() else {
                    continue;
                };
                if let Some(name) = &spec.name {
                    if m.get("name").and_then(|v| v.as_str()) != Some(name) {
                        m.insert("name".into(), Value::String(name.clone()));
                        changed = true;
                    }
                }
                if let Some(ctx) = spec.context_window {
                    if m.get("contextWindow").and_then(|v| v.as_u64()) != Some(ctx) {
                        m.insert("contextWindow".into(), Value::Number(ctx.into()));
                        changed = true;
                    }
                }
                if let Some(out) = spec.max_output_tokens {
                    if m.get("maxTokens").and_then(|v| v.as_u64()) != Some(out) {
                        m.insert("maxTokens".into(), Value::Number(out.into()));
                        changed = true;
                    }
                }
                if !spec.input_modalities.is_empty() {
                    let want: Vec<Value> = spec
                        .input_modalities
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect();
                    if m.get("input").and_then(|v| v.as_array()) != Some(&want) {
                        m.insert("input".into(), Value::Array(want));
                        changed = true;
                    }
                }
                if let Some(r) = spec.reasoning {
                    if m.get("reasoning").and_then(|v| v.as_bool()) != Some(r) {
                        m.insert("reasoning".into(), Value::Bool(r));
                        changed = true;
                    }
                }
                if !spec.reasoning_efforts.is_empty() {
                    let mut map = Map::new();
                    for level in ["off", "minimal", "low", "medium", "high", "xhigh", "max"] {
                        let val = if spec.reasoning_efforts.iter().any(|e| e == level) {
                            Value::String(level.to_string())
                        } else {
                            Value::Null
                        };
                        map.insert(level.to_string(), val);
                    }
                    let want = Value::Object(map);
                    if m.get("thinkingLevelMap") != Some(&want) {
                        m.insert("thinkingLevelMap".into(), want);
                        changed = true;
                    }
                }
            }
            None => {
                arr.push(make_pi_model(spec));
                changed = true;
            }
        }
    }

    changed
}

impl Injector for PiInjector {
    fn agent(&self) -> &'static str {
        "pi"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::auth_path(), Self::models_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let auth_path = Self::auth_path();
        let models_path = Self::models_path();

        // Pi 没装就跳过
        if !auth_path.exists() && !models_path.exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "pi"))
            .collect();

        // 母版为空时不做任何事，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 取消分配的：从 auth.json 和 models.json 里都摘掉

        // 母版是空的就什么都不做（空母版不等于"清空"）
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 取消分配的 provider：从 auth.json 和 models.json 里都摘掉
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "pi"))
            .collect();

        let mut plans = Vec::new();

        // ---- auth.json ----
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

        // 先移除未分配的。按 id 匹配 —— Pi 的 auth.json 键就是 provider id，
        // 而母版的 id 本来就从这里来。
        for p in &unassigned {
            if auth_obj.remove(p.id.as_str()).is_some() {
                auth_summary.push(format!("移除 {}（未分配给 pi）", p.name));
                auth_changed = true;
            }
        }

        for p in &wanted {
            let entry = auth_obj.get(p.id.as_str());

            match entry.and_then(|e| e.get("key")).and_then(|k| k.as_str()) {
                Some(existing) if existing == p.api_key => {
                    auth_summary.push(format!("{} 的 key 已一致", p.name));
                }
                Some(_) => {
                    // 只改 key，保留 type 等其他字段
                    if let Some(e) = auth_obj.get_mut(p.id.as_str()).and_then(|v| v.as_object_mut())
                    {
                        e.insert("key".into(), Value::String(p.api_key.clone()));
                        auth_summary.push(format!("更新 {} 的 key", p.name));
                        auth_changed = true;
                    }
                }
                None => {
                    let mut e = Map::new();
                    // Pi 的 type 是 "api_key"，注意和 opencode 的 "api" 不同
                    e.insert("type".into(), Value::String("api_key".into()));
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
            agent: "pi".to_string(),
            content: auth_content,
            pre_hash: auth_pre,
            post_hash: String::new(),
            changed: auth_changed,
            summary: auth_summary,
        });

        // ---- models.json ----
        let models_orig = if models_path.exists() {
            std::fs::read_to_string(&models_path)?
        } else {
            r#"{"providers":{}}"#.to_string()
        };
        let models_pre = hash_file(&models_path)?;
        let mut models: Value = serde_json::from_str(&models_orig).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                models_path.display(),
                e
            )
        })?;

        let root = models
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", models_path.display()))?;

        if !root.contains_key("providers") {
            root.insert("providers".into(), Value::Object(Map::new()));
        }
        let providers_map = root
            .get_mut("providers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 providers 不是对象", models_path.display()))?;

        let mut models_summary = Vec::new();
        let mut models_changed = false;

        // 先移除未分配的 provider 定义
        for p in &unassigned {
            if providers_map.remove(p.id.as_str()).is_some() {
                models_summary.push(format!("移除 {} 的定义（未分配给 pi）", p.name));
                models_changed = true;
            }
        }

        for p in &wanted {
            let new_def = to_pi_provider(p);

            match providers_map.get_mut(p.id.as_str()) {
                Some(existing) => {
                    let mut local_changed = false;

                    // baseUrl / api
                    let old_base = existing
                        .get("baseUrl")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let old_api = existing.get("api").and_then(|v| v.as_str()).unwrap_or("");
                    if old_base != p.base_url || old_api != pi_api(p.kind_for("pi")) {
                        existing
                            .as_object_mut()
                            .unwrap()
                            .insert("baseUrl".into(), Value::String(p.base_url.clone()));
                        existing.as_object_mut().unwrap().insert(
                            "api".into(),
                            Value::String(pi_api(p.kind_for("pi")).to_string()),
                        );
                        local_changed = true;
                    }

                    // models：按 id 逐个字段级合并。
                    // 重构前完全不覆盖，母版改了模型参数对已存在的 provider 无效。
                    if merge_pi_models(existing, &p.models) {
                        local_changed = true;
                    }

                    if local_changed {
                        models_summary.push(format!("更新 {} 的定义/模型", p.name));
                        models_changed = true;
                    } else {
                        models_summary.push(format!("{} 的定义已一致", p.name));
                    }
                }
                None => {
                    providers_map.insert(p.id.clone(), new_def);
                    models_summary.push(format!("新增 {} 到 models.json", p.name));
                    models_changed = true;
                }
            }
        }

        let models_content = if models_changed {
            let mut s = serde_json::to_string_pretty(&models)?;
            s.push('\n');
            s
        } else {
            models_orig
        };

        plans.push(FilePlan {
            path: contract_tilde(&models_path),
            agent: "pi".to_string(),
            content: models_content,
            pre_hash: models_pre,
            post_hash: String::new(),
            changed: models_changed,
            summary: models_summary,
        });

        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "pi"))
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
                    agent: "pi".to_string(),
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
