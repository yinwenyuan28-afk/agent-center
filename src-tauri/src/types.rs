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
    /// 用户取消了这个映射。为 true 时目标已删掉，同步也不会再建回来
    pub disabled: bool,
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
/// 归属的 agent 列表。空数组 = 公共 skill，放在中心库。
/// 直接读中心库目录的 agent 可见；其余 agent 需要在各自目录建链接才能读到
pub agents: Vec<String>,
    /// 专用 skill 的本体目录。公共 skill 为空
    pub store_path: Option<String>,
/// 专用 skill 在各 agent 目录里链接的健康状态。
/// 公共 skill 没有按 agent 的链接可查，这里恒为 `ok` —— 它只表示
/// 「中心库里有这个目录」，不代表每个 agent 都真的能读到
pub link_state: PrivateLinkState,
}

/// 专用 skill 的链接健康状态。
///
/// 专有性完全靠链接维持：本体在 `~/.agents/skills-mapping-<agent>/`，
/// agent 目录里的那一份是指向它的链接。链接一断，agent 就悄悄读不到这个 skill，
/// 而界面上没有任何报错 —— 所以必须显式建模这个状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrivateLinkState {
    /// 所有归属 agent 的位置都是指向本体的有效链接，或者本身就是本体（第一个 agent）
    Ok,
    /// 位置存在但是个断链，指向的目录已经不在了
    Broken,
    /// 位置根本不存在，需要建链接
    Missing,
}

/// 散落在别的 agent 目录里、尚未收进 `~/.agents` 的 skill。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanSkill {
    pub name: String,
    /// 出现在哪个 agent 目录，如 `.cursor/skills`
    pub location: String,
    /// 哪个 agent 的目录，如 `cursor`。界面据此显示「标记为 cursor 专用」
    ///
    /// 从路径反推不出来也可以为 None（比如用户手工传了个陌生路径），
    /// 那时界面不提供「标记专用」这个动作。
    pub agent: Option<String>,
    /// 完整路径
    pub path: String,
    /// 与中心库同名 skill 的比对结果
    pub relation: OrphanRelation,
    /// 散落这一份的 SKILL.md 描述。冲突时用来和中心库那份对照
    pub description: String,
    /// 目录内文件数
    pub file_count: usize,
    /// 目录字节数
    pub size_bytes: u64,
    /// 内容指纹前 8 位
    pub hash: String,
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

impl OrphanRelation {
    /// 从命令行字符串解析。接受 kebab-case 和常见的下划线/驼峰写法。
    pub fn parse(s: &str) -> Option<Self> {
        let norm = s.trim().to_lowercase().replace('_', "-");
        match norm.as_str() {
            "unique" => Some(Self::Unique),
            "identical" => Some(Self::Identical),
            "conflict" => Some(Self::Conflict),
            "broken" | "broken-link" => Some(Self::BrokenLink),
            _ => None,
        }
    }

    /// 转成命令行/JSON 里的字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::Identical => "identical",
            Self::Conflict => "conflict",
            Self::BrokenLink => "broken-link",
        }
    }
}

/// 用户对一条散落 skill 选定的处置方式。
///
/// 界面上每个散落条目按 `relation` 给出可选项，用户点哪个就走哪个。
/// 没有「保留不动」这一项 —— 那就是不点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrphanAction {
    /// 移入中心库，并在原位置建链接指回来（agent 仍能读到）
    Import,
    /// 中心库那份留着，把散落副本删掉，原位置改建链接
    Dedupe,
    /// 冲突时采用散落这一份：覆盖中心库同名目录，原位置改建链接
    TakeOrphan,
    /// 冲突时采用中心库那一份：删掉散落目录，原位置改建链接
    TakeShared,
    /// 只删掉散落的那份，不建链接（彻底断开）
    Delete,
    /// 坏链接：删掉链接文件本身，不碰任何真实内容
    RemoveLink,
    /// 标记为当前 agent 专用：移进 `~/.agents/skills-mapping-<agent>`，
    /// 原位置建链接，并写一条归属记录。不进公共库
    Privatize,
}

impl OrphanAction {
    /// 从命令行字符串解析。
    ///
    /// 同时接受 kebab-case（`take-orphan`）和下划线写法，
    /// 以及几个直观别名（`use-orphan` / `use-shared` / `link` / `remove`），
    /// 因为不同人对同一动作的说法不一样，逼人记唯一种写法没意义。
    pub fn parse(s: &str) -> Option<Self> {
        let norm = s.trim().to_lowercase().replace('_', "-");
        match norm.as_str() {
            "import" | "move-in" => Some(Self::Import),
            "dedupe" | "dedup" => Some(Self::Dedupe),
            "take-orphan" | "use-orphan" | "keep-orphan" => Some(Self::TakeOrphan),
            "take-shared" | "use-shared" | "keep-shared" => Some(Self::TakeShared),
            "delete" | "rm" => Some(Self::Delete),
            "remove-link" | "unlink" => Some(Self::RemoveLink),
            "privatize" | "mark-private" | "exclusive" => Some(Self::Privatize),
            _ => None,
        }
    }

    /// 转成命令行/JSON 里的字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Dedupe => "dedupe",
            Self::TakeOrphan => "take-orphan",
            Self::TakeShared => "take-shared",
            Self::Delete => "delete",
            Self::RemoveLink => "remove-link",
            Self::Privatize => "privatize",
        }
    }

    /// 这个动作要求散落副本和中心库处于哪种关系。
    ///
    /// 用来在动手前拦掉明显不对的组合 —— 比如对一个内容相同的副本
    /// 选「采用散落版」，那是把同一份内容自己覆盖自己，虽然不会出错，
    /// 但会白白备份一次，也说明用户没看清扫描结果。
    ///
    /// 返回空数组表示「任何关系都可以」（`Delete` / `RemoveLink` 属于这类）。
    pub fn allowed_relations(self) -> &'static [OrphanRelation] {
        use OrphanRelation::*;
        match self {
            Self::Import => &[Unique, Conflict],
            Self::Dedupe => &[Identical, Conflict],
            Self::TakeOrphan => &[Conflict, Unique],
            Self::TakeShared => &[Conflict, Identical],
            Self::Delete => &[],
            Self::RemoveLink => &[],
            // 中心库已经有同名 skill 时，这个 skill 的内容是"公共的"，
            // 把它标成某个 agent 专用等于把别人也在用的东西拿走。
            // 只允许对中心库没有的（unique）做这个动作。
            Self::Privatize => &[Unique],
        }
    }
}

/// 一个 agent 的指令文档现状：通用片段 + 专用片段 + 合成产物。
///
/// 界面用它渲染片段选项卡和"待生成"提示。纯只读快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDoc {
    /// agent 标识，如 `cursor`
    pub agent: String,
    /// 展示名
    pub label: String,
    /// 通用母版路径（`~` 形式）
    pub common_path: String,
    /// 该 agent 的指令文件最终落点，如 `~/.cursor/AGENTS.md`
    pub target: String,
    /// 专用片段路径，无论存不存在都给出来，方便界面做"新建"
    pub specific_path: String,
    /// 专用片段是否存在。false 表示这个 agent 直接读通用母版
    pub specific_exists: bool,
    /// 合成产物路径。没有专用片段时为 None
    pub composed_path: Option<String>,
    /// 合成产物与当前片段内容不一致，需要重建
    pub composed_stale: bool,
    /// 这个 agent 最终会读到的字节数（有专用片段时是合成后的大小）
    pub bytes: u64,
    /// 该 agent 的配置目录是否存在。false 表示工具未安装
    pub installed: bool,
}

/// 一条处置执行结果。界面把它逐条显示在 dialog 下方。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanActionResult {
    pub name: String,
    pub path: String,
    /// 用的哪种处置
    pub action: OrphanAction,
    pub ok: bool,
    /// 人话说明，成功或失败都写这里
    pub message: String,
}

/// 专用 skill 的一条链接现场状态。
///
/// 一条专用记录可能对应多个 agent，每个 agent 目录里的链接单独报告 ——
/// 用户要知道具体是哪一处断了，只说「这个 skill 有问题」没法修。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateLinkView {
    /// agent 标识
    pub agent: String,
    /// agent 展示名
    pub label: String,
    /// 该 agent 的 skills 目录里这个 skill 的位置
    pub path: String,
    /// 这一处的状态
    pub state: PrivateLinkState,
    /// 实际链接方式，不是链接时为空
    pub actual: Option<LinkKind>,
    /// 补充说明，如「指向的不是本体」
    pub note: Option<String>,
}

/// 一条专用 skill 记录的完整视图：本体位置 + 每个 agent 的链接状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMappingView {
    /// skill 名
    pub skill: String,
    /// 归属 agent 列表
    pub agents: Vec<String>,
    /// 本体所在目录
    pub store_path: String,
    /// 本体是否存在
    pub store_exists: bool,
    /// 各 agent 的链接状态
    pub links: Vec<PrivateLinkView>,
    /// 整体状态：全部 ok 才 ok，否则取最差的那个
    pub state: PrivateLinkState,
    /// 配置里要求的链接方式
    pub mode: LinkMode,
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
    /// 当前分支是否配置了上游。false 时 ahead/behind 没有意义
    pub has_upstream: bool,
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
/// **`api_key` 是明文原文**。母版文件 `~/.agents/providers.toml` 本身就是明文，
/// 界面也跑在本机 WebView 里，所以直接把真 key 给到前端 —— 编辑框能显示当前值，
/// 不用让用户凭「末尾 4 位」猜。`key_len` / `key_tail` 保留，用于列表页判断
/// 有没有配 key，以及侧栏等不需要原文的地方。
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
    /// Codex 是否绕过 ChatGPT 后端（`requires_openai_auth`）。
    ///
    /// `null` 表示交给注入器按盘上登录态自动判断；`true` / `false` 是用户
    /// 在界面上强制指定。
    pub codex_requires_openai_auth: Option<bool>,
    /// 分配给 Codex 时写进 `config.toml` 顶层 `model` 的模型 id。
    ///
    /// Codex 的模型选择器只列官方目录，自定义 provider 的模型进不去，
    /// 所以只能靠这个字段决定用哪个。`None` 表示回退到 `models` 的第一个。
    pub codex_model: Option<String>,
    /// Codex 是否启用 WebSocket（写入注入器管的那张 provider 表）。
    ///
    /// `None` 表示不写这个键；`Some(false)` 关闭 WebSocket，避免端点不支持
    /// 时反复无效重连。
    pub codex_supports_websockets: Option<bool>,
    /// Codex 拉模型目录的地址（写入注入器管的那张 provider 表）。
    ///
    /// 自定义端点拉不到官方目录时补上，一般是 `<base_url>/models`。
    pub codex_model_catalog_url: Option<String>,
    pub note: String,
    /// key 明文原文。界面编辑框直接显示它
    pub api_key: String,
    /// key 长度，用于界面提示"已配置"
    pub key_len: usize,
    /// key 末尾 4 位，帮助辨认
    pub key_tail: String,
}

/// Codex 连接参数的实际状态，读自 `~/.codex/config.toml`。
///
/// 界面用它预填「连接设置」：母版里可能还没写过这两个键，但用户可能
/// 直接改过配置文件，所以显示值要以盘上为准。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConnectionStatus {
    /// 配置文件是否存在
    pub available: bool,
    /// 实际读写的文件路径，界面展示用
    pub path: String,
    /// 当前生效的 provider 表名（顶层 `model_provider`）。
    /// `None` 表示表不存在，界面禁用写入
    pub provider_id: Option<String>,
    /// 表里的 base_url，用来猜模型目录地址
    pub base_url: Option<String>,
    /// 当前 `supports_websockets`；`None` 表示文件里没写这个键
    pub supports_websockets: Option<bool>,
    /// 当前 `model_catalog_url`；`None` 表示文件里没写这个键
    pub model_catalog_url: Option<String>,
    /// 当前 `requires_openai_auth`；`None` 表示没写（Codex 自动判断）
    pub requires_openai_auth: Option<bool>,
}

/// Claude Code 首次启动引导（onboarding）开关的状态。
///
/// 对应 `~/.claude.json` 根上的 `hasCompletedOnboarding`。这个标记
/// 和 provider 无关，是「这台机器上的 Claude Code 是否已完成引导」。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOnboardingStatus {
    /// 实际读写的文件路径，界面展示用
    pub path: String,
    /// Claude Code 装没装（`~/.claude.json` 或 `~/.claude` 存在即可）
    pub available: bool,
    /// 当前是否已跳过引导
    pub skipped: bool,
}

/// 一条差异。前端展示用。
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

// ---- Cursor 的自带密钥（BYOK） ----

/// Cursor 当前的 BYOK 配置，界面展示用。
///
/// **不含密钥原文**。只给长度和末尾 4 位，和 provider 列表的规则一致。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorByokState {
    /// Cursor 库文件存在
    pub installed: bool,
    /// `Use OpenAI API Key` 开关。false 时自定义模型一定用不了
    pub use_openai_key: bool,
    /// 当前接口地址；null 表示没覆盖，走 OpenAI 官方
    pub base_url: Option<String>,
    /// 库里存着的密钥长度。0 表示没存过
    pub key_len: usize,
    /// 密钥末尾 4 位，方便核对是不是同一把
    pub key_tail: String,
    /// 已添加的自定义模型名
    pub user_added_models: Vec<String>,
    /// 库文件路径
    pub db_path: String,
    /// Cursor 进程是否在运行。运行中不允许写入
    pub running: bool,
}

/// Cursor 还差哪些改动才能让某个 provider 生效。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorByokPlan {
    /// 要打开/关闭总开关时给出目标值
    pub set_use_openai_key: Option<bool>,
    /// 要改接口地址时给出目标值
    pub set_base_url: Option<String>,
    /// 要不要写密钥
    pub write_key: bool,
    /// 要加进模型选择器的名字
    pub add_models: Vec<String>,
    /// 要从模型选择器里删掉的名字
    pub remove_models: Vec<String>,
    /// 面向人的说明，一行一条
    pub summary: Vec<String>,
    /// 什么都不用做
    pub noop: bool,
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

// ---- token 用量统计 ----

/// 一条按 天 × agent × 模型 聚合的用量。
///
/// `input` 是**归一后**的输入量：含缓存命中的部分。
///
/// 四个 agent 的原始口径不一致，直接相加会算错：
/// - pi：`input` 不含缓存（`total = input + output + cacheRead + cacheWrite`）
/// - zcode / codex：`input` 已含缓存读取，缓存是它的子集
/// - claude：`input` 不含缓存，`cache_read` / `cache_creation` 另计
///
/// 统一折算成「输入含缓存」后，四家的数字才可比。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    /// 本地日期，形如 `2026-11-12`
    pub date: String,
    /// agent 标识，如 `zcode`
    pub agent: String,
    /// 模型标识，如 `deepseek/deepseek-v4.1-flash`
    pub model: String,
    /// 输入 token，已归一为「含缓存命中」
    pub input: u64,
    /// 输出 token
    pub output: u64,
    /// 请求次数（一次模型调用算一次，不含打印等非模型动作）
    pub calls: u64,
}

/// 一个 agent 的统计状态：能不能统计、开没开、读到了多少条。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAgentStatus {
    /// agent 标识
    pub agent: String,
    /// 展示名
    pub label: String,
    /// 这个 agent 有没有实现用量采集
    pub supported: bool,
    /// 用户是否勾选了「纳入统计」
    pub enabled: bool,
    /// 数据源在不在（本机有没有对应的库或目录）
    pub available: bool,
    /// 数据源位置，给用户看
    pub source: String,
    /// 口径说明或限制，如「Codex 压缩过的历史文件未纳入」
    pub note: String,
    /// 本次实际读到的用量记录条数
    pub records: u64,
    /// 读取失败的原因。空串表示正常
    pub error: String,
}

/// 用量统计结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    /// 统计窗口天数。0 表示不限
    pub days: u32,
    /// 每个 agent 的状态，顺序同侧栏
    pub agents: Vec<UsageAgentStatus>,
    /// 按天聚合的明细，日期倒序
    pub rows: Vec<UsageRow>,
    /// 折线图与饼图的数据
    pub charts: UsageCharts,
}

/// 折线图的一个点：某天某模型的总量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    /// 本地日期 `2026-11-12`
    pub date: String,
    /// 模型标识
    pub model: String,
    /// 输入 + 输出（归一后）
    pub tokens: u64,
}

/// 饼图的一块：某模型的合计与占比。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelShare {
    pub model: String,
    /// 输入 + 输出
    pub tokens: u64,
    /// 占总量的百分比，0~100，保留一位小数
    pub percent: f64,
    /// 调用次数
    pub calls: u64,
}

/// 图表数据。与 [`UsageReport`] 分开，因为图表只需要两种视角的汇总，
/// 不需要按 agent 拆开。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCharts {
    /// 折线图每条线的名称，按总量降序
    pub models: Vec<String>,
    /// 折线图数据点
    pub trend: Vec<TrendPoint>,
    /// 饼图数据，按总量降序
    pub shares: Vec<ModelShare>,
    /// 折线图的横轴日期，升序
    pub dates: Vec<String>,
}
