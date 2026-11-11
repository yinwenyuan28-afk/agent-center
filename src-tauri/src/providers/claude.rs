//! Claude Code 的 provider 注入器。
//!
//! ## 关键约束：Claude 只有一个槽位
//!
//! Claude Code 没有 provider 列表的概念，只有一组 `ANTHROPIC_*` 环境变量
//! 决定当前端点。所以这里**只能注入一个** provider，不能像其他 agent 那样并存。
//!
//! 选哪个？取 `agents` 里包含 `claude` 的**第一个** provider。
//! 顺序即优先级 —— 母版里的书写顺序就是用户的意图顺序。
//! 界面上的矩阵会保证同一时刻只有一个 provider 勾着 claude（单槽位互斥），
//! 所以这里的"第一个"在正常操作下唯一。
//!
//! ## 取消勾选时清掉哪些键
//!
//! 只有一种情况会清：当前 `ANTHROPIC_BASE_URL` 能对上母版里某个
//! **明确没分配给 claude** 的 provider。这时说明这套配置是我们写的、
//! 用户现在不要了，可以清掉接管的那些键。
//!
//! 对不上任何母版端点（订阅登录、用户手配的第三方）一律不动 ——
//! 清掉会让 Claude 彻底连不上，而那不是用户的意图。
//!
//! ## 所有权清单
//!
//! 只接管这些键，其余一律不碰：
//!
//! - `ANTHROPIC_BASE_URL`
//! - `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`（二选一，我们统一用前者）
//! - `ANTHROPIC_DEFAULT_*_MODEL` / `_MODEL_NAME`
//! - `ANTHROPIC_MODEL`
//!
//! **不碰**：`JAVA_HOME` 这类普通环境变量、`permissions`、`hooks`、
//! `includeCoAuthoredBy`、`statusLine`、`model` 顶层键。
//!
//! 这个清单是显式的，不是前缀猜测 —— CC Switch 的教训是
//! `CLAUDE_CODE_USE_` 下面有 `USE_POWERSHELL_TOOL` 这种功能开关，
//! 按前缀匹配会把用户的功能设置误删。

use super::model::{DiffAction, DiffLine, FilePlan, Provider};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct ClaudeInjector;

impl ClaudeInjector {
    fn settings_path() -> PathBuf {
        super::store::home_dir().join(".claude").join("settings.json")
    }
}

/// 本工具声明接管的键。不在这份清单里的一律不写、不删。
///
/// 注意 `ANTHROPIC_DEFAULT_*_MODEL_NAME` 这类也要列 ——
/// 它们和 `_MODEL` 成对出现，只改一个会让 /model 里显示名对不上。
const OWNED_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
];

impl Injector for ClaudeInjector {
    fn agent(&self) -> &'static str {
        "claude"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::settings_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::settings_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        // 母版是空的就什么都不做（避免把空母版理解成"清空"）
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;
        let mut root: Value = serde_json::from_str(&orig).map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                path.display(),
                e
            )
        })?;

        let root_obj = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

        // env 段不存在就建
        if !root_obj.contains_key("env") {
            root_obj.insert("env".into(), Value::Object(Map::new()));
        }
        let env = root_obj
            .get_mut("env")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 env 不是对象", path.display()))?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 当前生效的端点
        let current_base = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 母版里明确没分配给 claude 的端点
        let unassigned_urls: Vec<String> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "claude"))
            .map(|p| normalize_url(&p.base_url))
            .collect();

        // 当前配置是我们写的、而且用户现在不要了 → 清掉接管的键
        if !current_base.is_empty() && unassigned_urls.contains(&normalize_url(&current_base)) {
            for key in OWNED_KEYS.iter() {
                if env.remove(*key).is_some() {
                    changed = true;
                    summary.push(format!("移除 {key}（{current_base} 已取消分配给 claude）"));
                }
            }
            let content = {
                let mut s = serde_json::to_string_pretty(&root)?;
                s.push('\n');
                s
            };
            return Ok(vec![FilePlan {
                path: contract_tilde(&path),
                agent: "claude".to_string(),
                content,
                pre_hash: pre,
                post_hash: String::new(),
                changed,
                summary,
            }]);
        }

        // Claude 只有一个槽位，取第一个声明注入 claude 的 provider
        let Some(p) = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "claude"))
        else {
            // 没有任何 provider 分配给 claude，且当前端点不在母版里：
            // 那是用户自己的配置，不动
            return Ok(Vec::new());
        };

        // 逐键比对，只改不一致的
        let set = |key: &str, val: &str, env: &mut Map<String, Value>, summary: &mut Vec<String>, changed: &mut bool| {
            let old = env.get(key).and_then(|v| v.as_str()).unwrap_or("");
            if old != val {
                env.insert(key.to_string(), Value::String(val.to_string()));
                *changed = true;
                summary.push(format!("{key} 更新"));
            }
        };

        set("ANTHROPIC_BASE_URL", &p.base_url, env, &mut summary, &mut changed);

        // 统一用 AUTH_TOKEN（Bearer 方式）。同时清掉 API_KEY 避免两者并存冲突
        set("ANTHROPIC_AUTH_TOKEN", &p.api_key, env, &mut summary, &mut changed);
        if env.remove("ANTHROPIC_API_KEY").is_some() {
            changed = true;
            summary.push("移除 ANTHROPIC_API_KEY（统一用 AUTH_TOKEN）".to_string());
        }

        // 三个别名按序分配模型：opus ← [0]、sonnet ← [1]、haiku ← [2]。
        // 模型不足时回落，保证别名不会空着。
        // 重构前三个别名全指向 models[0]，导致 /model 里三个档位是同一个模型。
        if !p.models.is_empty() {
            let pick = |i: usize| -> &crate::providers::model::ModelSpec {
                if i < p.models.len() {
                    &p.models[i]
                } else {
                    p.models.last().unwrap()
                }
            };

            let slots = [
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", pick(0)),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", pick(1)),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", pick(2)),
            ];

            for (var, model) in slots {
                set(var, &model.id, env, &mut summary, &mut changed);
                let name_var = format!("{var}_NAME");
                set(&name_var, model.display_name(), env, &mut summary, &mut changed);
            }

            // 顶层 ANTHROPIC_MODEL 指向第一个
            set("ANTHROPIC_MODEL", &p.models[0].id, env, &mut summary, &mut changed);
        }

        if !changed {
            summary.push(format!("{} 已是最新", p.name));
        } else {
            summary.insert(0, format!("注入 {} 的配置", p.name));
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
            agent: "claude".to_string(),
            content,
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
            .find(|p| p.agents.iter().any(|a| a == "claude"));

        Ok(plans
            .into_iter()
            .map(|plan| {
                // 移除路径：说明里带「移除」前缀，标成 Update（清空也算改动）
                let removing = plan.summary.iter().any(|s| s.starts_with("移除"));
                let action = if !plan.changed {
                    DiffAction::Unchanged
                } else {
                    DiffAction::Update
                };
                let target = match &wanted {
                    Some(p) => p.id.clone(),
                    None => "（已取消分配）".to_string(),
                };
                let detail = if removing {
                    "当前端点的分配已取消，清掉接管的环境变量".to_string()
                } else if plan.changed {
                    "展开成 ANTHROPIC_* 环境变量（Claude 只支持单 provider）".to_string()
                } else {
                    "已一致".to_string()
                };
                DiffLine {
                    agent: "claude".to_string(),
                    path: plan.path.clone(),
                    action,
                    target,
                    detail,
                }
            })
            .collect())
    }
}

/// 归一化 URL 用于比对。忽略协议差异和尾部斜杠。
fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

/// 暴露给测试和界面：本工具接管的键列表。
pub fn owned_keys() -> &'static [&'static str] {
    OWNED_KEYS
}
