//! provider 统一管理的核心数据模型。
//!
//! 这里定义三组结构：
//!
//! 1. [`Provider`] —— 母版 `providers.toml` 里的一条记录，是唯一的真相来源
//! 2. [`FilePlan`] —— 某个文件"应该变成什么样"的计划，写盘前先在内存里算好
//! 3. [`DiffLine`] —— 面向人的差异描述，用于 `providers diff` 和界面预览
//!
//! 设计原则：**计划与执行分离**。`plan` 阶段只读，产出 `FilePlan`；
//! 执行阶段拿 `FilePlan` 去写。这样 `diff` 和真正的同步可以复用同一套计算。

use serde::{Deserialize, Serialize};

// ModelSpec / ModelCost 定义在 types.rs —— 它们是跨进程契约的一部分，
// 前端要消费，契约检查脚本也只会扫那个文件。这里 re-export 出来，
// 让 `providers::model::ModelSpec` 依然可用。
pub use crate::types::{ModelCost, ModelSpec};

impl ModelSpec {
    /// 只有 id、没有任何元数据。
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            context_window: None,
            max_output_tokens: None,
            input_modalities: Vec::new(),
            output_modalities: Vec::new(),
            reasoning: None,
            reasoning_efforts: Vec::new(),
            default_effort: None,
            tool_call: None,
            cost: None,
        }
    }

    /// 是否只有 id、没填任何元数据。
    ///
    /// 用来决定序列化形态：全都没填就写纯字符串数组，
    /// 保持存量母版的文件形状不变。
    pub fn is_bare(&self) -> bool {
        self.name.is_none()
            && self.context_window.is_none()
            && self.max_output_tokens.is_none()
            && self.input_modalities.is_empty()
            && self.output_modalities.is_empty()
            && self.reasoning.is_none()
            && self.reasoning_efforts.is_empty()
            && self.default_effort.is_none()
            && self.tool_call.is_none()
            && self.cost.is_none()
    }

    /// 展示名。没填就用 id。
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// 上下文窗口。没填时给一个宽松的默认值，
    /// 避免下游注入器各自写死 1000000。
    pub fn context_or_default(&self) -> u64 {
        self.context_window.unwrap_or(1_000_000)
    }

    /// 输出上限。没填时给默认 128000。
    pub fn output_or_default(&self) -> u64 {
        self.max_output_tokens.unwrap_or(128_000)
    }

    /// 输入模态。没填按纯文本算。
    pub fn input_or_default(&self) -> Vec<String> {
        if self.input_modalities.is_empty() {
            vec!["text".to_string()]
        } else {
            self.input_modalities.clone()
        }
    }

    /// 是否支持图像输入。WorkBuddy 只有 `supportsImages: bool` 这一个字段，
    /// 需要从模态列表反推。
    pub fn supports_images(&self) -> bool {
        self.input_modalities.iter().any(|m| m == "image")
    }

    /// 是否支持推理。没显式声明时按「有档位就支持」推断。
    pub fn reasoning_or_infer(&self) -> bool {
        self.reasoning
            .unwrap_or(!self.reasoning_efforts.is_empty())
    }

    /// 把这个模型里母版**明确填了**的字段合并到 `target` 上。
    ///
    /// 这是「只改我们声明管的部分」原则在模型粒度的落地：
    /// 母版留空的字段一律不动，保留 agent 配置里的原值。
    /// `target` 是刚从目标配置读出来的那个模型。
    pub fn merge_into(&self, target: &mut ModelSpec) {
        if let Some(v) = &self.name {
            target.name = Some(v.clone());
        }
        if let Some(v) = self.context_window {
            target.context_window = Some(v);
        }
        if let Some(v) = self.max_output_tokens {
            target.max_output_tokens = Some(v);
        }
        if !self.input_modalities.is_empty() {
            target.input_modalities = self.input_modalities.clone();
        }
        if !self.output_modalities.is_empty() {
            target.output_modalities = self.output_modalities.clone();
        }
        if let Some(v) = self.reasoning {
            target.reasoning = Some(v);
        }
        if !self.reasoning_efforts.is_empty() {
            target.reasoning_efforts = self.reasoning_efforts.clone();
        }
        if let Some(v) = &self.default_effort {
            target.default_effort = Some(v.clone());
        }
        if let Some(v) = self.tool_call {
            target.tool_call = Some(v);
        }
        if let Some(v) = &self.cost {
            target.cost = Some(v.clone());
        }
    }
}

/// 母版里 models 字段的反序列化。
///
/// 同时接受两种写法：
/// - `models = ["a", "b"]`（存量母版全是这种）
/// - `models = [{ id = "a", context_window = 1000000 }]`（带元数据）
///
/// 用 untagged 让 serde 自己判断每个元素是字符串还是表。
pub fn de_models<'de, D>(deserializer: D) -> Result<Vec<ModelSpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Item {
        Id(String),
        Full(ModelSpec),
    }

    let raw = Vec::<Item>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|m| match m {
            Item::Id(s) => ModelSpec::new(s),
            Item::Full(spec) => spec,
        })
        .collect())
}

/// 母版里 models 字段的序列化。
///
/// TOML 不允许一个数组里同时混字符串和表，所以按整个 provider 判断：
/// - 所有模型都只有 id → 写 `["a", "b"]`，存量文件形状不变
/// - 有任一模型带元数据 → 整组写成内联表，保证语法合法
pub fn ser_models<S>(models: &[ModelSpec], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;

    let any_rich = models.iter().any(|m| !m.is_bare());
    let mut seq = serializer.serialize_seq(Some(models.len()))?;
    for m in models {
        if any_rich {
            seq.serialize_element(m)?;
        } else {
            seq.serialize_element(&m.id)?;
        }
    }
    seq.end()
}


/// 母版里的一条 provider 记录。
///
/// 一个 provider 只对应**一个** api_key —— 这是"唯一来源"这个目标的落点。
/// 如果实际存在多把 key，收敛时只保留一把，另一把进备份。
///
/// ## 协议为什么要按 agent 分开
///
/// 实测同一个端点在不同 agent 里用的协议可以完全不同：
///
/// | 端点 | ZCode 里写的 | Pi 里写的 |
/// |---|---|---|
/// | `api.commandcode.ai/provider/v1` | `openai-compatible` | `openai-completions` |
/// | `opencode.ai/zen/go` | `anthropic` | `openai-responses` |
///
/// 所以协议不是 provider 的属性，是「provider × agent」的属性。
/// `kind` 保留为**默认值**（新增 provider 时不用逐个 agent 填），
/// 具体的按 agent 覆盖写在 `agent_kinds` 里。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    /// 稳定标识，用作各 agent 配置里的键
    pub id: String,
    /// 展示名
    pub name: String,
    /// API 端点
    pub base_url: String,
    /// 默认协议。`agent_kinds` 里没写的 agent 用这个
    #[serde(default)]
    pub kind: ProviderKind,
    /// 按 agent 覆盖的协议。key 是 agent id，value 是协议
    ///
    /// 只在某几个 agent 用的协议和默认值不同时才需要写。
    /// 比如同一个中转端点，ZCode 走 anthropic、Pi 走 openai-responses。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub agent_kinds: std::collections::BTreeMap<String, ProviderKind>,
    /// 唯一的 API key。明文存放，靠 .gitignore 隔离
    pub api_key: String,
    /// 这个 provider 支持的模型。可以只写 id，也可以带完整能力元数据
    #[serde(default, deserialize_with = "de_models", serialize_with = "ser_models")]
    pub models: Vec<ModelSpec>,
    /// 要注入到哪些 agent。未列出的 agent 不会被改动
    #[serde(default)]
    pub agents: Vec<String>,
    /// 备注，自由文本。用于记录 key 是哪天从哪拿的
    #[serde(default)]
    pub note: String,
}

/// 协议类型。
///
/// 四档对应业界实际存在的四种 API 协议，不是随便切的：
///
/// - **Anthropic Messages**：Claude 官方协议，Anthropic 兼容网关走这条
/// - **OpenAI Chat Completions**：`/v1/chat/completions`，最通用
/// - **OpenAI Responses**：`/v1/responses`，Codex 唯一支持的协议
/// - **Google Generative AI**：Gemini 原生协议
///
/// 各家 agent 的词表不一样，注入时按各自映射：
///
/// | 本枚举 | ZCode kind | Pi api | Codex wire_api | opencode package |
/// |---|---|---|---|---|
/// | Anthropic | anthropic | anthropic-messages | — | anthropic |
/// | OpenaiChat | openai-compatible | openai-completions | — | openai-compatible |
/// | OpenaiResponses | openai-compatible | openai-responses | responses | openai-compatible |
/// | GoogleGenAi | openai-compatible | google-generative-ai | — | — |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderKind {
    /// Anthropic Messages 协议。默认值，因为国内多数中转走这条
    #[default]
    Anthropic,
    /// OpenAI Chat Completions（`/v1/chat/completions`）
    OpenaiChat,
    /// OpenAI Responses（`/v1/responses`）。Codex 只支持这个
    OpenaiResponses,
    /// Google Generative AI（Gemini 原生）
    GoogleGenAi,
}

/// 自定义反序列化。
///
/// 不能直接用 serde 自动派生，因为要兼容旧写法：
/// 早期版本只有 `anthropic` 和 `openai-compatible` 两档，
/// 已有母版文件里写的是后者。直接派生会因为未知变体报错，
/// 让用户的存量配置全部失效。
impl<'de> Deserialize<'de> for ProviderKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::parse(&s))
    }
}

impl Serialize for ProviderKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl ProviderKind {
    /// 从配置文件的字符串解析。兼容旧的 `openai-compatible` 写法。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "anthropic" | "anthropic-messages" => Self::Anthropic,
            "openai-responses" | "responses" => Self::OpenaiResponses,
            "google-generative-ai" | "google" | "gemini" => Self::GoogleGenAi,
            // "openai-compatible" / "openai" / "openai-chat" / 其它都归到 chat
            _ => Self::OpenaiChat,
        }
    }

    /// 转成配置里存的字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenaiChat => "openai-chat",
            Self::OpenaiResponses => "openai-responses",
            Self::GoogleGenAi => "google-generative-ai",
        }
    }

    /// 界面上显示的名字。
    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic Messages",
            Self::OpenaiChat => "OpenAI Chat Completions",
            Self::OpenaiResponses => "OpenAI Responses",
            Self::GoogleGenAi => "Google Generative AI",
        }
    }
}

impl Provider {
    /// 这个 provider 在指定 agent 下应该用哪种协议。
    ///
    /// 优先取 `agent_kinds` 里的覆盖值，没有再回落到 `kind`。
    pub fn kind_for(&self, agent: &str) -> ProviderKind {
        self.agent_kinds
            .get(agent)
            .copied()
            .unwrap_or(self.kind)
    }

    /// 是否给某个 agent 单独指定过协议。
    pub fn has_agent_kind(&self, agent: &str) -> bool {
        self.agent_kinds.contains_key(agent)
    }
}

/// 母版文件的完整结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderFile {
    #[serde(default, rename = "provider")]
    pub providers: Vec<Provider>,
}

/// 计划改动的一个文件。
///
/// 这是"写什么"和"写之前是什么"的完整记录，
/// 让执行阶段能在写入前做 hash 复查，出问题时也能回滚。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePlan {
    /// 目标文件路径
    pub path: String,
    /// 属于哪个 agent
    pub agent: String,
    /// 计划写入的完整内容
    #[serde(skip)]
    pub content: String,
    /// 计划时读到的原始内容 hash。执行前拿它复查文件有没有被外部改过
    pub pre_hash: String,
    /// 计划内容的 hash
    pub post_hash: String,
    /// 这个文件有没有实际变化。false 表示可以跳过写入
    pub changed: bool,
    /// 面向人的改动说明，一行一条
    pub summary: Vec<String>,
}

/// 一条差异。用于 `providers diff` 和界面预览。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub agent: String,
    pub path: String,
    /// add / update / remove / unchanged
    pub action: DiffAction,
    /// 改动对象，如 `provider.command` 或 `model_providers.ollama`
    pub target: String,
    /// 补充说明
    pub detail: String,
}

/// 差异的动作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffAction {
    /// 母版有、目标没有，需要新增
    Add,
    /// 两边都有但内容不同，需要更新
    Update,
    /// 母版没有、目标有，需要删除
    Remove,
    /// 两边一致，无需改动
    Unchanged,
    /// 需要人工决定（如多把 key 冲突）
    Conflict,
}

/// 一次 dry-run 的完整结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    /// 逐条差异
    pub lines: Vec<DiffLine>,
    /// 需要人工确认的项
    pub conflicts: Vec<String>,
    /// 涉及的 agent 数量
    pub agent_count: usize,
    /// 有改动的文件数
    pub changed_files: usize,
}

/// 同步执行的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub ok: bool,
    /// 实际写入的文件
    pub written: Vec<String>,
    /// 因内容未变而跳过的文件
    pub skipped: Vec<String>,
    /// 每个文件的备份位置
    pub backups: Vec<String>,
    /// 面向人的日志
    pub log: Vec<String>,
}
