//! CLI 子命令定义。
//!
//! 一个二进制两种运行方式：带子命令走命令行输出文本，不带则开 GUI。
//! 这样 npm 只需要分发一个 exe，不用维护两套逻辑。

use clap::{Parser, Subcommand};

/// 统一管理多个 AI coding agent 的全局配置。
#[derive(Debug, Parser)]
#[command(name = "agentcenter", version, about, long_about = None)]
pub struct Cli {
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

    /// skill 相关操作
    #[command(subcommand)]
    Skills(SkillsCmd),

    /// API key 体检（只读）
    #[command(subcommand)]
    Creds(CredsCmd),

    /// provider 统一管理（母版与注入）
    #[command(subcommand)]
    Providers(ProvidersCmd),

    /// MCP server 统一管理
    #[command(subcommand)]
    Mcp(McpCmd),

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
    Collect {
        /// 真正执行，不加则只预览
        #[arg(long)]
        apply: bool,
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

    /// 生成示例母版
    Example,
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
    },

    /// 从母版里删除一条 provider
    Remove {
        /// provider 标识
        id: String,
    },

    /// 显示分配矩阵：每个 provider 用在哪些 agent 上
    Matrix,

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
