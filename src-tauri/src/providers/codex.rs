//! Codex 的 provider 注入器。
//!
//! ## 和 cc-switch 对齐后的行为
//!
//! Codex 一次只有一家第三方供应商生效：
//!
//! - 第三方统一写成一张表 `[model_providers.custom]`
//! - 密钥写在表的 `experimental_bearer_token` 里（Codex 0.149 起自定义
//!   provider 不再读 `auth.json` 里的密钥；`env_key` 仍然可用，但为了跟
//!   cc-switch 的盘上形状一致，这里统一用 bearer token）
//! - 顶层 `model_provider = "custom"`，`model` 取这一家的第一个模型
//!
//! ## 为什么不再生成 `~/.agents/codex-env.ps1`
//!
//! 早期实现用 `env_key` + 环境变量脚本，要求用户每次开 Codex 前
//! `. ~/.agents/codex-env.ps1`。cc-switch 的做法是把 key 直接放进
//! provider 表，用户不需要额外步骤。两套并存时以 bearer token 为准，
//! 旧脚本不再由本工具维护 —— 里面的旧变量不会影响新表（表里已没有
//! `env_key`）。
//!
//! ## 保留用户的东西
//!
//! 只改自己声明管的键：
//!
//! - `model_providers.custom` 整张表
//! - 顶层 `model_provider` / `model` / `model_reasoning_effort`
//! - 切换时清掉上一家留下的顶层兜底键（`base_url` / `wire_api` /
//!   `experimental_bearer_token` / `disable_response_storage`）
//!
//! `mcp_servers`、`projects`、`plugins`、`notify`、`service_tier` 等
//! 用户键一律不动。
//!
//! ## TOML 写入的两个坑
//!
//! 1. **`model_provider` / `model` 必须写在任何 `[table]` 之前** ——
//!    一旦落到表头之后，TOML 语义上就变成那个表的字段了
//! 2. **`toml_edit` 不保留 CRLF**，需要自己检测并恢复

use super::model::{DiffAction, DiffLine, FilePlan, Provider};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct CodexInjector;

/// 第三方统一用的表名。和 cc-switch 保持一致。
const ROUTE_ID: &str = "custom";

impl CodexInjector {
    fn config_path() -> PathBuf {
        super::store::home_dir().join(".codex").join("config.toml")
    }

    fn auth_path() -> PathBuf {
        super::store::home_dir().join(".codex").join("auth.json")
    }
}

/// `auth.json` 里有没有官方登录。
///
/// 决定 `requires_openai_auth`：
/// - 有登录 → `true`（Codex 才会显示账号信息、刷新 token）
/// - 没有登录 → `false`（写成 `true` 会让 Codex 卡在登录页）
fn login_on_disk() -> bool {
    let path = CodexInjector::auth_path();
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    json.get("tokens").map(|t| !t.is_null()).unwrap_or(false)
}

impl Injector for CodexInjector {
    fn agent(&self) -> &'static str {
        "codex"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let cfg_path = Self::config_path();
        if !cfg_path.exists() {
            return Ok(Vec::new());
        }

        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 单槽位：只取第一个声明注入 codex 的 provider。
        // 界面上的矩阵会保证只有一个（single_slot），这里再兜一层。
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "codex"));

        // 母版里有、但没分配给 codex 的：如果是我们以前按 id 写进去的表，
        // 要清掉；顶层 model_provider 指向它时一起清。
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "codex"))
            .collect();

        // 检测原文件的行尾风格和末尾换行 —— toml_edit 都不保留
        let orig = std::fs::read_to_string(&cfg_path)?;
        let pre = hash_file(&cfg_path)?;
        let uses_crlf = orig.contains("\r\n");
        let had_trailing_newline = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = orig.parse().map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                cfg_path.display(),
                e
            )
        })?;

        let mut summary = Vec::new();
        let mut changed = false;

        // ---- 清理旧形态：早期实现按母版 id 建的表 ----
        //
        // 判定标准：表名是母版里出现过的 provider id，且不是 `custom`。
        // 这类表只可能是旧版本工具写的（用户自己起的 id 不会和母版撞名）。
        // 不清的话，`[model_providers.command]` 会一直带着旧的
        // `env_key = "COMMAND_API_KEY"` 留在文件里，和新的 custom 表并存。
        if let Some(tables) = doc.get_mut("model_providers").and_then(|i| i.as_table_mut()) {
            let doomed: Vec<String> = tables
                .iter()
                .filter(|(id, _)| {
                    *id != ROUTE_ID && providers.iter().any(|p| p.id.as_str() == *id)
                })
                .map(|(id, _)| id.to_string())
                .collect();
            for id in doomed {
                tables.remove(&id);
                summary.push(format!("移除旧 provider 表 {id}（单槽位已统一到 {ROUTE_ID}）"));
                changed = true;
            }
        }

        match wanted {
            Some(p) => {
                // 顶层选路
                if doc.get("model_provider").and_then(|v| v.as_str()) != Some(ROUTE_ID) {
                    doc["model_provider"] = toml_edit::value(ROUTE_ID);
                    changed = true;
                    summary.push(format!("model_provider → {ROUTE_ID}"));
                }

                // 模型：取第一个，并写推理档位
                if let Some(model) = p.models.first() {
                    let cur = doc.get("model").and_then(|v| v.as_str()).unwrap_or("");
                    if cur != model.id {
                        doc["model"] = toml_edit::value(model.id.clone());
                        changed = true;
                        summary.push(format!("model → {}", model.id));
                    }
                    if let Some(effort) = &model.default_effort {
                        let cur = doc
                            .get("model_reasoning_effort")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if cur != effort {
                            doc["model_reasoning_effort"] = toml_edit::value(effort.clone());
                            changed = true;
                            summary.push(format!("model_reasoning_effort → {effort}"));
                        }
                    }
                }

                // `[model_providers.custom]`
                let table_changed = write_custom_table(&mut doc, p, &mut summary)?;
                changed |= table_changed;

                // 上一家留下的顶层兜底键：一并清掉，否则 Codex 可能走错端点
                for key in [
                    "base_url",
                    "wire_api",
                    "experimental_bearer_token",
                    "disable_response_storage",
                ] {
                    if doc.remove(key).is_some() {
                        changed = true;
                        summary.push(format!("清除顶层遗留 {key}"));
                    }
                }
            }
            None => {
                // 一个都没分配给 codex：把 model_provider 指向我们那张表时清掉，
                // 否则 Codex 启动会报找不到 provider。
                let current = doc
                    .get("model_provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ours = current == ROUTE_ID
                    || unassigned.iter().any(|p| p.id == current);
                if ours {
                    if doc.remove("model_provider").is_some() {
                        changed = true;
                        summary.push(format!("清除 model_provider（{current} 已取消分配）"));
                    }
                }
                // 我们那张表里不再留真实 key
                if let Some(t) = doc
                    .get_mut("model_providers")
                    .and_then(|i| i.as_table_mut())
                    .and_then(|t| t.get_mut(ROUTE_ID))
                    .and_then(|i| i.as_table_mut())
                {
                    if t.remove("experimental_bearer_token").is_some() {
                        changed = true;
                        summary.push("移除 custom 表里的旧密钥".to_string());
                    }
                }
            }
        }

        // 序列化并恢复行尾风格
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

        let content = if changed { content } else { orig };
        if !changed {
            summary.push("已是最新".to_string());
        }

        Ok(vec![FilePlan {
            path: contract_tilde(&cfg_path),
            agent: "codex".to_string(),
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
            .find(|p| p.agents.iter().any(|a| a == "codex"));

        Ok(plans
            .into_iter()
            .map(|plan| {
                let action = if !plan.changed {
                    DiffAction::Unchanged
                } else {
                    DiffAction::Update
                };
                let target = wanted.map(|p| p.id.clone()).unwrap_or_else(|| "（已取消分配）".into());
                let detail = if plan.changed {
                    plan.summary.first().cloned().unwrap_or_default()
                } else {
                    "已一致".to_string()
                };
                DiffLine {
                    agent: "codex".to_string(),
                    path: plan.path.clone(),
                    action,
                    target,
                    detail,
                }
            })
            .collect())
    }
}

/// 写 `[model_providers.custom]`。已存在时只改本工具声明管的键，
/// 表里的 `http_headers`、`query_params` 等用户键保留。
fn write_custom_table(
    doc: &mut toml_edit::DocumentMut,
    p: &Provider,
    summary: &mut Vec<String>,
) -> anyhow::Result<bool> {
    if !doc.contains_key("model_providers") {
        doc["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let providers = doc["model_providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("model_providers 不是表"))?;

    let mut changed = false;
    if !providers.contains_key(ROUTE_ID) {
        providers.insert(ROUTE_ID, toml_edit::Item::Table(toml_edit::Table::new()));
        changed = true;
    }
    let t = providers
        .get_mut(ROUTE_ID)
        .and_then(|i| i.as_table_mut())
        .ok_or_else(|| anyhow::anyhow!("model_providers.custom 不是表"))?;

    // 表里的键全部归本工具管：这是第三方路由表，不是用户表。
    // 但 `http_headers` / `query_params` 这类鉴权/路由扩展如果存在，
    // 保留它们 —— 用户可能给上游加了自定义头。
    for (key, value) in [
        ("name", p.name.clone()),
        ("base_url", p.base_url.clone()),
    ] {
        if t.get(key).and_then(|v| v.as_str()) != Some(value.as_str()) {
            t[key] = toml_edit::value(value);
            changed = true;
        }
    }

    // wire_api 只接受 responses。母版标了别的协议也强制写，
    // 但给出警告 —— 静默改写会让用户以为请求格式对上了。
    if t.get("wire_api").and_then(|v| v.as_str()) != Some("responses") {
        t["wire_api"] = toml_edit::value("responses");
        changed = true;
    }
    if p.kind_for("codex") != super::model::ProviderKind::OpenaiResponses
        && p.has_agent_kind("codex")
    {
        summary.push(format!(
            "{} 在 codex 下标的是 {}，但 Codex 只支持 responses，已强制写成 responses",
            p.id,
            p.kind_for("codex").label()
        ));
    }

    // 密钥内联。cc-switch 的盘上形状；Codex 0.149 起自定义 provider
    // 不再从 auth.json 读 key。
    if t.get("experimental_bearer_token").and_then(|v| v.as_str()) != Some(p.api_key.as_str()) {
        t["experimental_bearer_token"] = toml_edit::value(p.api_key.clone());
        changed = true;
    }

    // env_key 是旧形态，写 new key 时移除，避免两个凭据来源并存
    if t.remove("env_key").is_some() {
        changed = true;
    }

    // requires_openai_auth 决定 Codex 走不走 ChatGPT 后端：
    // - true  → 带着官方登录去请求，官方镜像 / 需要账号信息时用
    // - false → 只用 provider 表里的 bearer token，完全绕过 ChatGPT 后端
    //
    // 母版给了明确值就以母版为准；没给（None）才按盘上有没有登录自动判断。
    // 自动判断在两种场景下会选错，所以必须留手动入口：
    // 装了 Codex 但想强制用第三方 key、或登录态还在但临时切第三方。
    let want = p.codex_requires_openai_auth.unwrap_or_else(login_on_disk);
    if t.get("requires_openai_auth").and_then(|v| v.as_bool()) != Some(want) {
        t["requires_openai_auth"] = toml_edit::value(want);
        changed = true;
    }
    summary.push(format!(
        "requires_openai_auth = {want}（{}）",
        if p.codex_requires_openai_auth.is_some() {
            "母版指定"
        } else if want {
            "盘上有登录，自动"
        } else {
            "盘上无登录，自动"
        }
    ));

    if changed {
        summary.push(format!("写入 model_providers.{ROUTE_ID}（bearer token 内联）"));
    } else {
        summary.push(format!("model_providers.{ROUTE_ID} 已是最新"));
    }
    Ok(changed)
}
