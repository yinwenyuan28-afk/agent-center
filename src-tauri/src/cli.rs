//! CLI 子命令定义。
//!
//! 一个二进制两种运行方式：带子命令走命令行输出文本，不带则开 GUI。
//! 这样 npm 只需要分发一个 exe，不用维护两套逻辑。

use clap::{Parser, Subcommand};

/// 统一管理多个 AI coding agent 的全局配置。
#[derive(Debug, Parser)]
#[command(name = "agentcenter", version, about, long_about = None)]
pub struct Cli {
    /// 以 JSON 输出结果，供脚本和 agent 解析。
    ///
    /// 只作用于读命令（status / mappings / agents / * list / * diff /
    /// * matrix / * inspect / usage report）。写命令仍输出人话文本。
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 把母版指令文件同步到各 agent 目录
    Sync {
        /// 只显示将要做什么，不写盘
        #[arg(long)]
        dry_run: bool,
    },

    /// 查看各映射的当前状态
    Status,

    /// 列出所有映射
    Mappings,

    /// 母版指令文件（~/.agents/AGENTS.md）的读写
    #[command(subcommand)]
    Doc(DocCmd),

    /// 取消某个 agent 的映射：删掉目标链接并记下「已取消」
    Unmap {
        /// agent 标识，如 claude / zcode
        agent: String,
    },

    /// 恢复某个被取消的映射：清掉标记并立刻重建链接
    Remap {
        /// agent 标识
        agent: String,
    },

    /// skill 相关操作
    #[command(subcommand)]
    Skills(SkillsCmd),

    /// API key 体检（只读）
    #[command(subcommand)]
    Creds(CredsCmd),

    /// provider 统一管理（母版与注入）
    #[command(subcommand)]
    Providers(ProvidersCmd),

    /// Claude Code 的首次引导开关（写 ~/.claude.json）
    #[command(subcommand)]
    Claude(ClaudeCmd),

    /// Claude 的本地协议转换端口（127.0.0.1:15722）
    #[command(subcommand)]
    Bridge(BridgeCmd),

    /// MCP server 统一管理
    #[command(subcommand)]
    Mcp(McpCmd),

    /// token 用量统计
    #[command(subcommand)]
    Usage(UsageCmd),

    /// 列出所有 agent 及其支持的能力
    Agents,

    /// 模型元数据相关操作
    #[command(subcommand)]
    Models(ModelsCmd),

    /// git 相关操作
    #[command(subcommand)]
    Git(GitCmd),

    /// 打开图形界面
    Ui,
}

/// 模型元数据子命令。
#[derive(Debug, Subcommand)]
pub enum ModelsCmd {
    /// 从各 agent 现有配置回填模型元数据到母版（只补空字段）
    Backfill {
        /// 只显示将补哪些字段，不写母版
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillsCmd {
    /// 列出中心库里的 skill
    List,

    /// 扫描散落在各 agent 目录的 skill
    Scan,

    /// 收集散落副本到中心库
    ///
    /// 只处理「中心库没有」和「内容完全相同」两类。冲突的必须逐条处置，
    /// 用 `skills apply`。
    Collect {
        /// 真正执行，不加则只预览
        #[arg(long)]
        apply: bool,
    },

    /// 对一条散落 skill 执行选定处置
    ///
    /// 和 `collect` 的区别：`collect` 按关系批量套固定策略，这个由调用方
    /// 逐条指定动作。每条都会重新校验现场，不信任先前 `scan` 的结果。
    Apply {
        /// skill 名。与 `--path` 二选一
        name: Option<String>,

        /// 散落副本的完整路径。`scan --json` 输出的 `path` 字段
        #[arg(long)]
        path: Option<String>,

        /// 处置动作：
        /// import（移入中心库）/ dedupe（删副本改链接）/
        /// take-orphan（采用散落版）/ take-shared（采用中心库版）/
        /// delete（只删不建链接）/ remove-link（删坏链接）
        #[arg(long)]
        action: Option<String>,

        /// 列出每条散落 skill 可用的动作，不执行任何操作
        #[arg(long)]
        dry_run: bool,
    },

    /// 给 skill 打标签
    Tag {
        /// skill 名
        name: String,
        /// 标签列表，用逗号分隔。传空串表示清空
        #[arg(long, value_delimiter = ',')]
        add: Vec<String>,
    },

    /// 检查第三方 skill 更新
    Check,

    /// 更新第三方 skill
    Update {
        /// 指定 skill 名，不传则全部
        names: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GitCmd {
    /// 查看仓库状态
    Status,

    /// 按指定路径提交
    Commit {
        /// 要提交的路径，逗号分隔
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,
        /// 提交信息
        #[arg(short, long)]
        message: String,
    },

    /// 推送当前分支
    Push,
}

/// API key 体检子命令。
///
/// 只读，不提供任何写入操作 —— 这个模块的边界就是"看清楚现状"。
#[derive(Debug, Subcommand)]
pub enum CredsCmd {
    /// 体检：列出所有 agent 的 key 分布、重复情况
    Inspect,
}

/// Claude Code 的首次引导开关。
///
/// 写的是 `~/.claude.json` 根上的 `hasCompletedOnboarding`，和 provider
/// 无关，所以单独一组命令，不在 `providers` 下面。
#[derive(Debug, Subcommand)]
pub enum ClaudeCmd {
    /// 查看当前是否跳过首次引导
    Status,

    /// 跳过首次引导（写 hasCompletedOnboarding = true）
    Skip,

    /// 恢复首次引导（删掉 hasCompletedOnboarding）
    Restore,
}

/// MCP server 统一管理子命令。
#[derive(Debug, Subcommand)]
pub enum McpCmd {
    /// 列出母版里的 MCP server
    List,

    /// 对比母版与各 agent 现状，不写盘
    Diff,

    /// 注入到各 agent
    Sync {
        /// 只显示将要做什么
        #[arg(long)]
        dry_run: bool,
    },

    /// 在母版里新增或更新一条 MCP server
    ///
    /// `--env` / `--header` 传完整键值对；更新时只覆盖显式给出的项。
    /// 想清空 env 用 `--clear-env`（单独一个开关，避免「不传」和「传空」
    /// 两种语义混在一起）。
    Add {
        /// server 标识，也是各 agent 配置里的键名
        id: String,
        /// 展示名。省略则用 id
        #[arg(long)]
        name: Option<String>,
        /// 传输方式：stdio / sse / http。省略按 stdio
        #[arg(long)]
        transport: Option<String>,
        /// stdio 的可执行文件
        #[arg(long)]
        command: Option<String>,
        /// stdio 的命令行参数，逗号分隔。
        ///
        /// 允许以 `-` 开头（`npx -y pkg` 这种），否则 clap 会把它当成
        /// 自己的参数报 unexpected argument
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        args: Vec<String>,
        /// stdio 的工作目录
        #[arg(long)]
        cwd: Option<String>,
        /// sse / http 的端点地址
        #[arg(long)]
        url: Option<String>,
        /// 环境变量，可重复。格式 `KEY=VALUE`（值里可以有 `=`）
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// 请求头，可重复。格式 `KEY=VALUE`
        #[arg(long = "header", value_name = "KEY: VALUE")]
        headers: Vec<String>,
        /// 停用这个 server
        #[arg(long)]
        disabled: bool,
        /// 分配给哪些 agent，逗号分隔。省略表示保持原值（新增时为空）
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
        /// 备注
        #[arg(long)]
        note: Option<String>,
        /// 清空全部环境变量
        #[arg(long)]
        clear_env: bool,
        /// 清空全部请求头
        #[arg(long)]
        clear_headers: bool,
    },

    /// 从母版里删除一条 MCP server
    ///
    /// 只删母版。已注入到各 agent 的条目要再跑一次 `mcp sync` 才会清理。
    Remove {
        /// server 标识
        id: String,
    },

    /// 整行设置某条 server 的分配列表（省略 --agents 表示清空）
    Assign {
        /// server 标识
        id: String,
        /// agent 名，逗号分隔
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
    },

    /// 显示分配矩阵：每条 server 分配给哪些 agent
    Matrix,

    /// 从各 agent 现有配置导入 server 到母版
    Import,

    /// 生成示例母版
    Example,
}

/// 母版指令文件的读写。
///
/// 母版是 `~/.agents/AGENTS.md`（路径可在 agentcenter.toml 的
/// `[source].instructions` 改）。写它只改这一份文件，各 agent 的
/// 指令文件是它的链接或副本，要再跑 `sync` 才同步出去。
#[derive(Debug, Subcommand)]
pub enum DocCmd {
    /// 输出母版内容
    Read,

    /// 覆盖写入母版内容。内容从 --file 或标准输入读
    Write {
        /// 从文件读取内容。省略则读标准输入
        #[arg(long)]
        file: Option<String>,
    },

    /// 只在母版不存在时创建（已存在则拒绝，不覆盖）
    Create {
        /// 从文件读取内容。省略则读标准输入
        #[arg(long)]
        file: Option<String>,
    },

    /// 显示母版路径和是否存在
    Path,
}

/// Claude 本地协议转换端口的控制。
///
/// 上游不是 Anthropic 协议时，注入器会把 `ANTHROPIC_BASE_URL` 指向
/// `http://127.0.0.1:15722`，由本地转换层把 `/v1/messages` 翻成上游协议。
/// 这个端口原本只在图形界面启动时起，CLI 改完 Claude 配置后 Claude Code
/// 会连不上。这一组命令让 CLI 也能把它拉起来。
#[derive(Debug, Subcommand)]
pub enum BridgeCmd {
    /// 查看转换端口在不在听、配置指向哪里
    Status,

    /// 前台运行转换端口。Ctrl+C 结束
    Run,

    /// 后台启动一个脱离当前终端的转换进程
    Start,

    /// 停掉后台启动的转换进程
    Stop,
}

/// provider 统一管理子命令。
#[derive(Debug, Subcommand)]
pub enum ProvidersCmd {
    /// 列出母版里定义的 provider
    List,

    /// 对比母版与各 agent 现状，不写盘
    Diff,

    /// 把母版里的 provider 注入到各 agent
    Sync {
        /// 只显示将要做什么，不写盘
        #[arg(long)]
        dry_run: bool,
    },

    /// 在母版里新增或更新一条 provider
    Set {
        /// provider 标识
        id: String,
        /// 展示名
        #[arg(long)]
        name: Option<String>,
        /// API 端点
        #[arg(long)]
        base_url: Option<String>,
        /// API key
        #[arg(long)]
        api_key: Option<String>,
        /// 默认协议：anthropic / openai-chat / openai-responses / google-generative-ai
        #[arg(long)]
        kind: Option<String>,
        /// 按 agent 覆盖协议，可重复。格式 `<agent>=<协议>`，
        /// 如 `--agent-kind pi=openai-responses`
        #[arg(long = "agent-kind", value_name = "AGENT=KIND")]
        agent_kinds: Vec<String>,
        /// 模型 id，逗号分隔
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        /// 注入到哪些 agent，逗号分隔
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
        /// 分配给 Codex 时用哪个模型。必须出现在 `--models` 里。
        ///
        /// Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型
        /// 进不去，所以只能靠这个值写进 config.toml 顶层的 `model`。
        /// 不填就沿用 models 的第一个。
        #[arg(long = "codex-model", value_name = "MODEL")]
        codex_model: Option<String>,
    },

    /// 从母版里删除一条 provider
    Remove {
        /// provider 标识
        id: String,
    },

    /// 显示分配矩阵：每个 provider 用在哪些 agent 上
    Matrix,

    /// Cursor 的自带密钥（BYOK）配置。
    ///
    /// Cursor 的配置存在 SQLite 库里，不进 `providers sync`，必须单独操作，
    /// 而且要先把 Cursor 完全退出 —— 它开着时会用内存状态覆盖我们的写入。
    Cursor {
        #[command(subcommand)]
        cmd: CursorCmd,
    },

    /// 整行设置某个 provider 的分配列表（空串表示全不分配）
    Assign {
        /// provider 标识
        id: String,
        /// agent 名，逗号分隔。省略表示清空
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
    },

    /// 生成示例母版文件
    Example,

    /// 检查母版是否已被 .gitignore 忽略
    Doctor,
}

/// Cursor 的自带密钥（BYOK）子命令。
///
/// Cursor 的接口地址、总开关和密钥都在 `state.vscdb` 里，且密钥经
/// Electron safeStorage 加密。这一组命令复现「在 Cursor 设置里点一遍」
/// 那个流程：读状态 → 看还差什么 → 退出 Cursor → 写入 → 重启。
#[derive(Debug, Subcommand)]
pub enum CursorCmd {
    /// 看 Cursor 当前的 BYOK 状态：开关、接口地址、密钥长度、自定义模型
    Status,

    /// 只显示要改什么，不写库
    Plan {
        /// provider 标识，用母版里的 base_url 和 api_key
        provider: String,
        /// 要加进 Cursor 模型选择器的模型名，逗号分隔。
        /// 省略则不动模型列表
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
    },

    /// 把某个 provider 写进 Cursor。要求 Cursor 已完全退出
    Apply {
        /// provider 标识，用母版里的 base_url 和 api_key
        provider: String,
        /// 要加进 Cursor 模型选择器的模型名，逗号分隔。
        /// 省略则不动模型列表
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        /// 即使 Cursor 在运行也继续（不推荐，可能被覆盖）
        #[arg(long)]
        force: bool,
    },

    /// 从 Cursor 的模型选择器里删掉指定模型。只改这一项，不动开关/地址/密钥
    RemoveModel {
        /// 模型名，逗号分隔
        #[arg(value_delimiter = ',')]
        models: Vec<String>,
        /// 即使 Cursor 在运行也继续（不推荐）
        #[arg(long)]
        force: bool,
    },

    /// 把 Cursor 恢复成「不用自带密钥」：关开关、清地址
    Reset,
}

/// token 用量统计子命令。
///
/// 默认一个 agent 都不统计，要先 `usage enable <agent>` 打开。
#[derive(Debug, Subcommand)]
pub enum UsageCmd {
    /// 列出各 agent 的统计状态与启用情况
    Status,

    /// 统计 token 用量，按 天 × agent × 模型 输出
    Report {
        /// 统计最近多少天。0 表示全部历史
        #[arg(long, default_value_t = 7)]
        days: u32,

        /// 丢掉缓存，从头重算全部历史
        ///
        /// 平时不需要：统计是增量的，只读新增的数据。
        /// 只有在怀疑数字不对（比如手动删改过会话文件）时才用
        #[arg(long)]
        refresh: bool,
    },

    /// 打开某个 agent 的用量统计
    Enable {
        /// agent 标识，如 zcode / pi / codex / claude
        agent: String,
    },

    /// 关闭某个 agent 的用量统计
    Disable {
        /// agent 标识
        agent: String,
    },
}
