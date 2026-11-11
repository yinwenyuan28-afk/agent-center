//! 从各 agent 的现有配置里读出 provider，用于和母版比对。
//!
//! **全部只读**。这个模块不改任何文件，只做解析和归一化。
//!
//! 归一化是关键：各家把同一个概念叫成不同名字（ZCode 叫 `kind`、
//! Pi 叫 `api`、Codex 叫 `wire_api`），这里统一映射到
//! [`FoundProvider`]，方便和母版逐字段比对。

use crate::providers::model::{ModelSpec, ProviderKind};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// 从某个 agent 配置里读出来的 provider。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundProvider {
    /// 在这个 agent 里的键名
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// 这个 agent 里**实际用的协议**。
    ///
    /// 协议是 provider × agent 的属性：同一个端点在 ZCode 里可能是
    /// `anthropic`、在 Pi 里是 `openai-responses`。回填时靠它发现
    /// 母版默认值和实际不一致。
    ///
    /// 各家的词表不同（ZCode `kind` / Pi `api` / Hermes `api_mode`），
    /// 这里统一归一成 [`ProviderKind`]，读不出来时为 None。
    pub api_kind: Option<ProviderKind>,
    /// 密钥指纹前 8 位。用于判断是否和母版同一把 key
    pub key_fp: String,
    /// 密钥长度
    pub key_len: usize,
    /// 这个 agent 定义的模型，带能读到的能力元数据
    pub models: Vec<ModelSpec>,
    /// 在哪读到的，如 "config.json · provider.xxx"
    pub location: String,
}

/// 一个 agent 的扫描结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScan {
    pub agent: String,
    /// 配置文件是否存在
    pub present: bool,
    /// 扫描到的 provider
    pub providers: Vec<FoundProvider>,
    /// 读不到明文 key 的原因
    pub unsupported_reason: Option<String>,
}

fn home() -> PathBuf {
    // 与母版共用 home 解析，测试时能整体重定向
    super::store::home_dir()
}

fn fp(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    format!("{:x}", h.finalize()).chars().take(8).collect()
}

fn read_json(p: &PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// 从 JSON 数组里取字符串列表（模态、档位这类）。
fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// 解析 ZCode 的模型条目。
///
/// 真实结构（`~/.zcode/v2/config.json`）：
/// ```jsonc
/// "models": {
///   "<modelId>": {
///     "name": "…",                                  // 可缺省
///     "reasoning": { "enabled": true, "variants": ["low","high"], "defaultVariant": "high" },
///     "limit": { "context": 1000000, "output": 128000 },
///     "modalities": { "input": ["text","image"], "output": ["text"] },
///     "zcode": { "modified": false, "priority": 99 }  // 内部状态，不读
///   }
/// }
/// ```
fn parse_zcode_model(id: &str, v: &Value) -> ModelSpec {
    let mut spec = ModelSpec::new(id);

    spec.name = v
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    spec.context_window = v
        .get("limit")
        .and_then(|l| l.get("context"))
        .and_then(|x| x.as_u64());
    spec.max_output_tokens = v
        .get("limit")
        .and_then(|l| l.get("output"))
        .and_then(|x| x.as_u64());

    if let Some(mods) = v.get("modalities") {
        spec.input_modalities = str_list(mods.get("input"));
        spec.output_modalities = str_list(mods.get("output"));
    }

    if let Some(r) = v.get("reasoning") {
        spec.reasoning = r.get("enabled").and_then(|x| x.as_bool());
        spec.reasoning_efforts = str_list(r.get("variants"));
        spec.default_effort = r
            .get("defaultVariant")
            .and_then(|x| x.as_str())
            .map(String::from);
    }

    spec
}

/// 解析 Pi 的模型条目。
///
/// 真实结构（`~/.pi/agent/models.json` 的 `providers.*.models[]`）：
/// ```jsonc
/// {
///   "id": "…", "name": "…", "reasoning": true,
///   "input": ["text","image"],
///   "contextWindow": 1048576, "maxTokens": 131072,
///   "thinkingLevelMap": { "off": null, "low": "low", … },   // 值非 null 表示该档可用
///   "cost": { "input": 0.1, "output": 0.2, "cacheRead": 0.002, "cacheWrite": 0 }
/// }
/// ```
fn parse_pi_model(v: &Value) -> Option<ModelSpec> {
    let id = v.get("id").and_then(|x| x.as_str())?;
    let mut spec = ModelSpec::new(id);

    spec.name = v
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty() && *s != id)
        .map(String::from);
    spec.context_window = v.get("contextWindow").and_then(|x| x.as_u64());
    spec.max_output_tokens = v.get("maxTokens").and_then(|x| x.as_u64());
    spec.input_modalities = str_list(v.get("input"));
    spec.reasoning = v.get("reasoning").and_then(|x| x.as_bool());

    // thinkingLevelMap：键是档位，值为 null 表示该档不可用
    if let Some(map) = v.get("thinkingLevelMap").and_then(|x| x.as_object()) {
        spec.reasoning_efforts = map
            .iter()
            .filter(|(_, val)| !val.is_null())
            .map(|(k, _)| k.clone())
            .collect();
    }

    if let Some(c) = v.get("cost").and_then(|x| x.as_object()) {
        spec.cost = Some(crate::providers::model::ModelCost {
            input: c.get("input").and_then(|x| x.as_f64()),
            output: c.get("output").and_then(|x| x.as_f64()),
            cache_read: c.get("cacheRead").and_then(|x| x.as_f64()),
            cache_write: c.get("cacheWrite").and_then(|x| x.as_f64()),
        });
    }

    Some(spec)
}

/// 解析 WorkBuddy 的单条模型记录。
///
/// 真实结构（`~/.workbuddy/models.json` 是扁平数组）：
/// ```jsonc
/// {
///   "id": "…", "name": "…", "url": "…", "apiKey": "…",
///   "supportsToolCall": true, "supportsImages": false, "supportsReasoning": true,
///   "maxInputTokens": 1000000,                     // 可缺省
///   "reasoning": { "supportedEfforts": ["high"] }
/// }
/// ```
fn parse_workbuddy_model(v: &Value) -> Option<ModelSpec> {
    let id = v.get("id").and_then(|x| x.as_str())?;
    let mut spec = ModelSpec::new(id);

    spec.name = v
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty() && *s != id)
        .map(String::from);
    spec.context_window = v.get("maxInputTokens").and_then(|x| x.as_u64());
    spec.tool_call = v.get("supportsToolCall").and_then(|x| x.as_bool());
    spec.reasoning = v.get("supportsReasoning").and_then(|x| x.as_bool());

    // WorkBuddy 只有 supportsImages 一个布尔，反推成模态列表
    if let Some(img) = v.get("supportsImages").and_then(|x| x.as_bool()) {
        spec.input_modalities = if img {
            vec!["text".to_string(), "image".to_string()]
        } else {
            vec!["text".to_string()]
        };
    }

    spec.reasoning_efforts = str_list(
        v.get("reasoning")
            .and_then(|r| r.get("supportedEfforts")),
    );

    Some(spec)
}

/// 扫描 ZCode：`~/.zcode/v2/config.json` 的 `provider` map。
pub fn scan_zcode() -> AgentScan {
    let path = home().join(".zcode").join("v2").join("config.json");
    let present = path.exists();
    let mut providers = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(map) = json.get("provider").and_then(|v| v.as_object()) {
            for (id, p) in map {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                let options = p.get("options");
                let base = options
                    .and_then(|o| o.get("baseURL"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let key = options
                    .and_then(|o| o.get("apiKey"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let models = p
                    .get("models")
                    .and_then(|v| v.as_object())
                    .map(|m| {
                        m.iter()
                            .map(|(mid, mv)| parse_zcode_model(mid, mv))
                            .collect()
                    })
                    .unwrap_or_default();

                providers.push(FoundProvider {
                    id: id.clone(),
                    name: name.to_string(),
                    base_url: base.to_string(),
                    api_kind: p
                        .get("kind")
                        .and_then(|k| k.as_str())
                        .map(ProviderKind::parse),
                    key_fp: if key.len() >= 16 { fp(key) } else { String::new() },
                    key_len: key.len(),
                    models,
                    location: format!("config.json · provider.{id}"),
                });
            }
        }
    }

    AgentScan {
        agent: "zcode".into(),
        present,
        providers,
        unsupported_reason: None,
    }
}

/// 扫描 Pi：`auth.json` 提供 key，`models.json` 提供定义。
///
/// `models.json` 里有些 provider 只有名字没有 baseUrl（模型定义从远端目录拉，
/// 存在 `models-store.json` 缓存里）。所以 baseUrl 要三级回退：
/// models.json → models-store.json → 空。
pub fn scan_pi() -> AgentScan {
    let dir = home().join(".pi").join("agent");
    let auth_path = dir.join("auth.json");
    let models_path = dir.join("models.json");
    let store_path = dir.join("models-store.json");
    let present = auth_path.exists() || models_path.exists();
    let mut providers = Vec::new();

    let auth = read_json(&auth_path);
    let models = read_json(&models_path);
    let store = read_json(&store_path);

    /// 从 models-store 缓存里找某个 provider 第一个带 baseUrl 的模型。
    fn cached_base(store: &Option<Value>, id: &str) -> String {
        store
            .as_ref()
            .and_then(|s| s.get(id))
            .and_then(|v| v.get("models"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().find_map(|m| m.get("baseUrl")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// 从缓存里拿模型 id 列表。
    fn cached_models(store: &Option<Value>, id: &str) -> Vec<String> {
        store
            .as_ref()
            .and_then(|s| s.get(id))
            .and_then(|v| v.get("models"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    if let Some(auth) = auth.as_ref().and_then(|v| v.as_object()) {
        for (id, v) in auth {
            let key = v.get("key").and_then(|x| x.as_str()).unwrap_or("");
            // 从 models.json 里找同名 provider 的定义
            let def = models
                .as_ref()
                .and_then(|m| m.get("providers"))
                .and_then(|p| p.get(id));

            let name = def
                .and_then(|d| d.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or(id);

            // baseUrl 三级回退
            let base = def
                .and_then(|d| d.get("baseUrl"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| cached_base(&store, id));

            // 模型列表：models.json 优先（带元数据），空了从缓存补（只有 id）
            let models_from_def: Vec<ModelSpec> = def
                .and_then(|d| d.get("models"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(parse_pi_model).collect())
                .unwrap_or_default();
            let models_final = if models_from_def.is_empty() {
                cached_models(&store, id)
                    .into_iter()
                    .map(ModelSpec::new)
                    .collect()
            } else {
                models_from_def
            };

            providers.push(FoundProvider {
                id: id.clone(),
                name: name.to_string(),
                base_url: base,
                api_kind: def
                    .and_then(|d| d.get("api"))
                    .and_then(|v| v.as_str())
                    .map(ProviderKind::parse),
                key_fp: if key.len() >= 16 { fp(key) } else { String::new() },
                key_len: key.len(),
                models: models_final,
                location: format!("auth.json · {id}"),
            });
        }
    }

    AgentScan {
        agent: "pi".into(),
        present,
        providers,
        unsupported_reason: None,
    }
}

/// 扫描 opencode：`opencode.json` 定义 + `~/.local/share/opencode/auth.json` 的 key。
pub fn scan_opencode() -> AgentScan {
    let cfg = home().join(".config").join("opencode").join("opencode.json");
    let auth_path = home()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json");
    let present = cfg.exists() || auth_path.exists();
    let mut providers = Vec::new();

    let json = read_json(&cfg);
    let auth = read_json(&auth_path);

    // 新版结构在 providers 下，旧版在 provider 下
    let block = json
        .as_ref()
        .and_then(|j| j.get("providers").or_else(|| j.get("provider")));

    if let Some(map) = block.and_then(|v| v.as_object()) {
        for (id, p) in map {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or(id);
            let base = p
                .get("settings")
                .and_then(|s| s.get("baseURL"))
                .or_else(|| p.get("options").and_then(|o| o.get("baseURL")))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // key 可能在 auth.json，也可能（旧版）直接在 options 里
            let auth_key = auth
                .as_ref()
                .and_then(|a| a.get(id))
                .and_then(|e| e.get("key"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let inline_key = p
                .get("options")
                .and_then(|o| o.get("apiKey"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key = if !auth_key.is_empty() { auth_key } else { inline_key };

            let models: Vec<ModelSpec> = p
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .map(|(mid, mv)| {
                            // opencode 的模型条目通常只有 name，没有能力字段。
                            // 有 name 就带上，其余留空
                            let mut spec = ModelSpec::new(mid);
                            spec.name = mv
                                .get("name")
                                .and_then(|x| x.as_str())
                                .filter(|s| !s.is_empty() && *s != mid)
                                .map(String::from);
                            spec
                        })
                        .collect()
                })
                .unwrap_or_default();

            providers.push(FoundProvider {
                id: id.clone(),
                name: name.to_string(),
                base_url: base.to_string(),
                // V2 用 package，V1 用 npm；含 anthropic 即 Anthropic，否则按 chat 兼容层
                api_kind: p
                    .get("package")
                    .or_else(|| p.get("npm"))
                    .and_then(|v| v.as_str())
                    .map(|pkg| {
                        if pkg.contains("anthropic") {
                            ProviderKind::Anthropic
                        } else if pkg.contains("openai") && !pkg.contains("compatible") {
                            ProviderKind::OpenaiResponses
                        } else {
                            ProviderKind::OpenaiChat
                        }
                    }),
                key_fp: if key.len() >= 16 { fp(key) } else { String::new() },
                key_len: key.len(),
                models,
                location: format!("opencode.json · provider.{id}"),
            });
        }
    }

    AgentScan {
        agent: "opencode".into(),
        present,
        providers,
        unsupported_reason: None,
    }
}

/// 扫描 Codex：`~/.codex/config.toml` 的 `model_providers` 表。
///
/// 注意 Codex 的 key 不落盘，只有 `env_key` 指向环境变量名。
/// 这里能读到的是环境变量名，不是 key 本身。
pub fn scan_codex() -> AgentScan {
    let path = home().join(".codex").join("config.toml");
    let present = path.exists();
    let mut providers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(doc) = text.parse::<toml_edit::DocumentMut>() {
            if let Some(tbl) = doc.get("model_providers").and_then(|v| v.as_table()) {
                for (id, v) in tbl {
                    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or(id);
                    let base = v
                        .get("base_url")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    // key 在环境变量里，这里只能拿到变量名
                    let env_key = v.get("env_key").and_then(|x| x.as_str()).unwrap_or("");
                    let key_len = if env_key.is_empty() {
                        0
                    } else {
                        std::env::var(env_key).map(|s| s.len()).unwrap_or(0)
                    };

                    providers.push(FoundProvider {
                        id: id.to_string(),
                        name: name.to_string(),
                        base_url: base.to_string(),
                        // Codex 只有 responses 一种往上写的可能
                        api_kind: v
                            .get("wire_api")
                            .and_then(|x| x.as_str())
                            .map(ProviderKind::parse),

                        // 环境变量读得到才算指纹，读不到留空
                        key_fp: std::env::var(env_key)
                            .ok()
                            .filter(|s| s.len() >= 16)
                            .map(|s| fp(&s))
                            .unwrap_or_default(),
                        key_len,
                        models: Vec::new(),
                        location: format!("config.toml · model_providers.{id} (env_key={env_key})"),
                    });
                }
            }
        }
    }

    // 先算出结论再构造结构体：结构体字面量里 providers 会被 move，
    // 之后就不能再借用它了
    let unsupported_reason = if present && providers.is_empty() {
        Some("config.toml 里没有 model_providers 表，当前走内置 openai + OAuth".to_string())
    } else {
        None
    };

    AgentScan {
        agent: "codex".into(),
        present,
        providers,
        unsupported_reason,
    }
}

/// 扫描 Claude：它没有 provider 列表，只有一个当前端点。
pub fn scan_claude() -> AgentScan {
    let path = home().join(".claude").join("settings.json");
    let present = path.exists();
    let mut providers = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(env) = json.get("env") {
            let base = env
                .get("ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str())
                .unwrap_or("官方 Anthropic");
            let key = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !key.is_empty() {
                // Claude 只有一个槽位，用 base_url 作为识别标识
                providers.push(FoundProvider {
                    id: "current".to_string(),
                    name: base.to_string(),
                    base_url: base.to_string(),
                    // Claude 只有 ANTHROPIC_* 这一套，恒为 Anthropic
                    api_kind: Some(ProviderKind::Anthropic),

                    key_fp: if key.len() >= 16 { fp(key) } else { String::new() },
                    key_len: key.len(),
                    models: Vec::new(),
                    location: "settings.json · env.ANTHROPIC_*".to_string(),
                });
            }
        }
    }

    let unsupported_reason = if present && providers.is_empty() {
        Some("settings.json 里没有 ANTHROPIC_AUTH_TOKEN，可能用的是订阅登录".to_string())
    } else {
        None
    };

    AgentScan {
        agent: "claude".into(),
        present,
        providers,
        unsupported_reason,
    }
}

/// 扫描 WorkBuddy：`~/.workbuddy/models.json` 是扁平数组。
///
/// 同一个 provider 的多个模型各占一条，按 url 聚合去重。
pub fn scan_workbuddy() -> AgentScan {
    let path = home().join(".workbuddy").join("models.json");
    let present = path.exists();
    let mut providers: Vec<FoundProvider> = Vec::new();

    if let Some(json) = read_json(&path) {
        if let Some(arr) = json.as_array() {
            for entry in arr {
                let url = entry.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() {
                    continue;
                }
                let key = entry.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
                let model_id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");

                // 按 url 聚合：已有同 url 的条目就合并模型
                let spec = parse_workbuddy_model(entry);
                if let Some(existing) = providers
                    .iter_mut()
                    .find(|p| p.base_url == url && !p.base_url.is_empty())
                {
                    if let Some(s) = spec {
                        if !existing.models.iter().any(|m| m.id == s.id) {
                            existing.models.push(s);
                        }
                    }
                } else {
                    providers.push(FoundProvider {
                        id: model_id.to_string(),
                        name: entry
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(model_id)
                            .to_string(),
                        base_url: url.to_string(),
                        // useCustomProtocol: true 表示走自定义协议（responses）
                        api_kind: entry
                            .get("useCustomProtocol")
                            .and_then(|v| v.as_bool())
                            .map(|custom| {
                                if custom {
                                    ProviderKind::OpenaiResponses
                                } else {
                                    ProviderKind::OpenaiChat
                                }
                            }),

                        key_fp: if key.len() >= 16 { fp(key) } else { String::new() },
                        key_len: key.len(),
                        models: spec.into_iter().collect(),
                        location: format!("models.json · {url}"),
                    });
                }
            }
        }
    }

    AgentScan {
        agent: "workbuddy".into(),
        present,
        providers,
        unsupported_reason: None,
    }
}

/// 扫描 DSH：provider 在 cordis.patch.yml，key 在 .credentials.yaml。
///
/// 两边通过 `apiKeyEnv` / `refs` 的变量名关联，需要一起读才能还原一条完整记录。
pub fn scan_dsh() -> AgentScan {
    let root = home().join(".dsh");
    let patch_path = root.join("profiles").join("desktop").join("cordis.patch.yml");
    let cred_path = root.join(".credentials.yaml");
    let present = patch_path.exists() || cred_path.exists();
    let mut providers = Vec::new();

    // 从 credentials.yaml 读 refs 段：ENV_NAME -> key
    let mut refs: std::collections::BTreeMap<String, String> = Default::default();
    if let Ok(text) = std::fs::read_to_string(&cred_path) {
        let mut in_refs = false;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent == 0 {
                in_refs = line.trim_start().starts_with("refs:");
                continue;
            }
            if in_refs {
                if let Some((k, v)) = line.trim().split_once(':') {
                    refs.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
    }

    // 从 cordis.patch.yml 读 providers 段
    if let Ok(text) = std::fs::read_to_string(&patch_path) {
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        let mut in_providers = false;
        let mut current: Option<(String, String, String)> = None; // (id, api, base_url)

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();

            if trimmed.is_empty() {
                i += 1;
                continue;
            }

            if indent == 0 {
                // 离开 providers 段时收尾
                if in_providers {
                    if let Some((id, api, base)) = current.take() {
                        push_dsh(&mut providers, &refs, id, api, base, &lines, i);
                    }
                }
                in_providers = trimmed.starts_with("providers:");
                current = None;
                i += 1;
                continue;
            }

            if in_providers && indent == 2 {
                // 新的 provider 起点，先把上一个收尾
                if let Some((id, api, base)) = current.take() {
                    push_dsh(&mut providers, &refs, id, api, base, &lines, i);
                }
                let id = trimmed.trim_end_matches(':').to_string();
                current = Some((id, String::new(), String::new()));
                i += 1;
                continue;
            }

            if in_providers && indent >= 4 {
                if let Some((_, api, base)) = current.as_mut() {
                    if let Some(v) = trimmed.strip_prefix("api:") {
                        *api = v.trim().to_string();
                    } else if let Some(v) = trimmed
                        .strip_prefix("baseURL:")
                        .or_else(|| trimmed.strip_prefix("baseUrl:"))
                    {
                        *base = v.trim().to_string();
                    }
                }
            }

            i += 1;
        }

        // 文件末尾时收尾
        if in_providers {
            if let Some((id, api, base)) = current.take() {
                push_dsh(&mut providers, &refs, id, api, base, &lines, lines.len());
            }
        }
    }

    // 先算结论再构造：字面量里 providers 会被 move
    let unsupported_reason = if present && providers.is_empty() {
        Some("cordis.patch.yml 里没有 providers 段".to_string())
    } else {
        None
    };

    AgentScan {
        agent: "dsh".into(),
        present,
        providers,
        unsupported_reason,
    }
}

/// 把一个解析到的 DSH provider 收进结果。key 从 refs 里按环境变量名查。
fn push_dsh(
    out: &mut Vec<FoundProvider>,
    refs: &std::collections::BTreeMap<String, String>,
    id: String,
    api: String,
    base_url: String,
    _lines: &[&str],
    _at: usize,
) {
    // 约定：env 变量名是 ID_API_KEY
    let env = format!(
        "{}_API_KEY",
        id.chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            })
            .collect::<String>()
    );
    let key = refs.get(&env).cloned().unwrap_or_default();

    // 先算 location / name，避免在结构体字面量里重复用 id
    let location = format!("cordis.patch.yml · providers.{id} (api={api}, env={env})");
    let name = id.clone();

    out.push(FoundProvider {
        id,
        name,
        base_url,
        // DSH 的 api 字段用的是 Pi 那套词表（anthropic-messages / openai-completions）
        api_kind: if api.is_empty() {
            None
        } else {
            Some(ProviderKind::parse(&api))
        },
        key_fp: if key.len() >= 16 { fp(&key) } else { String::new() },
        key_len: key.len(),
        models: Vec::new(),
        location,
    });
}

/// 扫描 Hermes：`$HERMES_HOME/config.yaml` 的 `providers` 段。
///
/// 结构（行级解析，不引 YAML 库）：
/// ```yaml
/// providers:
///   opencode-go:
///     name: OpenCode Go
///     base_url: https://opencode.ai/zen/go/v1
///     api_mode: chat_completions
///     transport: openai_chat
///     model: deepseek-v4-flash
///     models:
///       glm-5.2:
///         context_length: 204800
///         supports_tools: true
///     api_key: sk-xxx
/// ```
pub fn scan_hermes() -> AgentScan {
    let path = crate::providers::hermes::config_path();
    let present = path.exists();
    let mut providers = Vec::new();

    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(providers_block) = find_yaml_top_block(&text, "providers") {
            providers = parse_hermes_providers(providers_block);
        }
    }

    AgentScan {
        agent: "hermes".into(),
        present,
        providers,
        unsupported_reason: None,
    }
}

/// 从 YAML 文本里取一个顶层键的完整区段（含缩进内容，不含该键行）。
fn find_yaml_top_block<'a>(text: &'a str, key: &str) -> Option<Vec<&'a str>> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.starts_with(&format!("{key}:")))?;

    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim().is_empty() {
            continue;
        }
        if l.len() - l.trim_start().len() == 0 {
            end = i;
            break;
        }
    }
    Some(lines[start + 1..end].to_vec())
}

/// 解析 Hermes 的 `providers` 段。
///
/// 逐行状态机：二级键（缩进 2）开一个新 provider，
/// 三级键（缩进 4）读 provider 级字段，四级键（缩进 6）读模型字段。
fn parse_hermes_providers(block: Vec<&str>) -> Vec<FoundProvider> {
    #[derive(Default)]
    struct Cur {
        id: String,
        name: String,
        base: String,
        api_key: String,
        /// Hermes 的 api_mode：chat_completions / responses
        api_mode: String,
        models: Vec<ModelSpec>,
    }

    let mut out: Vec<FoundProvider> = Vec::new();
    let mut cur: Option<Cur> = None;
    // 当前在 models 下的哪个模型
    let mut cur_model: Option<ModelSpec> = None;

    fn flush_model(cur: &mut Option<Cur>, m: &mut Option<ModelSpec>) {
        if let (Some(c), Some(spec)) = (cur.as_mut(), m.take()) {
            c.models.push(spec);
        }
    }

    fn flush_provider(out: &mut Vec<FoundProvider>, cur: &mut Option<Cur>) {
        let Some(c) = cur.take() else { return };
        if c.id.is_empty() {
            return;
        }
        out.push(FoundProvider {
            id: c.id.clone(),
            name: if c.name.is_empty() {
                c.id.clone()
            } else {
                c.name.clone()
            },
            base_url: c.base.clone(),
            // Hermes 的 api_mode：chat_completions / responses
            api_kind: if c.api_mode.is_empty() {
                None
            } else {
                Some(ProviderKind::parse(&c.api_mode))
            },
            key_fp: if c.api_key.len() >= 16 {
                fp(&c.api_key)
            } else {
                String::new()
            },
            key_len: c.api_key.len(),
            models: c.models,
            location: format!("config.yaml · providers.{}", c.id),
        });
    }

    for line in block {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();

        if indent == 2 {
            // 新 provider 起点
            flush_model(&mut cur, &mut cur_model);
            flush_provider(&mut out, &mut cur);
            let id = t.trim_end_matches(':').to_string();
            cur = Some(Cur {
                id,
                ..Default::default()
            });
            continue;
        }

        if indent == 4 {
            // provider 级字段。遇到 models: 就进入模型列表
            flush_model(&mut cur, &mut cur_model);
            let Some((k, v)) = t.split_once(':') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim();
            if let Some(c) = cur.as_mut() {
                match k {
                    "name" => c.name = v.to_string(),
                    "base_url" => c.base = v.to_string(),
                    "api_key" => c.api_key = v.to_string(),
                    "api_mode" => c.api_mode = v.to_string(),
                    // models: 下面才是模型，这里不处理
                    _ => {}
                }
            }
            continue;
        }

        if indent == 6 {
            // 模型 id 行
            flush_model(&mut cur, &mut cur_model);
            let id = t.trim_end_matches(':').to_string();
            if !id.is_empty() && t.ends_with(':') {
                let mut spec = ModelSpec::new(id);
                spec.input_modalities = vec!["text".to_string()];
                cur_model = Some(spec);
            }
            continue;
        }

        if indent >= 8 {
            // 模型字段
            if let Some((k, v)) = t.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                if let Some(spec) = cur_model.as_mut() {
                    match k {
                        "context_length" => {
                            spec.context_window = v.parse::<u64>().ok();
                        }
                        "supports_tools" => {
                            spec.tool_call = Some(v == "true");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    flush_model(&mut cur, &mut cur_model);
    flush_provider(&mut out, &mut cur);
    out
}

/// 扫描全部支持的 agent。
pub fn scan_all() -> Vec<AgentScan> {
    vec![
        scan_zcode(),
        scan_pi(),
        scan_opencode(),
        scan_codex(),
        scan_claude(),
        scan_workbuddy(),
        scan_dsh(),
        scan_hermes(),
    ]
}
