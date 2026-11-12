//! Grok Build CLI 的 provider 注入器。
//!
//! ## 按 cc-switch 移植，本机未安装未实测
//!
//! 本机没有 `~/.grok`。下面照着 cc-switch 的 `grok_config.rs` +
//! `live/project/grok.rs` 写，装好后再按真实文件核对。
//!
//! ## 文件与结构
//!
//! `~/.grok/config.toml`：
//!
//! ```toml
//! [models]
//! default = "cc-switch-grok"       # 指向下面那张表
//! session_summary = "cc-switch-grok"
//!
//! [model."cc-switch-grok"]
//! model = "x-ai/grok-4.7"          # 发给上游的真实模型名
//! base_url = "https://..."
//! name = "显示名"
//! api_key = "sk-..."               # 与 env_key 二选一
//! api_backend = "responses"
//! context_window = 500000
//! ```
//!
//! ## 三个特殊性
//!
//! 1. **整表替换**：不按字段合并，避免残留上一家的 `env_key` / 模型名
//! 2. **按上次写入记录删旧表**，不按当前 `models.default` 找 ——
//!    Grok 自己的 `/settings` 会把 `default` 改成内置模型
//! 3. `models.session_summary` 也要跟着指向新表，否则 Grok 会用内置
//!    `grok-4.6` 打第三方上游，直接失败
//!
//! ## 为什么表名固定
//!
//! cc-switch 用固定表名 + 记录文件来识别"是自己写的"。本工具用
//! `agentcenter-grok` 这个固定表名，写之前只删这个名字的表；
//! 用户自己起的其他 `[model.*]` 表不动。

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct GrokInjector;

/// 本工具写入的表名。固定值，便于识别归属。
const TABLE: &str = "agentcenter-grok";

impl GrokInjector {
    fn config_path() -> PathBuf {
        super::store::home_dir().join(".grok").join("config.toml")
    }
}

/// 协议 → Grok 的 `api_backend`。
///
/// Grok 后端只校验"非空字符串"，已知取值是 `responses` 和
/// `chat_completions`。
fn api_backend(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenaiResponses => "responses",
        _ => "chat_completions",
    }
}

impl Injector for GrokInjector {
    fn agent(&self) -> &'static str {
        "grok"
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

        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "grok"));

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
        // 不能「删除时置 changed、写入时再置 changed」—— 那样第二次同步
        // 会先删再写同样的表，永远报"有改动"，幂等就废了。
        let before = doc.clone();

        // 先把我们的旧表整张删掉（整表替换，不合并）
        if let Some(models_tbl) = doc.get_mut("model").and_then(|i| i.as_table_mut()) {
            models_tbl.remove(TABLE);
            if models_tbl.is_empty() {
                doc.as_table_mut().remove("model");
            }
        }

        if let Some(p) = wanted {
            let model = p.models.first();
            let model_id = model
                .map(|m| m.id.clone())
                .unwrap_or_else(|| p.id.clone());
            let context = model.and_then(|m| m.context_window).unwrap_or(500_000);
            let name = p.name.clone();
            let base_url = p.base_url.clone();
            let api_key = p.api_key.clone();
            let backend = api_backend(p.kind_for("grok"));

            // ---- [model."agentcenter-grok"] ----
            if !doc.contains_key("model") {
                doc["model"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let models_tbl = doc["model"]
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("model 不是表"))?;

            let mut t = toml_edit::Table::new();
            t["model"] = toml_edit::value(model_id);
            t["base_url"] = toml_edit::value(base_url);
            t["name"] = toml_edit::value(name);
            t["api_key"] = toml_edit::value(api_key);
            t["api_backend"] = toml_edit::value(backend);
            t["context_window"] = toml_edit::value(context as i64);
            models_tbl.insert(TABLE, toml_edit::Item::Table(t));

            // ---- [models] ----
            if !doc.contains_key("models") {
                doc["models"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let sel = doc["models"]
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("models 不是表"))?;
            sel["default"] = toml_edit::value(TABLE);
            // session_summary 不跟着改的话，Grok 会用内置 grok-4.6 打第三方端点
            sel["session_summary"] = toml_edit::value(TABLE);
        }

        // 整体比对：只有最终结果和原文不同才算改动
        let changed = doc.to_string() != before.to_string();
        if changed {
            summary.push(format!("写入 [model.\"{TABLE}\"] 与 models.default"));
        } else {
            summary.push("已是最新".to_string());
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

        if !changed {
            summary.push("已是最新".to_string());
        }

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "grok".to_string(),
            content: if changed { content } else { orig },
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "grok"));
        Ok(plans
            .into_iter()
            .map(|plan| DiffLine {
                agent: "grok".to_string(),
                path: plan.path.clone(),
                action: if plan.changed {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                },
                target: wanted
                    .map(|p| p.id.clone())
                    .unwrap_or_else(|| "（已取消分配）".into()),
                detail: plan.summary.first().cloned().unwrap_or_default(),
            })
            .collect())
    }
}

/// 供测试用：暴露协议映射。
pub fn api_backend_of(kind: ProviderKind) -> &'static str {
    api_backend(kind)
}
