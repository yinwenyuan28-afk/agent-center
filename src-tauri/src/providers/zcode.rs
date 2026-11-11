//! ZCode 的 provider 注入器。
//!
//! 目标是 `~/.zcode/v2/config.json` 的 `provider` map。
//!
//! ## 结构要点
//!
//! ```jsonc
//! "provider": {
//!   "<uuid>": {
//!     "name": "...",
//!     "kind": "anthropic" | "openai-compatible",
//!     "options": { "apiKey": "...", "baseURL": "https://...", "apiKeyRequired": true },
//!     "source": "custom",
//!     "models": { "<modelId>": { "limit": {...}, "modalities": {...} } }
//!   }
//! }
//! ```
//!
//! ## 存量的 provider 怎么处理
//!
//! 分配关系存在母版的 `agents` 字段里，所以注入器必须两个方向都执行：
//!
//! - 母版声明的 provider：存在就更新 `options.apiKey` / `options.baseURL`，
//!   不存在就新增
//! - 母版里有、但**没**分配给 zcode 的 provider：从配置里移除
//! - 母版里根本没有的 provider（用户手动配的、内置模板）：**保持原样不动**
//!
//! 三个方向靠 base_url 识别。ZCode 的键是 UUID，不能拿母版的 id 直接匹配，
//! 所以这里用端点当身份 —— 同一个端点就是同一个 provider。
//!
//! ## 为什么 models 不能为空
//!
//! ZCode 界面上 provider 的模型列表来自这个 map。空的 `models` 会让
//! 该 provider 在 UI 里无模型可选，等于配了个用不了的 provider。
//! 所以母版的 `models` 为空时，我们给一个占位模型，并在 summary 里提示。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct ZCodeInjector;

impl ZCodeInjector {
    fn config_path() -> PathBuf {
        // 和母版用同一个 home 解析，保证测试时能整体重定向
        super::store::home_dir()
            .join(".zcode")
            .join("v2")
            .join("config.json")
    }
}

/// 把统一的 provider 翻译成 ZCode 的 provider 对象。
fn to_zcode_provider(p: &Provider) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(p.name.clone()));
    // ZCode 只有两档 kind：anthropic / openai-compatible。
    // 各协议按这个词表折叠：四种协议里只有 Anthropic 单独一类，其余都是 openai 系
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
    obj.insert("options".into(), Value::Object(options));

    obj.insert("source".into(), Value::String("custom".into()));

    // models：母版给的是模型规格列表，ZCode 需要完整对象。
    // 母版填了元数据就用母版的，没填的字段走默认值。
    let mut models = Map::new();
    if p.models.is_empty() {
        // 至少给一个占位，否则 UI 里无模型可选
        models.insert("default".into(), make_model_entry(&ModelSpec::new("default")));
    } else {
        for spec in &p.models {
            models.insert(spec.id.clone(), make_model_entry(spec));
        }
    }
    obj.insert("models".into(), Value::Object(models));

    Value::Object(obj)
}

/// 构造一个 ZCode 模型条目。
///
/// 只写母版里**明确填了**的字段和 ZCode 的必填结构。
/// `reasoning` 和 `zcode` 都是可选的，不写会让 ZCode 用默认行为，反而更安全。
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

    // 推理：只有母版明确声明时才写，避免给不支持的模型加上推理开关
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

impl Injector for ZCodeInjector {
    fn agent(&self) -> &'static str {
        "zcode"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::config_path();
        if !path.exists() {
            // ZCode 没装，不生成计划
            return Ok(Vec::new());
        }

        // 只处理母版里的全部 provider —— 分配了的写入，没分配的移除
        let all = providers;
        if all.is_empty() {
            return Ok(Vec::new());
        }

        // 第 1 道闸：解析失败即全停
        let original = std::fs::read_to_string(&path)?;
        let pre_hash = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&original)
            .map_err(|e| anyhow::anyhow!("{} 解析失败（{}），已中止，不会写入任何文件", path.display(), e))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 拿到 provider map，没有就建
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

        // ---- 第一步：移除「母版里有、但没分配给 zcode」的条目 ----
        //
        // 只删母版端点能对上的（池子内的）。内置模板、用户手配的端点
        // 在母版里找不到，一律不碰。
        let unassigned_urls: Vec<String> = all
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

        let wanted: Vec<&Provider> = all
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "zcode"))
            .collect();

        for p in &wanted {
            // 用 base_url 找已有条目（ZCode 的键是 UUID，不能用 id 直接匹配）
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
                    // 更新：只改 apiKey 和 baseURL，保留原有的 models 和其他字段。
                    // 这是"关键字段替换"，不是整块覆盖。
                    let entry = provider_map.get_mut(&key).unwrap();
                    let mut updated = false;

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
                    }

                    // 模型：按 id 逐个字段级合并。
                    //
                    // 重构前只在 models 为空时补齐 —— 母版改了模型参数
                    // 对已存在的 provider 完全不生效。现在改成：
                    // 母版里填了的字段覆盖，没填的保持 ZCode 里的原值。
                    let merged = merge_models_into(
                        entry.get_mut("models").and_then(|m| m.as_object_mut()),
                        &p.models,
                    );
                    if merged {
                        summary.push(format!("更新 {} 的模型元数据", p.name));
                        updated = true;
                    }

                    if updated {
                        summary.push(format!("更新 {} 的 key/baseURL/models", p.name));
                        changed = true;
                    } else {
                        summary.push(format!("{} 已是最新", p.name));
                    }
                }
                None => {
                    // 新增：用母版的 id 作为键。如果这个 id 已被占用
                    // （比如同名但不同 base_url），加后缀避免覆盖。
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

        // 序列化。保留原有键顺序（preserve_order 已开）
        let content = if changed {
            // 用 2 空格缩进，和 ZCode 自己写出的格式一致
            let mut s = serde_json::to_string_pretty(&root)?;
            s.push('\n');
            s
        } else {
            original
        };

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "zcode".to_string(),
            content,
            pre_hash,
            post_hash: String::new(),
            changed,
            summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        for plan in plans {
            // 逐个 provider 报告，而不是整份文件一条
            let wanted: Vec<&Provider> = providers
                .iter()
                .filter(|p| p.agents.iter().any(|a| a == "zcode"))
                .collect();

            for p in wanted {
                // 从 summary 里找对应的那一条
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

fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

/// 把母版里的模型元数据按 id 合并进 ZCode 的 models map。
///
/// 三条规则：
/// - 母版有、配置里没有 → 新增整条
/// - 两边都有 → 只覆盖母版里**明确填了**的字段（limit / modalities / reasoning）
/// - 配置里有、母版没列 → 保留（ZCode 的 deletedModels 机制不在我们管理范围）
///
/// 返回是否发生了改动。
fn merge_models_into(map: Option<&mut Map<String, Value>>, specs: &[ModelSpec]) -> bool {
    let Some(map) = map else {
        return false;
    };
    let mut changed = false;

    for spec in specs {
        match map.get_mut(&spec.id).and_then(|v| v.as_object_mut()) {
            Some(entry) => {
                // limit.context / limit.output
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

                // modalities
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

                // reasoning
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

                // 展示名
                if let Some(name) = &spec.name {
                    if entry.get("name").and_then(|v| v.as_str()) != Some(name) {
                        entry.insert("name".into(), Value::String(name.clone()));
                        changed = true;
                    }
                }
            }
            None => {
                // 母版有、配置里没有 → 新增整条
                map.insert(spec.id.clone(), make_model_entry(spec));
                changed = true;
            }
        }
    }

    changed
}
