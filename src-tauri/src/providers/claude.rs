//! Claude Code 的 provider 注入器。
//!
//! ## 为什么 Claude 只能有一个 provider
//!
//! Claude Code 没有 provider 列表的概念，只有一组 `ANTHROPIC_*` 环境
//! 变量决定当前端点。所以这里是**单槽位**：同一时刻只有一个 provider
//! 生效。界面上的矩阵会保证母版里只有一条勾着 `claude`。
//!
//! ## 关键字段（floor）清单对齐 cc-switch
//!
//! cc-switch 把 `settings.json` 的键分成三类：
//!
//! 1. **关键字段**：请求发到哪、凭什么鉴权、哪个模型名、说哪种协议。
//!    切换时一律先清空再写目标值。
//! 2. **供应商独有字段**：上游兼容开关、窗口值。切入时写入；切走时
//!    只删「上一家带进来、而且值没被改过」的。
//! 3. **其余键**：归用户和 Claude Code，一律不碰。
//!
//! 早期实现只接管 10 个 `ANTHROPIC_*` 键，结果切换 Bedrock / Vertex
//! 时旧凭据残留。现在按 cc-switch 的清单来：
//!
//! - `env` 里 `ANTHROPIC_` / `AWS_` / `VERTEX_REGION_` **整个前缀**归关键字段
//! - 另外点名 `CLAUDE_CODE_SUBAGENT_MODEL*`、`CLAUDE_CODE_OAUTH_*`、
//!   `CLOUD_ML_REGION`、`GOOGLE_APPLICATION_CREDENTIALS`、
//!   `CLAUDE_CODE_API_KEY_HELPER_TTL_MS`
//! - `CLAUDE_CODE_USE_*` **不能按前缀全收** —— 同一前缀下还有
//!   `USE_POWERSHELL_TOOL` 这类功能开关。只认 7 个协议选择器
//! - `CLAUDE_CODE_SKIP_*_AUTH` 按前后缀匹配
//!
//! 顶层只清 12 个点名键（`apiKey`、`model`、`apiKeyHelper` 等），
//! `permissions`、`hooks`、`statusLine`、`enabledPlugins` 不动。
//!
//! ## 代理模式：稳定别名 + 本地映射
//!
//! 上游不是 Anthropic 协议、或者模型不是 `claude-` 开头时，Claude Code
//! 无法直接对话。这时 `ANTHROPIC_BASE_URL` 指向本地转换端口，模型别名
//! 写成稳定的 `claude-*` 名字，由 [`crate::providers::claude_bridge`]
//! 按档位翻成上游真实模型。
//!
//! 直接写上游真实模型名（如 `deepseek/deepseek-v4.1-flash`）也能用，
//! 但 Claude Code 会按未知模型给它 200K 窗口、关掉思考参数。稳定别名
//! 让 Claude Code 按自己认识的模型表处理，转换层再做真实映射。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider};
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

// ---- 关键字段清单（和 cc-switch 的 live/floor.rs 一致）----

/// `env` 里整个前缀都属于连接和鉴权。
const FLOOR_ENV_PREFIXES: &[&str] = &["ANTHROPIC_", "AWS_", "VERTEX_REGION_"];

/// Claude Code 的协议选择器。
///
/// `CLAUDE_CODE_USE_` 不能按前缀匹配：同一前缀下还有
/// `USE_POWERSHELL_TOOL`、`USE_NATIVE_FILE_SEARCH` 这类功能开关。
const PROTOCOL_SELECTORS: &[&str] = &[
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
];

/// `env` 里按名字列出的关键字段（协议选择器之外）。
const FLOOR_ENV_KEYS: &[&str] = &[
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL_FORCE",
    "CLOUD_ML_REGION",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
];

/// `settings.json` 顶层的关键字段。
const FLOOR_TOP: &[&str] = &[
    "apiKeyHelper",
    "apiBaseUrl",
    "primaryModel",
    "smallFastModel",
    "apiKey",
    "model",
    "fallbackModel",
    "modelOverrides",
    "modelPicker",
    "advisorModel",
    "awsAuthRefresh",
    "awsCredentialExport",
    "gcpAuthRefresh",
];

/// 供应商独有字段（`env` 里）：上游兼容开关和窗口值。
///
/// 切入时写入；切走时只删上一家带进来、而且值没被改过的。
const EXCLUSIVE_ENV: &[&str] = &[
    "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
    "CLAUDE_CODE_DISABLE_ARTIFACT",
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_DISABLE_THINKING",
    "DISABLE_INTERLEAVED_THINKING",
    "CLAUDE_CODE_ALWAYS_ENABLE_EFFORT",
    "CLAUDE_CODE_EXTRA_BODY",
    "CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING",
    "CLAUDE_CODE_AUTO_MODE_SERVER",
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_1M_CONTEXT",
    "CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT",
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
];

/// 这个 `env` 键是不是关键字段。
fn is_floor_env(key: &str) -> bool {
    FLOOR_ENV_PREFIXES.iter().any(|p| key.starts_with(p))
        || PROTOCOL_SELECTORS.contains(&key)
        || FLOOR_ENV_KEYS.contains(&key)
        || (key.starts_with("CLAUDE_CODE_SKIP_") && key.ends_with("_AUTH"))
}

/// 代理模式写进客户端的稳定档位别名。
///
/// Claude Code 按别名查自己的模型表决定思考参数和窗口：`claude-opus-5`
/// 是 128K 输出、支持思考的当代模型。直接写上游真实模型名会被当成
/// 未知模型，按 200K 窗口、`thinking: disabled` 处理。
const PROXY_ALIASES: [(&str, &str); 3] = [
    ("ANTHROPIC_DEFAULT_OPUS_MODEL", "claude-opus-5"),
    ("ANTHROPIC_DEFAULT_SONNET_MODEL", "claude-sonnet-5"),
    ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "claude-haiku-5-5"),
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

        if !root_obj.contains_key("env") {
            root_obj.insert("env".into(), Value::Object(Map::new()));
        }
        if !root
            .get("env")
            .map(|v| v.is_object())
            .unwrap_or(false)
        {
            anyhow::bail!("{} 的 env 不是对象", path.display());
        }

        let mut summary = Vec::new();

        // Claude 只有一个槽位，取第一个声明注入 claude 的 provider
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "claude"));

        // 当前生效的端点
        let current_base = root
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 没有任何 provider 分配给 claude 时，才考虑"要不要清理旧配置"。
        // **必须在确认无分配之后**再判断，否则已分配的场景会先被清空、
        // 再写回同样的值，把幂等性破坏掉（每次同步都报"有改动"）。
        if wanted.is_none() {
            // 母版里明确没分配给 claude 的端点
            let unassigned_urls: Vec<String> = providers
                .iter()
                .filter(|p| !p.agents.iter().any(|a| a == "claude"))
                .map(|p| normalize_url(&p.base_url))
                .collect();

            // 走本地转换时 `ANTHROPIC_BASE_URL` 是 `http://127.0.0.1:15722`，
            // 不是上游端点，所以也要认这个地址 —— 否则取消分配后代理地址
            // 会一直留着。
            let ours = !current_base.is_empty()
                && (unassigned_urls.contains(&normalize_url(&current_base))
                    || current_base == super::claude_bridge::bridge_base_url());

            if !ours {
                // 对不上任何母版端点：订阅登录、用户手配的第三方，不动
                return Ok(Vec::new());
            }

            let before = root.clone();
            clear_floor(root.as_object_mut().unwrap());
            let changed = root != before;
            summary.push(if changed {
                "当前端点的分配已取消，清掉接管的字段".to_string()
            } else {
                "已是最新".to_string()
            });
            let content = if changed {
                let mut s = serde_json::to_string_pretty(&root)?;
                s.push('\n');
                s
            } else {
                orig
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

        let p = wanted.unwrap();

        // ---- 1. 先算出最终值，再和现有内容比对 ----
        //
        // 不能「清空时置 changed、写入时再置 changed」—— 那样第二次
        // 同步会先删再写同样的值，永远报"有改动"，幂等就废了。
        // 正确做法：在内存里清空 + 写入，最后整体比对。
        let before = root.clone();

        // 关键字段全部清空。不清干净的话，上一家留下的 AWS_ /
        // CLAUDE_CODE_OAUTH_ / 协议选择器会让 Claude Code 绕过新地址
        // 去走旧凭据。只清关键字段，用户键不动。
        clear_floor(root.as_object_mut().unwrap());

        let root_obj = root.as_object_mut().unwrap();
        if !root_obj.contains_key("env") {
            root_obj.insert("env".into(), Value::Object(Map::new()));
        }
        let env = root_obj
            .get_mut("env")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("{} 的 env 不是对象", path.display()))?;

        // ---- 2. 决定直连还是走本地转换 ----
        let model_ids = emitted_model_ids(p);
        let use_bridge = super::claude_bridge::needs_bridge(p.kind_for("claude"), &model_ids);
        let base_url = if use_bridge {
            super::claude_bridge::bridge_base_url()
        } else {
            p.base_url.clone()
        };

        env.insert(
            "ANTHROPIC_BASE_URL".into(),
            Value::String(base_url.clone()),
        );
        // 统一用 AUTH_TOKEN（Bearer 方式）。API_KEY 已在清空阶段拿掉。
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".into(),
            Value::String(p.api_key.clone()),
        );

        // ---- 3. 模型档位 ----
        //
        // 代理模式写稳定别名，让 Claude Code 用自己的模型表算窗口和
        // 思考参数；真实模型由转换层按档位映射。
        // 直连 Anthropic 时写真实模型名。
        if !p.models.is_empty() {
            let pick = |i: usize| -> &ModelSpec {
                if i < p.models.len() {
                    &p.models[i]
                } else {
                    p.models.last().unwrap()
                }
            };
            let resolve = |want: Option<&String>, fallback: usize| -> &ModelSpec {
                want.and_then(|id| p.find_model(id))
                    .unwrap_or_else(|| pick(fallback))
            };

            let map = &p.claude_model_map;
            let slots = [
                (PROXY_ALIASES[0].0, PROXY_ALIASES[0].1, resolve(map.opus.as_ref(), 0)),
                (
                    PROXY_ALIASES[1].0,
                    PROXY_ALIASES[1].1,
                    resolve(map.sonnet.as_ref(), 1),
                ),
                (
                    PROXY_ALIASES[2].0,
                    PROXY_ALIASES[2].1,
                    resolve(map.haiku.as_ref(), 2),
                ),
            ];

            for (var, alias, model) in slots {
                let written = if use_bridge { alias } else { model.id.as_str() };
                env.insert(var.into(), Value::String(written.to_string()));
                env.insert(
                    format!("{var}_NAME"),
                    Value::String(model.display_name().to_string()),
                );
            }

            let default_model = resolve(map.default_model.as_ref(), 0);
            let written = if use_bridge {
                PROXY_ALIASES[0].1
            } else {
                default_model.id.as_str()
            };
            env.insert("ANTHROPIC_MODEL".into(), Value::String(written.to_string()));
        }

        // ---- 4. 整体比对 ----
        let changed = root != before;
        if !changed {
            summary.push(format!("{} 已是最新", p.name));
        } else {
            summary.insert(0, format!("注入 {} 的配置", p.name));
            if use_bridge {
                summary.push("有模型不走 Anthropic Messages，Claude 改连本地转换".to_string());
            }
            // 说清楚变了什么，而不是罗列每个键
            let before_env = before
                .get("env")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let after_env = root
                .get("env")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let mut wrote = Vec::new();
            for (k, v) in after_env.iter() {
                if before_env.get(k) != Some(v) {
                    wrote.push(k.clone());
                }
            }
            let mut removed = Vec::new();
            for k in before_env.keys() {
                if !after_env.contains_key(k) {
                    removed.push(k.clone());
                }
            }
            if !wrote.is_empty() {
                summary.push(format!("写入 {}", wrote.join(", ")));
            }
            if !removed.is_empty() {
                summary.push(format!("清掉旧关键字段 {}", removed.join(", ")));
            }
        }

        let content = if changed {
            let mut s = serde_json::to_string_pretty(&root)?;
            s.push('\n');
            s
        } else {
            orig
        };

        let mut plans = vec![FilePlan {
            path: contract_tilde(&path),
            agent: "claude".to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }];
        if use_bridge {
            let bridge_path = super::claude_bridge::config_path();
            let bridge_pre = hash_file(&bridge_path)?;
            let bridge_content =
                super::claude_bridge::bridge_file_content(&p.base_url, p.kind_for("claude"));
            let bridge_changed = std::fs::read_to_string(&bridge_path).ok().as_deref()
                != Some(bridge_content.as_str());
            plans.push(FilePlan {
                path: contract_tilde(&bridge_path),
                agent: "claude".to_string(),
                content: bridge_content,
                pre_hash: bridge_pre,
                post_hash: String::new(),
                changed: bridge_changed,
                summary: vec!["记下转换上游。agentcenter 窗口开着时 Claude 走这里".into()],
            });
        }
        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let wanted = providers
            .iter()
            .find(|p| p.agents.iter().any(|a| a == "claude"));

        Ok(plans
            .into_iter()
            .map(|plan| {
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

/// 清掉所有关键字段。用户键（`JAVA_HOME`、`permissions`、`hooks` 等）不动。
fn clear_floor(root: &mut Map<String, Value>) {
    // 顶层关键字段
    for key in FLOOR_TOP {
        root.remove(*key);
    }

    // env 里的关键字段
    if let Some(env) = root.get_mut("env").and_then(|v| v.as_object_mut()) {
        let doomed: Vec<String> = env.keys().filter(|k| is_floor_env(k)).cloned().collect();
        for key in doomed {
            env.remove(&key);
        }
    }
}

/// 这次会写进 Claude 环境变量的模型 id。空列表表示母版没填模型。
fn emitted_model_ids(p: &Provider) -> Vec<String> {
    if p.models.is_empty() {
        return Vec::new();
    }
    let pick = |i: usize| {
        if i < p.models.len() {
            &p.models[i]
        } else {
            p.models.last().unwrap()
        }
    };
    let resolve = |want: Option<&String>, fallback: usize| -> String {
        want.and_then(|id| p.find_model(id))
            .unwrap_or_else(|| pick(fallback))
            .id
            .clone()
    };
    let map = &p.claude_model_map;
    let mut ids = vec![
        resolve(map.opus.as_ref(), 0),
        resolve(map.sonnet.as_ref(), 1),
        resolve(map.haiku.as_ref(), 2),
        resolve(map.default_model.as_ref(), 0),
    ];
    ids.sort();
    ids.dedup();
    ids
}

/// 归一化 URL 用于比对。忽略协议差异和尾部斜杠。
fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

/// 暴露给测试和界面：本工具接管的 `env` 键判定函数。
pub fn is_owned_env_key(key: &str) -> bool {
    is_floor_env(key)
}

/// 暴露给测试和界面：本工具接管的顶层键。
pub fn owned_top_keys() -> &'static [&'static str] {
    FLOOR_TOP
}

/// 暴露给测试和界面：供应商独有字段。
pub fn exclusive_env_keys() -> &'static [&'static str] {
    EXCLUSIVE_ENV
}
