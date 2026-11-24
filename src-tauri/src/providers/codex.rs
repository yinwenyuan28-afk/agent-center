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

/// Codex 接受的推理档位。
///
/// 这些值会被原样放进请求体的 `reasoning.effort`，上游是封闭枚举：
/// 传别的字符串直接 400（实测传 `enabled` 报
/// `Invalid option: expected one of "none"|"minimal"|"low"|...`）。
///
/// 母版的档位词表可能更大也可能更小 —— 各家中转自己的写法不统一，
/// CommandCode 的 mimo 就写 `enabled` / `off`。所以注入前必须收敛。
const EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// 这个档位能不能写进 `config.toml`。
///
/// 见 [`EFFORTS`] 的说明：母版的档位词表和 Codex 的不一致，写错的后果
/// 是每次请求都被上游 400，而不是「档位被忽略」。
pub fn is_codex_effort(v: &str) -> bool {
    EFFORTS.contains(&v)
}

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

/// Codex 配置文件路径，给界面展示用。
pub fn config_path_on_disk() -> PathBuf {
    CodexInjector::config_path()
}

/// `~/.codex/config.toml` 里当前生效 provider 表的连接参数。
///
/// 读盘结果，供界面显示和预填。`provider_id` 是表名（顶层 `model_provider`
/// 的值），`None` 表示表不存在，此时界面禁用写入控件。
#[derive(Debug, Clone, Default)]
pub struct DiskConnection {
    /// 配置文件是否存在
    pub available: bool,
    /// 当前生效的表名，如 `custom` / `openai_http`
    pub provider_id: Option<String>,
    /// 表里的 base_url，用来猜模型目录地址
    pub base_url: Option<String>,
    /// 当前 `supports_websockets`；`None` 表示文件里没写这个键
    pub supports_websockets: Option<bool>,
    /// 当前 `model_catalog_url`；`None` 表示文件里没写这个键
    pub model_catalog_url: Option<String>,
    /// 当前 `requires_openai_auth`；`None` 表示文件里没写这个键（交给 Codex 自动判断）
    pub requires_openai_auth: Option<bool>,
}

/// 读盘上「当前生效的 provider 表」的连接参数。
///
/// 表名取顶层 `model_provider`，不写死 `custom` —— 用户可能按 Codex 官方
/// 写法自己命名表（如 `openai_http`）。没有顶层键时视为「没有生效表」，
/// 不回退任何默认名。
pub fn read_connection_on_disk() -> DiskConnection {
    let path = CodexInjector::config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return DiskConnection::default();
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return DiskConnection {
            available: true,
            ..Default::default()
        };
    };
    // 顶层 model_provider 才是「当前生效」的唯一声明。
    // 没有它时不回退到 custom —— 那样会把一张可能没被使用的表冒充成生效表。
    let Some(route) = doc.get("model_provider").and_then(|v| v.as_str()) else {
        return DiskConnection {
            available: true,
            ..Default::default()
        };
    };
    let route = route.to_string();
    let table = doc
        .get("model_providers")
        .and_then(|i| i.as_table())
        .and_then(|t| t.get(&route))
        .and_then(|i| i.as_table());
    match table {
        Some(t) => DiskConnection {
            available: true,
            provider_id: Some(route),
            base_url: t
                .get("base_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            supports_websockets: t.get("supports_websockets").and_then(|v| v.as_bool()),
            model_catalog_url: t
                .get("model_catalog_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            requires_openai_auth: t
                .get("requires_openai_auth")
                .and_then(|v| v.as_bool()),
        },
        None => DiskConnection {
            available: true,
            ..Default::default()
        },
    }
}

/// 改当前生效 provider 表里的键，不经过母版。
///
/// 只动 `apply` 里明确处理的键；表内其它字段、注释、行尾风格全部保留。
/// 没有变化时不写盘，返回「已是最新」。
fn edit_active_table(
    apply: impl FnOnce(&mut toml_edit::Table) -> Vec<String>,
) -> anyhow::Result<Vec<String>> {
    let path = CodexInjector::config_path();
    if !path.exists() {
        anyhow::bail!("{} 不存在，先让 Codex 生成配置文件", path.display());
    }

    let orig = std::fs::read_to_string(&path)?;
    let pre = hash_file(&path)?;
    let uses_crlf = orig.contains("\r\n");
    let had_trailing_newline = orig.ends_with('\n');

    let mut doc: toml_edit::DocumentMut = orig.parse().map_err(|e| {
        anyhow::anyhow!(
            "{} 解析失败（{}），已中止，不会写入",
            path.display(),
            e
        )
    })?;

    let Some(route) = doc.get("model_provider").and_then(|v| v.as_str()) else {
        anyhow::bail!(
            "{} 里没有顶层 model_provider，无法确定当前生效的是哪张表",
            path.display()
        );
    };
    let route = route.to_string();

    let table = doc
        .get_mut("model_providers")
        .and_then(|i| i.as_table_mut())
        .and_then(|t| t.get_mut(&route))
        .and_then(|i| i.as_table_mut())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "config.toml 里没有生效的 [model_providers.{route}] 表，没有可以写的地方"
            )
        })?;

    let lines = apply(table);

    if lines.is_empty() {
        return Ok(vec![format!("[model_providers.{route}] 已是最新")]);
    }

    // 恢复原来的行尾风格和末尾换行（toml_edit 都不保留）
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

    super::safe_write::safe_write(&path, &content, &pre)?;

    let mut out = vec![format!(
        "写入 {} 的 [model_providers.{route}]",
        contract_tilde(&path)
    )];
    out.extend(lines);
    Ok(out)
}

/// 直接改当前生效 provider 表的两个连接键，不经过母版。
///
/// 给「Claude & Codex」页用：这两个键只影响 Codex 自己的连接行为，
/// 和「母版里分配给 Codex 的是哪条 provider」是两件事。用户可能压根没用
/// 本工具管理 provider，但 config.toml 里已经有生效的表（如 `openai_http`）。
///
/// 目标表名取顶层 `model_provider`；没有顶层键时报错，不猜默认表。
pub fn set_connection_direct(
    supports_websockets: Option<bool>,
    model_catalog_url: Option<String>,
) -> anyhow::Result<Vec<String>> {
    let want_catalog = model_catalog_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    edit_active_table(|table| {
        let mut lines = Vec::new();

        match supports_websockets {
            Some(v) => {
                if table.get("supports_websockets").and_then(|x| x.as_bool()) != Some(v) {
                    table["supports_websockets"] = toml_edit::value(v);
                    lines.push(format!("supports_websockets → {v}"));
                }
            }
            None => {
                if table.remove("supports_websockets").is_some() {
                    lines.push("移除 supports_websockets（恢复 Codex 默认）".to_string());
                }
            }
        }

        match want_catalog.as_deref() {
            Some(url) => {
                if table.get("model_catalog_url").and_then(|x| x.as_str()) != Some(url) {
                    table["model_catalog_url"] = toml_edit::value(url);
                    lines.push(format!("model_catalog_url → {url}"));
                }
            }
            None => {
                if table.remove("model_catalog_url").is_some() {
                    lines.push("移除 model_catalog_url（恢复 Codex 默认目录）".to_string());
                }
            }
        }

        lines
    })
}

/// 直接改当前生效表的 `requires_openai_auth`，不经过母版。
///
/// `None` 表示删掉这个键，交回 Codex 按盘上登录态自动判断。
pub fn set_requires_openai_auth_direct(value: Option<bool>) -> anyhow::Result<Vec<String>> {
    edit_active_table(|table| {
        let mut lines = Vec::new();
        match value {
            Some(v) => {
                if table.get("requires_openai_auth").and_then(|x| x.as_bool()) != Some(v) {
                    table["requires_openai_auth"] = toml_edit::value(v);
                    lines.push(format!("requires_openai_auth → {v}"));
                }
            }
            None => {
                if table.remove("requires_openai_auth").is_some() {
                    lines.push("移除 requires_openai_auth（交给 Codex 自动判断）".to_string());
                }
            }
        }
        lines
    })
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

                // 模型：优先取 provider 上显式指定的 `codex_model`，
                // 没指定才回退到 models 的第一个。
                //
                // Codex 的模型选择器只列它自己的官方目录（`codex debug models`
                // 能导出那 12 个），自定义 provider 的模型进不去。所以「用哪个
                // 模型」只能由这个顶层 `model` 键决定 —— Codex 不校验名字，
                // 找不到元数据只打一条 warning，名字照原样发给上游。
                if let Some(model) = p.codex_model_or_first() {
                    let cur = doc.get("model").and_then(|v| v.as_str()).unwrap_or("");
                    if cur != model {
                        doc["model"] = toml_edit::value(model.to_string());
                        changed = true;
                        summary.push(format!("model → {model}"));
                    }

                    // 推理档位取自被选中的那个模型的元数据。
                    //
                    // 但母版的档位词表不一定是 Codex 的：实测 CommandCode 的
                    // mimo 写的是 `enabled` / `off`，而 Codex 会把
                    // `model_reasoning_effort` 原样放进请求体的
                    // `reasoning.effort`，上游只认那七个值，别的一律 400：
                    // `Invalid option: expected one of "none"|"minimal"|...`
                    //
                    // 所以只有落在白名单里的值才写；不认识的值不写，并且把
                    // 文件里残留的不合法值一并删掉 —— 留着会让每次请求都失败，
                    // 而「没有这个键」Codex 会用默认档位。
                    let raw_effort = p.find_model(model).and_then(|m| m.default_effort.as_deref());
                    let want_effort = raw_effort.filter(|e| is_codex_effort(e));

                    // 先取成 String，好让下面能改 doc
                    let cur = doc
                        .get("model_reasoning_effort")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    match want_effort {
                        Some(effort) if cur != effort => {
                            doc["model_reasoning_effort"] = toml_edit::value(effort.to_string());
                            changed = true;
                            summary.push(format!("model_reasoning_effort → {effort}"));
                        }
                        Some(_) => {}
                        None => {
                            // 母版给的值不能用（或压根没给）。文件里已有的值
                            // 只要合法就保留 —— 那是用户自己在 config.toml 里选的；
                            // 不合法必须删，否则 Codex 每次请求都会被上游 400。
                            if !cur.is_empty() && !is_codex_effort(&cur) {
                                doc.remove("model_reasoning_effort");
                                changed = true;
                                summary.push(format!(
                                    "移除不认识的 model_reasoning_effort = {cur}（Codex 只接受 {}）",
                                    EFFORTS.join("|")
                                ));
                            }
                            if let Some(raw) = raw_effort {
                                if !is_codex_effort(raw) {
                                    summary.push(format!(
                                        "母版档位 {raw} 不在 Codex 词表里，本次不写 model_reasoning_effort"
                                    ));
                                }
                            }
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

    // supports_websockets：母版没写就清掉，写了就按布尔值落盘。
    //
    // 端点不支持 WebSocket 时，Codex 会反复升级连接、失败、再重连，
    // 表现为回复迟迟不出。关掉后只走 HTTP，日志里不再刷无效重连。
    // 这个键也归本工具管：取消设置要真的删掉，否则用户以为恢复了默认，
    // 实际旧值还留在表里。
    match p.codex_supports_websockets {
        Some(v) => {
            if t.get("supports_websockets").and_then(|x| x.as_bool()) != Some(v) {
                t["supports_websockets"] = toml_edit::value(v);
                changed = true;
                summary.push(format!("supports_websockets → {v}"));
            }
        }
        None => {
            if t.remove("supports_websockets").is_some() {
                changed = true;
                summary.push("移除 supports_websockets（恢复 Codex 默认）".to_string());
            }
        }
    }

    // model_catalog_url：不填就删键，填了就写。
    //
    // Codex 默认只从官方地址拉模型目录，自定义端点可能拉不到，
    // 模型选择器就是空的。补一个返回 OpenAI 模型列表 JSON 的地址
    // （一般是 `<base_url>/models`）即可恢复。
    let want_catalog = p
        .codex_model_catalog_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match want_catalog {
        Some(url) => {
            if t.get("model_catalog_url").and_then(|x| x.as_str()) != Some(url) {
                t["model_catalog_url"] = toml_edit::value(url);
                changed = true;
                summary.push(format!("model_catalog_url → {url}"));
            }
        }
        None => {
            if t.remove("model_catalog_url").is_some() {
                changed = true;
                summary.push("移除 model_catalog_url（恢复 Codex 默认目录）".to_string());
            }
        }
    }

    if changed {
        summary.push(format!("写入 model_providers.{ROUTE_ID}（bearer token 内联）"));
    } else {
        summary.push(format!("model_providers.{ROUTE_ID} 已是最新"));
    }
    Ok(changed)
}
