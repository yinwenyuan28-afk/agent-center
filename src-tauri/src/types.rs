//! 前后端共享的数据结构。
//!
//! 这里的每个类型都会被 serde 序列化后交给前端，字段名必须与
//! `src/lib/types.ts` 里的 TypeScript 定义逐字对应，否则界面会拿到 undefined。

use serde::{Deserialize, Serialize};

/// 链接的实际落地方式。
///
/// 配置里可以写 `auto` 让工具自己选；运行结果的 `actual` 只会是后四种之一。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    /// 真正的符号链接。Windows 上需要管理员权限或开发者模式。
    Symlink,
    /// 目录连接（junction）。Windows 上无需任何权限，但只能用于目录。
    Junction,
    /// 硬链接。无需权限，但只能用于文件且必须在同一卷上。
    Hardlink,
    /// 文件副本。前三种都失败时的最后手段，靠 manifest 里的指纹检测漂移。
    Copy,
}

/// 配置里允许出现的方式，比 [`LinkKind`] 多一个 `auto`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    Auto,
    Symlink,
    Junction,
    Hardlink,
    Copy,
}

/// 单条映射的健康状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    /// 链接完好，或者副本内容与母版一致。
    Ok,
    /// 副本模式下目标文件被改动过，与记录的指纹不符。
    Drifted,
    /// 链接指向的目标不存在。
    Broken,
    /// 目标尚未建立。
    Missing,
}

/// 一个 agent 的指令文件映射及其当前状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    /// agent 标识，如 `claude`、`zcode`
    pub agent: String,
    /// 展示用名称
    pub label: String,
    /// 母版路径
    pub source: String,
    /// 目标路径
    pub target: String,
    /// 配置里要求的方式
    pub mode: LinkMode,
    /// 实测落地的方式；目标不存在时为空
    pub actual: Option<LinkKind>,
    /// 健康状态
    pub health: HealthState,
    /// 该 agent 的配置目录是否存在。false 表示工具未安装，同步时跳过
    pub installed: bool,
    /// 补充说明
    pub note: Option<String>,
}

/// 一个已收集的 skill。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// 目录名，也是唯一标识
    pub name: String,
    /// frontmatter 里的 description
    pub description: String,
    /// 标签列表
    pub tags: Vec<String>,
    /// 是否由 npx skills 管理
    pub managed: bool,
    /// 受管时的来源仓库
    pub source: Option<String>,
    /// 内容指纹前 8 位
    pub hash: String,
    /// 目录内文件数
    pub file_count: usize,
    /// 目录字节数
    pub size_bytes: u64,
}

/// 散落在别的 agent 目录里、尚未收进 `~/.agents` 的 skill。
#[derive(Debug, Clone, Serialize)]
pub struct OrphanSkill {
    pub name: String,
    /// 出现在哪个 agent 目录，如 `.cursor/skills`
    pub location: String,
    /// 完整路径
    pub path: String,
    /// 与中心库同名 skill 的比对结果
    pub relation: OrphanRelation,
}

/// 散落副本与中心库的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrphanRelation {
    /// 中心库里没有，可以安全移入
    Unique,
    /// 内容完全相同，只需删掉副本
    Identical,
    /// 同名但内容不同，需要人工决定
    Conflict,
    /// 是个坏掉的链接，只报告
    BrokenLink,
}

/// 一次 collect 操作的完整计划，dry-run 时就是这个结构。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectPlan {
    /// 会移入中心库的
    pub to_import: Vec<OrphanSkill>,
    /// 内容相同、只删副本的
    pub to_dedupe: Vec<OrphanSkill>,
    /// 内容冲突、需要人工决定的
    pub conflicts: Vec<OrphanSkill>,
    /// 坏链接，只报告
    pub broken: Vec<OrphanSkill>,
}

/// 一个有改动的文件。
#[derive(Debug, Clone, Serialize)]
pub struct GitChange {
    pub path: String,
    /// M 修改 / A 新增 / D 删除 / ?? 未跟踪
    pub status: String,
}

/// git 状态快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub changed: Vec<GitChange>,
    pub remote: String,
}

/// npx skills 的受管 skill 快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedSkill {
    pub name: String,
    pub source: String,
    pub source_type: String,
    /// 分支/标签/SHA，没有则为空
    pub r#ref: Option<String>,
    /// 内容指纹前 8 位
    pub hash_short: String,
    pub installed_at: String,
    pub updated_at: String,
    /// 是否检测到可用更新；None 表示未检查
    pub has_update: Option<bool>,
}

/// 命令执行结果，用于把 CLI 的原始输出透传给界面日志区。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub ok: bool,
    pub exit_code: i32,
    /// 合并后的 stdout 与 stderr
    pub output: String,
}

impl RunResult {
    /// 构造一个成功结果。
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            exit_code: 0,
            output: output.into(),
        }
    }

    /// 构造一个失败结果，保留退出码供调用方判断。
    pub fn failure(exit_code: i32, output: impl Into<String>) -> Self {
        Self {
            ok: false,
            exit_code,
            output: output.into(),
        }
    }
}

// ---- API key 体检 ----
//
// 注意：这组结构里**没有任何密钥字段**。密钥原文只在 Rust 内存里
// 停留于算指纹的瞬间，从不跨进程边界传输。

/// 一条凭证的体检信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntry {
    /// 哪个 agent 在用，如 claude / zcode / pi
    pub agent: String,
    /// 服务商标识：base URL 或环境变量名
    pub provider: String,
    /// api_key 或 oauth
    pub kind: String,
    /// 具体落在哪个文件、哪个字段
    pub location: String,
    /// 密钥指纹前 8 位，用于识别是否同一把钥匙
    pub fingerprint: String,
    /// 密钥字符数
    pub length: usize,
    /// 密钥末尾 4 位，帮助人辨认
    pub tail: String,
    /// 同一个指纹还出现在哪些别的 agent 上
    pub shared_with: Vec<String>,
    /// 是否在多个 agent 上重复
    pub duplicated: bool,
}

/// 读不到明文、需要用户自己登录的 agent。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedAgent {
    pub agent: String,
    pub reason: String,
}

/// 一次完整体检的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialOverview {
    /// 按 provider 分组后的全部凭证
    pub providers: Vec<Vec<CredentialEntry>>,
    /// 读不到明文的 agent
    pub unsupported: Vec<UnsupportedAgent>,
    /// 凭证总条数（同一把 key 在 3 个 agent 上算 3 条）
    pub total_entries: usize,
    /// 不同的 key 数量
    pub unique_keys: usize,
    /// 重复的 key 组数
    pub duplicated_keys: usize,
    /// 配了多把不同 key 的 provider 数量
    pub providers_with_multiple_keys: usize,
}

// ---- provider 统一管理 ----

/// 一个模型的能力描述。
///
/// 母版最早只存模型 id 字符串，元数据全靠注入器里写死的常量
/// （上下文 1M、输出 128K）。但真实配置里各家差别很大：
/// Pi 的 wangxin 是 32768/8192，opencode-go 是 1048576/131072 还带价格；
/// ZCode 里有的模型支持 image+video，有的只有 text。写死就把这些抹平了。
///
/// 所以把这些信息建模进母版。除 `id` 外全部可选 ——
/// 不填表示「不指定，沿用 agent 配置里的现有值或内置默认」，
/// 这样存量只写了 id 的母版升级后行为不变。
///
/// ## 字段名为什么两套
///
/// 这个结构同时用于两处：
/// - 母版 `providers.toml`：TOML 惯例是 snake_case
/// - 前端 JSON：TS 惯例是 camelCase
///
/// 用 `rename_all = "camelCase"` 让序列化输出 camelCase，
/// 再用 `alias` 让反序列化同时接受 snake_case 写法 —— 手写母版时
/// 写 `context_window` 和 `contextWindow` 都能读。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// 模型标识，调用 API 时传的名字。必填
    pub id: String,
    /// 展示名。留空时用 id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 最大输入 token 数（上下文窗口）
    #[serde(
        default,
        alias = "context_window",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_window: Option<u64>,
    /// 单次最大输出 token 数
    #[serde(
        default,
        alias = "max_output_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_output_tokens: Option<u64>,
    /// 能接受的输入类型：text / image / video / pdf
    ///
    /// 空数组也要序列化出去。前端把这几个字段当成必有数组，
    /// 若用 skip_serializing_if 省略，JSON 里就没有这个键，
    /// Providers 页一读 `.length` 就会在渲染时炸掉，看起来像卡在骨架屏。
    #[serde(default, alias = "input_modalities")]
    pub input_modalities: Vec<String>,
    /// 能产出的输出类型，通常只有 text
    #[serde(default, alias = "output_modalities")]
    pub output_modalities: Vec<String>,
    /// 是否支持推理/思考
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// 可选的推理档位，如 low / medium / high / max
    #[serde(default, alias = "reasoning_efforts")]
    pub reasoning_efforts: Vec<String>,
    /// 默认推理档位
    #[serde(
        default,
        alias = "default_effort",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_effort: Option<String>,
    /// 是否支持函数调用
    #[serde(
        default,
        alias = "tool_call",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_call: Option<bool>,
    /// 价格（每百万 token）。目前只有 Pi 的配置里有
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelCost>,
}

/// 模型价格。字段与 Pi 的 `cost` 对象一致。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(
        default,
        alias = "cache_read",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_read: Option<f64>,
    #[serde(
        default,
        alias = "cache_write",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_write: Option<f64>,
}

/// Claude Code 的档位映射：Opus / Sonnet / Haiku / 默认模型各自对应哪条模型 id。
///
/// 写入 `~/.claude/settings.json` 的
/// `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL` 与 `ANTHROPIC_MODEL`。
/// 全空表示未配置，注入器回退到按 `models` 下标推断。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaudeModelMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sonnet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub haiku: Option<String>,
    /// 对应 `ANTHROPIC_MODEL`。母版 TOML / JSON 都写成 `default`。
    #[serde(
        default,
        rename = "default",
        alias = "default_model",
        alias = "defaultModel",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_model: Option<String>,
}

impl ClaudeModelMap {
    pub fn is_empty(&self) -> bool {
        self.opus.is_none()
            && self.sonnet.is_none()
            && self.haiku.is_none()
            && self.default_model.is_none()
    }
}

/// 一条模型回填记录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillLine {
    pub provider: String,
    pub model: String,
    /// 从哪个 agent 读到的
    pub source: String,
    /// 填了哪些字段
    pub filled: Vec<String>,
}

/// 协议不一致：母版里这个 provider 在某个 agent 下的协议，
/// 和该 agent 配置里实际写的不同。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindMismatch {
    pub provider: String,
    pub agent: String,
    /// 母版要求的协议
    pub master: String,
    /// agent 配置里实际的协议
    pub actual: String,
}

/// 模型元数据回填报告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillReport {
    pub filled_fields: usize,
    pub lines: Vec<BackfillLine>,
    pub skipped_providers: Vec<String>,
    /// 协议不一致的条目。只报告，不自动改 —— 见 backfill.rs 里的说明
    pub kind_mismatches: Vec<KindMismatch>,
    pub wrote: bool,
}

/// 母版里的一条 provider，前端展示用。
///
/// **apiKey 不出现在这个结构里**。前端只能看到长度和末尾 4 位，
/// 编辑时要重新输入。这是有意的：密钥原文不跨进程边界。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// 协议标识：anthropic / openai-chat / openai-responses / google-generative-ai
    pub kind: String,
    /// 协议的展示名，如 "OpenAI Responses"
    pub kind_label: String,
    /// 按 agent 覆盖的协议。key 是 agent id，value 是协议标识。
    ///
    /// 协议不是 provider 的属性而是 provider × agent 的：
    /// 实测 opencode.ai/zen/go 在 ZCode 里是 anthropic、在 Pi 里是 openai-responses。
    /// 这里只列**和默认值不同**的那些。
    pub agent_kinds: std::collections::BTreeMap<String, String>,
    /// 模型列表，带能力元数据。只填了 id 的模型元数据字段都是空
    pub models: Vec<ModelSpec>,
    pub agents: Vec<String>,
    /// Claude Code 档位映射。未配置时为空对象
    pub claude_model_map: ClaudeModelMap,
    pub note: String,
    /// key 长度，用于界面提示"已配置"
    pub key_len: usize,
    /// key 末尾 4 位，帮助辨认
    pub key_tail: String,
}

/// 一条差异，前端展示用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiffLine {
    pub agent: String,
    pub path: String,
    /// add / update / remove / unchanged / conflict
    pub action: String,
    pub target: String,
    pub detail: String,
}

/// provider 差异报告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiffReport {
    pub lines: Vec<ProviderDiffLine>,
    pub conflicts: Vec<String>,
    pub agent_count: usize,
    pub changed_files: usize,
}

/// provider 分配矩阵：provider 池 × agent。
///
/// 和 MCP 的矩阵同形，只是行换成 provider。界面默认只列 provider，
/// 点某一行的「分配」才展开 agent 卡片。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMatrix {
    pub columns: Vec<AgentColumn>,
    pub providers: Vec<ProviderView>,
    /// 池子是空的
    pub empty: bool,
}

/// provider 同步结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncReport {
    pub ok: bool,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub backups: Vec<String>,
    pub log: Vec<String>,
}

// ---- MCP 统一管理 ----

/// 母版里的一条 MCP server，界面展示用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub id: String,
    pub name: String,
    /// stdio / sse / http
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub url: String,
    /// 环境变量名列表（值不返回给前端）
    pub env_keys: Vec<String>,
    /// 请求头名列表
    pub header_keys: Vec<String>,
    pub enabled: bool,
    pub agents: Vec<String>,
    pub note: String,
}

/// MCP 差异行。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiffLineView {
    pub agent: String,
    pub path: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

/// MCP 差异报告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiffReportView {
    pub lines: Vec<McpDiffLineView>,
    pub agent_count: usize,
    pub changed_files: usize,
}

/// MCP 同步结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSyncReportView {
    pub ok: bool,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub backups: Vec<String>,
    pub log: Vec<String>,
}

// ---- MCP 分配矩阵 ----

/// 矩阵的一列：一个 agent。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentColumn {
    pub agent: String,
    /// 展示名
    pub label: String,
    /// 配置文件是否存在。不存在时该列不可点
    pub available: bool,
    /// 读不到的原因
    pub note: String,
    /// 单槽位：同一时刻只能有一个条目在它上面生效。
    /// 界面点选时要自动把其他条目上的这个 agent 摘掉
    pub single_slot: bool,
    /// 这个 agent 支持哪些协议。空 = 全部四种。
    ///
    /// 界面给某个 agent 选协议时只列这些 —— Codex 只有 responses、
    /// Claude 只有 anthropic，列别的等于骗人。
    pub supported_kinds: Vec<String>,
}

/// 矩阵的一行：一个 MCP server 及其在各 agent 的分配。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRow {
    pub id: String,
    pub name: String,
    pub transport: String,
    /// 命令或 URL，用于识别
    pub target: String,
    /// 全局启用开关
    pub enabled: bool,
    /// 已分配给哪些 agent
    pub assigned: Vec<String>,
    pub note: String,
}

/// 完整的分配矩阵。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpMatrix {
    pub columns: Vec<AgentColumn>,
    pub rows: Vec<MatrixRow>,
    /// 池子是空的
    pub empty: bool,
}
