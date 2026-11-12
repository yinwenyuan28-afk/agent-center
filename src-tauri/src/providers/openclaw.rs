//! OpenClaw 的 provider 注入器。
//!
//! ## 按 cc-switch 移植，本机未安装未实测
//!
//! 本机没有 `~/.openclaw`。照着 cc-switch 的 `openclaw_config.rs` 写。
//!
//! ## 文件与结构
//!
//! `~/.openclaw/openclaw.json`（JSON5，允许注释）：
//!
//! ```jsonc
//! {
//!   "models": {
//!     "mode": "merge",
//!     "providers": {
//!       "<id>": {
//!         "baseUrl": "https://...",
//!         "apiKey": "sk-...",
//!         "api": "openai-completions",
//!         "models": [
//!           { "id": "...", "name": "...", "contextWindow": 1000000, "maxTokens": 128000 }
//!         ]
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! ## 累加式，不碰"当前模型"
//!
//! 多 provider 并存。真正的"当前模型"在
//! `agents.defaults.model.primary`（格式 `<provider>/<model>`），
//! 但那是用户的运行时选择 —— cc-switch 的供应商写入不碰它，只有单独的
//! "设默认模型"命令才改。本工具同样不改。
//!
//! ## 注释会丢
//!
//! cc-switch 用 json-five 的 round-trip AST 保留 JSON5 注释。本工具
//! 没有引入等价依赖，用 serde_json 读写 —— 带注释的文件会被剥掉注释
//! 后重写。这是已知差异，文档里标明。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub struct OpenClawInjector;

impl OpenClawInjector {
    fn config_path() -> PathBuf {
        super::store::home_dir()
            .join(".openclaw")
            .join("openclaw.json")
    }
}

/// 协议 → OpenClaw 的 `api` 字段。
fn openclaw_api(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChat => "openai-completions",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GoogleGenAi => "google-generative-ai",
    }
}

/// 构造一条模型条目。
///
/// 只写母版明确填了的元数据；`alias`、`cost`、`compat` 这些 OpenClaw
/// 自有结构没填就不写。
fn make_model(spec: &ModelSpec) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(spec.id));
    if let Some(name) = &spec.name {
        m.insert("name".into(), json!(name));
    }
    if let Some(ctx) = spec.context_window {
        m.insert("contextWindow".into(), json!(ctx));
    }
    if let Some(out) = spec.max_output_tokens {
        m.insert("maxTokens".into(), json!(out));
    }
    if !spec.input_modalities.is_empty() {
        // OpenClaw 的 preset 只用 text / image，和 Pi 同源。
        // 原样写 video / pdf 会和它自己的校验打架。
        let kept = spec.input_within(&["text", "image"]);
        if !kept.is_empty() {
            m.insert("input".into(), json!(kept));
        }
    }
    if let Some(r) = spec.reasoning {
        m.insert("reasoning".into(), json!(r));
    }
    Value::Object(m)
}

fn to_provider(p: &Provider) -> Value {
    let mut models = Vec::new();
    if p.models.is_empty() {
        models.push(make_model(&ModelSpec::new("default")));
    } else {
        for spec in &p.models {
            models.push(make_model(spec));
        }
    }
    json!({
        "baseUrl": p.base_url,
        "apiKey": p.api_key,
        "api": openclaw_api(p.kind_for("openclaw")),
        "models": models,
    })
}

impl Injector for OpenClawInjector {
    fn agent(&self) -> &'static str {
        "openclaw"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "openclaw"))
            .collect();
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "openclaw"))
            .collect();

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&strip_json5(&orig)).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                path.display(),
                e
            )
        })?;

        let before = root.clone();
        let obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

        // models.providers 是唯一写入点
        let models = obj.entry("models").or_insert_with(|| json!({}));
        let models_obj = models
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("models 不是对象"))?;
        if !models_obj.contains_key("providers") {
            models_obj.insert("providers".into(), json!({}));
        }
        let providers_map = models_obj
            .get_mut("providers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("models.providers 不是对象"))?;

        let mut summary = Vec::new();
        for p in &unassigned {
            if providers_map.remove(&p.id).is_some() {
                summary.push(format!("移除 {}（未分配给 openclaw）", p.id));
            }
        }
        for p in &wanted {
            providers_map.insert(p.id.clone(), to_provider(p));
            summary.push(format!("写入 {}", p.id));
        }

        let changed = root != before;
        let content = if changed {
            let mut s = serde_json::to_string_pretty(&root)?;
            s.push('\n');
            s
        } else {
            orig
        };
        if !changed {
            summary.push("已是最新".to_string());
        }

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "openclaw".to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        Ok(plans
            .into_iter()
            .map(|plan| DiffLine {
                agent: "openclaw".to_string(),
                path: plan.path.clone(),
                action: if plan.changed {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                },
                target: "models.providers".to_string(),
                detail: plan.summary.join("；"),
            })
            .collect())
    }
}

/// 去掉 JSON5 的注释和尾随逗号（最小实现，同 opencode 的那份）。
fn strip_json5(text: &str) -> String {
    super::opencode::strip_jsonc(text)
}
