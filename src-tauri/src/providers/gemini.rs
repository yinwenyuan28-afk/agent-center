//! Gemini CLI 的 provider 注入器。
//!
//! ## 按 cc-switch 移植，本机未安装未实测
//!
//! 本机没有 `~/.gemini`，也没有 `gemini` CLI。下面这份实现是对照着
//! cc-switch 的 `gemini_config.rs` + `live/project/gemini.rs` +
//! `live/floor.rs` 写的。等装了之后再按真实文件核对。
//!
//! ## 两个文件
//!
//! | 文件 | 写什么 |
//! |---|---|
//! | `~/.gemini/.env` | API key、base_url、模型名，每行 `KEY=VALUE` |
//! | `~/.gemini/settings.json` | 只改 `security.auth.selectedType` 和 `model.name` |
//!
//! ## `.env` 的编辑规则（照搬 cc-switch）
//!
//! 1. 先删掉所有关键字段行（`GOOGLE_*` 整个前缀 + 几个点名的 `GEMINI_*`）
//! 2. 再写目标供应商的值
//! 3. 注释、空行、其余变量、`export` 前缀、行尾风格、结尾换行一律保留
//! 4. 同名变量重复出现时，保留第一处原位改写，其余删除
//!
//! `GEMINI_SANDBOX`、`GEMINI_CLI_HOME`、`GEMINI_SYSTEM_MD` 这类不是
//! 关键字段，留给用户。
//!
//! ## 为什么这是切换式
//!
//! Gemini CLI 只有一个 live 配置，没有 provider 列表。所以和 Claude /
//! Codex 一样是单槽位。

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct GeminiInjector;

impl GeminiInjector {
    fn dir() -> PathBuf {
        super::store::home_dir().join(".gemini")
    }

    fn env_path() -> PathBuf {
        Self::dir().join(".env")
    }

    fn settings_path() -> PathBuf {
        Self::dir().join("settings.json")
    }
}

/// `.env` 里这个键是不是关键字段。
///
/// `GOOGLE_*` 整个前缀都属于连接和鉴权。`GEMINI_*` 不能按前缀 ——
/// 同一前缀下有 `GEMINI_CLI_HOME`、`GEMINI_SANDBOX`、`GEMINI_SYSTEM_MD`
/// 这类与供应商无关的 CLI 设置。
fn is_floor_env(key: &str) -> bool {
    key.starts_with("GOOGLE_")
        || matches!(
            key,
            "GEMINI_API_KEY"
                | "GEMINI_MODEL"
                | "GEMINI_API_KEY_AUTH_MECHANISM"
                | "GEMINI_CLI_CUSTOM_HEADERS"
                | "GEMINI_DEFAULT_AUTH_TYPE"
                | "GEMINI_CLI_USE_COMPUTE_ADC"
                | "CODE_ASSIST_ENDPOINT"
                | "CODE_ASSIST_API_VERSION"
        )
}

/// 目标供应商要写进 `.env` 的键值。
///
/// 官方 Google 卡（没有自定义端点）写 `oauth-personal`，第三方写
/// `gemini-api-key`。本工具只处理第三方端点。
fn env_pairs(p: &Provider) -> Vec<(String, String)> {
    let mut pairs = vec![
        ("GOOGLE_GEMINI_BASE_URL".to_string(), p.base_url.clone()),
        ("GEMINI_API_KEY".to_string(), p.api_key.clone()),
    ];
    if let Some(first) = p.models.first() {
        pairs.push(("GEMINI_MODEL".to_string(), first.id.clone()));
    }
    // 第三方端点要让 Gemini CLI 用 x-goog-api-key 头
    pairs.push((
        "GEMINI_API_KEY_AUTH_MECHANISM".to_string(),
        "x-goog-api-key".to_string(),
    ));
    pairs
}

/// 解析 `.env` 的一行。返回 `(是否 export, key, value)`。
///
/// cc-switch 的规则：跳过空行和 `#`；剥掉 `export ` 前缀；键只允许
/// 字母数字下划线；值只 trim，不去引号。
fn parse_env_line(line: &str) -> Option<(bool, String, String)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let (exported, rest) = match t.strip_prefix("export ") {
        Some(r) => (true, r.trim_start()),
        None => (false, t),
    };
    let (k, v) = rest.split_once('=')?;
    let key = k.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((exported, key.to_string(), v.trim().to_string()))
}

impl Injector for GeminiInjector {
    fn agent(&self) -> &'static str {
        "gemini"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::env_path(), Self::settings_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let dir = Self::dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 单槽位：只取第一个声明注入 gemini 的 provider
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "gemini"));

        let mut plans = Vec::new();

        // ---- .env ----
        let env_path = Self::env_path();
        let before = if env_path.exists() {
            std::fs::read_to_string(&env_path)?
        } else {
            String::new()
        };
        let pre = hash_file(&env_path)?;
        let uses_crlf = before.contains("\r\n");
        let had_trailing_newline = before.ends_with('\n');

        let mut content = strip_floor_lines(&before);
        if let Some(p) = wanted {
            for (k, v) in env_pairs(p) {
                content = upsert_env(&content, &k, &v);
            }
        }
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

        let changed = content != before;
        let mut summary = Vec::new();
        if let Some(p) = wanted {
            summary.push(if changed {
                format!("写入 {} 的 key / 端点 / 模型", p.name)
            } else {
                format!("{} 已是最新", p.name)
            });
        } else {
            summary.push(if changed {
                "取消分配，清掉关键字段".to_string()
            } else {
                "已是最新".to_string()
            });
        }

        plans.push(FilePlan {
            path: contract_tilde(&env_path),
            agent: "gemini".to_string(),
            content: if changed { content } else { before },
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        });

        // ---- settings.json：只改两条路径 ----
        let settings_path = Self::settings_path();
        if settings_path.exists() {
            let orig = std::fs::read_to_string(&settings_path)?;
            let pre = hash_file(&settings_path)?;
            let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
                anyhow::anyhow!(
                    "{} 解析失败（{}），已中止，不会写入任何文件",
                    settings_path.display(),
                    e
                )
            })?;

            let before_root = root.clone();
            let obj = root
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", settings_path.display()))?;

            // selectedType：第三方写 gemini-api-key；取消分配时不动
            // （回官方要靠 Google 登录，不是本工具该替用户做的决定）
            if wanted.is_some() {
                let security = obj
                    .entry("security")
                    .or_insert_with(|| json!({}));
                let auth = security
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("security 不是对象"))?
                    .entry("auth")
                    .or_insert_with(|| json!({}));
                auth.as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("security.auth 不是对象"))?
                    .insert("selectedType".into(), json!("gemini-api-key"));
            }

            // model.name：第一个模型；没有就删掉该键
            if let Some(p) = wanted {
                let model_name = p.models.first().map(|m| m.id.clone());
                let model = obj.entry("model").or_insert_with(|| json!({}));
                if let Some(m) = model.as_object_mut() {
                    match model_name {
                        Some(name) => {
                            m.insert("name".into(), json!(name));
                        }
                        None => {
                            m.remove("name");
                        }
                    }
                }
            }

            let changed = root != before_root;
            let content = if changed {
                let mut s = serde_json::to_string_pretty(&root)?;
                s.push('\n');
                s
            } else {
                orig
            };
            plans.push(FilePlan {
                path: contract_tilde(&settings_path),
                agent: "gemini".to_string(),
                content,
                pre_hash: pre,
                post_hash: String::new(),
                changed,
                summary: vec![if changed {
                    "更新 security.auth.selectedType / model.name".to_string()
                } else {
                    "settings.json 已是最新".to_string()
                }],
            });
        }

        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "gemini"));

        Ok(plans
            .into_iter()
            .map(|plan| DiffLine {
                agent: "gemini".to_string(),
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

/// 删掉所有关键字段行，保留其余内容逐字不变。
fn strip_floor_lines(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        match parse_env_line(line) {
            Some((_, key, _)) if is_floor_env(&key) => continue,
            _ => out.push(line),
        }
    }
    // 末尾补换行由调用方按原文件风格决定
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 写入或原位替换一个变量。
///
/// 已有同名变量时保留第一处的位置和 `export ` 前缀，其余重复行删除。
/// 没有则在末尾追加。
fn upsert_env(text: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut found_at: Option<usize> = None;
    let mut exported = false;

    let mut i = 0;
    while i < lines.len() {
        match parse_env_line(&lines[i]) {
            Some((exp, k, _)) if k == key => {
                if found_at.is_none() {
                    found_at = Some(i);
                    exported = exp;
                    i += 1;
                } else {
                    // 重复变量：删掉后面那几处
                    lines.remove(i);
                }
            }
            _ => i += 1,
        }
    }

    let new_line = if exported {
        format!("export {key}={value}")
    } else {
        format!("{key}={value}")
    };

    match found_at {
        Some(idx) => lines[idx] = new_line,
        None => lines.push(new_line),
    }

    let mut s = lines.join("\n");
    if text.ends_with('\n') || text.is_empty() {
        s.push('\n');
    }
    s
}

/// 供测试用：暴露 `.env` 的关键字段判定。
pub fn is_owned_env_key(key: &str) -> bool {
    is_floor_env(key)
}

/// 供测试用：暴露协议映射（Gemini 没有协议字段，只校验类型）。
pub fn supports_kind(kind: ProviderKind) -> bool {
    // Gemini CLI 只认自家协议；第三方端点走 OpenAI 兼容层时
    // 由 Gemini 自己的 adapter 处理。这里不额外限制。
    matches!(
        kind,
        ProviderKind::GoogleGenAi
            | ProviderKind::OpenaiChat
            | ProviderKind::OpenaiResponses
            | ProviderKind::Anthropic
    )
}
