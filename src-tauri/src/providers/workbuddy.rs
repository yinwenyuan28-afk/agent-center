//! WorkBuddy 的 provider 注入器。
//!
//! 目标是 `~/.workbuddy/models.json`，结构是一个**扁平数组**：
//!
//! ```jsonc
//! [{
//!   "id": "deepseek/deepseek-v4.1-flash",
//!   "name": "...",
//!   "vendor": "Custom",
//!   "url": "https://api.commandcode.ai/provider/v1",
//!   "apiKey": "明文",
//!   "supportsToolCall": true,
//!   "supportsImages": false,
//!   "supportsReasoning": true,
//!   "useCustomProtocol": false,
//!   "maxInputTokens": 1000000,
//!   "reasoning": { "supportedEfforts": [...] }
//! }]
//! ```
//!
//! ## 关键：这是模型列表，不是 provider 列表
//!
//! 同一个 provider 的多个模型各占一条记录，`url` 和 `apiKey` 重复存了多遍。
//! 所以这里按「provider × model」展开：母版里一个 provider 有 N 个模型，
//! 就写 N 条记录，它们共享同一个 url 和 apiKey。
//!
//! ## 已有条目的处理
//!
//! 按 `id`（模型 id）匹配：
//! - 已存在且 url + apiKey 都一致 → 不动
//! - 已存在但 url/apiKey 不同 → 只改这两个字段，其他（supportsToolCall 等）保留
//! - 不存在 → 新增一条
//!
//! 母版没声明的模型一律不动 —— 用户可能手动加了别的模型。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct WorkBuddyInjector;

impl WorkBuddyInjector {
    fn models_path() -> PathBuf {
        super::store::home_dir()
            .join(".workbuddy")
            .join("models.json")
    }
}

/// 判断一个模型条目是否属于某个 provider（按 url 判断）。
fn entry_belongs_to(entry: &Value, base_url: &str) -> bool {
    entry
        .get("url")
        .and_then(|v| v.as_str())
        .map(|u| normalize_url(u) == normalize_url(base_url))
        .unwrap_or(false)
}

fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

/// 构造一条 WorkBuddy 模型记录。
///
/// 只写母版里明确填了的字段。`reasoning.supportedEfforts` 尤其重要 ——
/// 重构前写死 `["low","medium","high"]`，而真实文件里这台机器只有 `["high"]`，
/// 写错会让 UI 显示模型其实不支持的档位。
fn make_entry(p: &Provider, spec: &ModelSpec) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(spec.id.clone()));
    m.insert(
        "name".into(),
        Value::String(spec.display_name().to_string()),
    );
    m.insert("vendor".into(), Value::String("Custom".into()));
    m.insert("url".into(), Value::String(p.base_url.clone()));
    m.insert("apiKey".into(), Value::String(p.api_key.clone()));
    m.insert("supportsToolCall".into(), Value::Bool(spec.tool_call.unwrap_or(true)));
    m.insert(
        "supportsImages".into(),
        Value::Bool(spec.supports_images()),
    );
    m.insert(
        "supportsReasoning".into(),
        Value::Bool(spec.reasoning_or_infer()),
    );
    // useCustomProtocol: false 表示走标准协议；true 表示自定义。
    // Responses 协议需要 true，其他走 false。
    m.insert(
        "useCustomProtocol".into(),
        Value::Bool(matches!(p.kind_for("workbuddy"), ProviderKind::OpenaiResponses)),
    );
    m.insert(
        "maxInputTokens".into(),
        Value::Number(spec.context_or_default().into()),
    );

    // 推理档位：只有母版填了才写。没填就不写这个字段，
    // 让 WorkBuddy 用它自己的默认值，而不是我们猜的一组
    if !spec.reasoning_efforts.is_empty() {
        let mut reasoning = Map::new();
        reasoning.insert(
            "supportedEfforts".into(),
            Value::Array(
                spec.reasoning_efforts
                    .iter()
                    .map(|e| Value::String(e.clone()))
                    .collect(),
            ),
        );
        m.insert("reasoning".into(), Value::Object(reasoning));
    }

    Value::Object(m)
}

impl Injector for WorkBuddyInjector {
    fn agent(&self) -> &'static str {
        "workbuddy"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::models_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::models_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "workbuddy"))
            .collect();

        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 取消分配的：按 url 把属于它的模型记录全删掉。
        // 只删母版端点能对上的，用户手加的其他模型不受影响。
        let unassigned_urls: Vec<String> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "workbuddy"))
            .map(|p| normalize_url(&p.base_url))
            .collect();

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                path.display(),
                e
            )
        })?;

        let arr = root
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是数组", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 先移除未分配 provider 的模型记录。
        // 这里是扁平数组，按 url 判定归属，一次删掉该端点的所有模型。
        if !unassigned_urls.is_empty() {
            let before = arr.len();
            arr.retain(|e| {
                let url = e.get("url").and_then(|v| v.as_str()).unwrap_or("");
                !unassigned_urls.contains(&normalize_url(url))
            });
            let removed = before - arr.len();
            if removed > 0 {
                summary.push(format!("移除 {removed} 条模型记录（所属 provider 未分配给 workbuddy）"));
                changed = true;
            }
        }

        for p in &wanted {
            // 母版没给模型就用 provider id 当模型名兜底
            let specs: Vec<ModelSpec> = if p.models.is_empty() {
                vec![ModelSpec::new(p.id.clone())]
            } else {
                p.models.clone()
            };

            for spec in &specs {
                let model_id = &spec.id;
                // 先按 id 找，再按 url 校验
                let existing_idx = arr.iter().position(|e| {
                    e.get("id").and_then(|v| v.as_str()) == Some(model_id.as_str())
                });

                match existing_idx {
                    Some(idx) => {
                        let entry = arr[idx].as_object_mut().unwrap();
                        // 先把旧值复制成 String，否则 &str 会借用 entry，
                        // 后面 insert 的可变借用就冲突了
                        let old_url = entry
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let old_key = entry
                            .get("apiKey")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let mut local_changed = false;
                        if normalize_url(&old_url) != normalize_url(&p.base_url) {
                            entry.insert("url".into(), Value::String(p.base_url.clone()));
                            local_changed = true;
                        }
                        if old_key != p.api_key {
                            entry.insert("apiKey".into(), Value::String(p.api_key.clone()));
                            local_changed = true;
                        }

                        // 能力元数据：只覆盖母版填了的字段
                        if let Some(n) = &spec.name {
                            if entry.get("name").and_then(|v| v.as_str()) != Some(n) {
                                entry.insert("name".into(), Value::String(n.clone()));
                                local_changed = true;
                            }
                        }
                        if let Some(ctx) = spec.context_window {
                            if entry.get("maxInputTokens").and_then(|v| v.as_u64()) != Some(ctx) {
                                entry.insert("maxInputTokens".into(), Value::Number(ctx.into()));
                                local_changed = true;
                            }
                        }
                        if let Some(tc) = spec.tool_call {
                            if entry.get("supportsToolCall").and_then(|v| v.as_bool()) != Some(tc)
                            {
                                entry.insert("supportsToolCall".into(), Value::Bool(tc));
                                local_changed = true;
                            }
                        }
                        if !spec.input_modalities.is_empty() {
                            let img = spec.supports_images();
                            if entry.get("supportsImages").and_then(|v| v.as_bool()) != Some(img) {
                                entry.insert("supportsImages".into(), Value::Bool(img));
                                local_changed = true;
                            }
                        }
                        if spec.reasoning.is_some() || !spec.reasoning_efforts.is_empty() {
                            if let Some(r_on) = spec.reasoning.clone() {
                                if entry.get("supportsReasoning").and_then(|v| v.as_bool())
                                    != Some(r_on)
                                {
                                    entry.insert("supportsReasoning".into(), Value::Bool(r_on));
                                    local_changed = true;
                                }
                            }
                            if !spec.reasoning_efforts.is_empty() {
                                let want: Vec<Value> = spec
                                    .reasoning_efforts
                                    .iter()
                                    .map(|e| Value::String(e.clone()))
                                    .collect();
                                let cur = entry
                                    .get("reasoning")
                                    .and_then(|r| r.get("supportedEfforts"))
                                    .and_then(|v| v.as_array());
                                if cur != Some(&want) {
                                    let mut reasoning = Map::new();
                                    reasoning.insert("supportedEfforts".into(), Value::Array(want));
                                    entry.insert(
                                        "reasoning".into(),
                                        Value::Object(reasoning),
                                    );
                                    local_changed = true;
                                }
                            }
                        }

                        if local_changed {
                            summary.push(format!("更新模型 {model_id} 的 url/apiKey/能力"));
                            changed = true;
                        } else {
                            summary.push(format!("模型 {model_id} 已是最新"));
                        }
                    }
                    None => {
                        arr.push(make_entry(p, spec));
                        summary.push(format!("新增模型 {model_id}"));
                        changed = true;
                    }
                }
            }

            // 同一个 provider 的其他模型：如果 url 相同但 id 不在母版列表里，
            // 只更新 key（因为它们的 key 是同一个，应该一致）
            //
            // 注意：这里要先收集索引再改，不能边遍历边可变借用
            let same_provider_idx: Vec<usize> = arr
                .iter()
                .enumerate()
                .filter(|(_, e)| entry_belongs_to(e, &p.base_url))
                .map(|(i, _)| i)
                .collect();

            for idx in same_provider_idx {
                if let Some(obj) = arr[idx].as_object_mut() {
                    let old_key = obj.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
                    if old_key != p.api_key {
                        obj.insert("apiKey".into(), Value::String(p.api_key.clone()));
                        changed = true;
                    }
                }
            }
        }

        let content = if changed {
            let mut s = serde_json::to_string_pretty(&root)?;
            s.push('\n');
            s
        } else {
            orig
        };

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "workbuddy".to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "workbuddy"))
            .collect();

        let mut out = Vec::new();
        for plan in &plans {
            for p in &wanted {
                let detail = plan
                    .summary
                    .iter()
                    .find(|s| s.contains(&p.id) || p.models.iter().any(|m| s.contains(&m.id)))
                    .cloned()
                    .unwrap_or_else(|| {
                        if plan.changed {
                            format!("{} 有改动", p.name)
                        } else {
                            "已一致".to_string()
                        }
                    });

                let action = if !plan.changed {
                    DiffAction::Unchanged
                } else if detail.starts_with("新增") {
                    DiffAction::Add
                } else {
                    DiffAction::Update
                };

                out.push(DiffLine {
                    agent: "workbuddy".to_string(),
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
