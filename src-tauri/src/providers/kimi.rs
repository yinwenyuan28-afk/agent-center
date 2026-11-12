//! Kimi Code 的 provider 注入器。
//!
//! ## 按 cc-switch 移植，本机未安装未实测
//!
//! 本机没有 `~/.kimi-code`。照着 cc-switch 的 `kimi_config.rs` 写。
//!
//! ## 文件与结构
//!
//! `~/.kimi-code/config.toml`（`KIMI_CODE_HOME` 非空时改用它）：
//!
//! ```toml
//! default_model = "agentcenter-command"       # 选中模型别名
//!
//! [providers."agentcenter-command"]
//! type = "openai"                             # 见下表
//! base_url = "https://..."
//! api_key = "sk-..."
//!
//! [models."agentcenter-command"]
//! provider = "agentcenter-command"            # 必须与所属 provider 一致
//! model = "deepseek/deepseek-v4.1-flash"      # 上游真实模型名
//! max_context_size = 1000000                  # 必填，正整数
//! display_name = "DeepSeek V4.1 Flash"
//! ```
//!
//! ## 三个硬约束（cc-switch 的 validate_provider）
//!
//! 1. `provider.type` 只能是 `kimi` / `openai` / `openai_responses` /
//!    `anthropic` / `google-genai` / `vertexai`
//! 2. 密钥三选一：`api_key`、`api_key_env`、`oauth.key`。`api_key_env`
//!    不能和另外两个同时出现
//! 3. 每个 `[models.<alias>]` 的 `provider` 字段必须等于它所属的
//!    provider id；`model` 和 `max_context_size` 必填
//!
//! ## 一个 provider 一个模型别名
//!
//! Kimi 的 `models` 是**按别名平铺**的，别名和 provider 是多对一。
//! 本工具简化成「一个 provider 下所有模型共用一个别名」—— 因为
//! `default_model` 只指向一个别名，而模型选择是用户的运行时行为。
//! 写多个别名会让 `default_model` 指向哪个变得含糊。

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct KimiInjector;

impl KimiInjector {
    fn dir() -> PathBuf {
        if let Ok(v) = std::env::var("KIMI_CODE_HOME") {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
        super::store::home_dir().join(".kimi-code")
    }

    fn config_path() -> PathBuf {
        Self::dir().join("config.toml")
    }
}

/// 协议 → Kimi 的 `provider.type`。
fn kimi_type(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenaiChat => "openai",
        ProviderKind::OpenaiResponses => "openai_responses",
        ProviderKind::GoogleGenAi => "google-genai",
    }
}

/// 模型别名：provider id 加前缀，避免和用户自己的别名撞。
fn alias_for(p: &Provider) -> String {
    format!("agentcenter-{}", p.id)
}

impl Injector for KimiInjector {
    fn agent(&self) -> &'static str {
        "kimi"
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
            .filter(|p| p.agents.iter().any(|a| a == "kimi"))
            .collect();
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "kimi"))
            .collect();

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;
        let uses_crlf = orig.contains("\r\n");
        let had_trailing_newline = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = orig.parse().map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                path.display(),
                e
            )
        })?;

        let mut summary = Vec::new();

        // 在内存里算出最终文档，最后整体比对。
        //
        // 「删除时置 changed、写入时再置 changed」会让第二次同步先删后写
        // 同样的内容，永远报"有改动"，幂等就废了。
        let before = doc.clone();
        let mut touched = false;

        // ---- 删未分配的：provider 节点 + 它名下所有模型别名 ----
        for p in &unassigned {
            let alias = alias_for(p);
            let mut removed = false;

            if let Some(tbl) = doc.get_mut("providers").and_then(|i| i.as_table_mut()) {
                if tbl.remove(&p.id).is_some() {
                    removed = true;
                }
            }
            if let Some(tbl) = doc.get_mut("models").and_then(|i| i.as_table_mut()) {
                // 别名本身 + 任何 provider 字段指向它的别名
                let doomed: Vec<String> = tbl
                    .iter()
                    .filter(|(k, item)| {
                        *k == alias
                            || item
                                .get("provider")
                                .and_then(|v| v.as_str())
                                .map(|v| v == p.id)
                                .unwrap_or(false)
                    })
                    .map(|(k, _)| k.to_string())
                    .collect();
                for k in doomed {
                    tbl.remove(&k);
                    removed = true;
                }
            }

            if removed {
                summary.push(format!("移除 {} 及其模型别名", p.id));
                touched = true;
            }
        }

        // ---- 写已分配的 ----
        for p in &wanted {
            let alias = alias_for(p);
            let kind = p.kind_for("kimi");

            // 至少一个模型是硬要求；母版没填就用 provider id 兜底
            let (model_id, display, ctx) = match p.models.first() {
                Some(m) => (
                    m.id.clone(),
                    m.display_name().to_string(),
                    m.context_or_default(),
                ),
                None => (p.id.clone(), p.name.clone(), 1_000_000),
            };

            if !doc.contains_key("providers") {
                doc["providers"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let providers = doc["providers"]
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("providers 不是表"))?;

            // 整节点替换：type / base_url / api_key 三项归本工具管
            let mut t = toml_edit::Table::new();
            t["type"] = toml_edit::value(kimi_type(kind));
            t["base_url"] = toml_edit::value(p.base_url.clone());
            t["api_key"] = toml_edit::value(p.api_key.clone());
            providers.insert(&p.id, toml_edit::Item::Table(t));

            if !doc.contains_key("models") {
                doc["models"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let models = doc["models"]
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("models 不是表"))?;

            let mut m = toml_edit::Table::new();
            m["provider"] = toml_edit::value(&p.id);
            m["model"] = toml_edit::value(model_id);
            m["max_context_size"] = toml_edit::value(ctx as i64);
            m["display_name"] = toml_edit::value(display);
            models.insert(&alias, toml_edit::Item::Table(m));

            // default_model 不主动改 —— 那是用户当前在用的选择。
            // 只在它指向一个刚被删掉的别名时才纠正（见上一步的删除分支）。

            summary.push(format!("写入 providers.{} 与 models.{}", p.id, alias));
            touched = true;
        }

        // ---- 只在 default_model 悬空时纠正 ----
        if let Some(first) = wanted.first() {
            let current = doc
                .get("default_model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let dangling = !current.is_empty()
                && unassigned
                    .iter()
                    .any(|p| alias_for(p) == current || p.id == current);
            if dangling {
                doc["default_model"] = toml_edit::value(alias_for(first));
                touched = true;
                summary.push(format!("default_model → {}（原指向已取消分配的条目）", alias_for(first)));
            }
        }

        let mut content = doc.to_string();
        if uses_crlf {
            content = content.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if !had_trailing_newline {
            while content.ends_with('\n') || content.ends_with("\r\n") {
                if content.ends_with("\r\n") {
                    content.truncate(content.len() - 2);
                } else {
                    content.truncate(content.len() - 1);
                }
            }
        }
        // 整体比对：只有最终结果和原文不同才算改动
        let changed = touched && doc.to_string() != before.to_string();
        if !changed {
            summary.push("已是最新".to_string());
        }

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "kimi".to_string(),
            content: if changed { content } else { orig },
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
                agent: "kimi".to_string(),
                path: plan.path.clone(),
                action: if plan.changed {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                },
                target: "providers/models".to_string(),
                detail: plan.summary.join("；"),
            })
            .collect())
    }
}

/// 供测试用：暴露协议映射。
pub fn kimi_type_of(kind: ProviderKind) -> &'static str {
    kimi_type(kind)
}
