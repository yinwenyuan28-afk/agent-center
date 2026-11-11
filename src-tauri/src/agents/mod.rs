//! Agent 能力接口。
//!
//! ## 为什么要有这个模块
//!
//! 重构前，新增一个 agent 要改 10 个文件：`registry.rs` 加路径、
//! `providers/mod.rs` 加注入器、`providers/<agent>.rs` 写实现、
//! `providers/matrix.rs` 加一列和一条可用性判断、`providers/scan.rs`
//! 加扫描函数、`providers/diff.rs` 加路径、`mcp/inject.rs` 加注入器、
//! `mcp/matrix.rs` 加列、`mcp/scan.rs` 加扫描、`mcp/diff.rs` 加路径、
//! `credentials.rs` 加凭证扫描。
//!
//! 而且这些表已经开始互相漂移：`providers/diff.rs` 的路径表漏了
//! workbuddy/dsh，`mcp/matrix` 只有 7 列而 `mcp/scan` 有 9 项，
//! `credentials.rs` 把 gemini 列进「不支持」但 `registry.rs` 里根本没它。
//!
//! 现在每个 agent 的全部能力收进一个实现文件，新增只写一个文件 +
//! 在 [`all`] 里加一行。
//!
//! ## 设计要点：能力是选项，不是必答
//!
//! trait 里除了身份和路径，其余方法都有默认实现 —— 默认表示「这个 agent
//! 没有这项能力」。比如 Cursor 只有 MCP 和 skills，它的实现里
//! 完全不用提 `provider_plan`。这样不支持的 agent 不必写一堆空函数，
//! 也不会因为漏写某个方法而 panic。

pub mod claude;
pub mod codebuddy;
pub mod codex;
pub mod cursor;
pub mod dsh;
pub mod gemini;
pub mod hermes;
pub mod model;
pub mod opencode;
pub mod pi;
pub mod qoder;
pub mod qoder_cn;
pub mod trae_cn;
pub mod workbuddy;
pub mod zcode;
pub mod zed;

use crate::mcp::inject::McpFilePlan;
use crate::mcp::scan::McpAgentScan;
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use std::path::PathBuf;

// ModelSpec 定义在 providers/model.rs 里 —— 它属于母版结构，
// 这里转手导出，让 agent 实现只需 `use crate::agents::ModelSpec`
pub use crate::providers::model::{ModelCost, ModelSpec};
pub use model::RawEntry;

/// 一个 AI coding agent 的全部可管理能力。
///
/// 实现者只要覆盖自己支持的项，不支持的用默认实现（返回空/None）。
pub trait Agent: Send + Sync {
    // ---- 身份 ----

    /// 短标识，用作配置里的 `agent` 字段和母版 `agents` 数组里的值
    fn id(&self) -> &'static str;

    /// 界面展示名
    fn label(&self) -> &'static str;

    /// 界面排序，小的排前面。默认 100
    fn order(&self) -> u32 {
        100
    }

    // ---- AGENTS 文档映射 + skills ----

    /// 配置根目录。存在性被当作「这个 agent 装了」的判断依据
    fn config_dir(&self) -> PathBuf;

    /// 全局指令文件路径。None 表示这个 agent 没有单一指令文件
    /// （如 WorkBuddy 是 SOUL/IDENTITY/USER/MEMORY 一套，Qoder 只有工作区级）
    fn instruction_file(&self) -> Option<PathBuf> {
        None
    }

    /// skills 目录
    fn skills_dir(&self) -> PathBuf;

    /// 是否原生读取 `~/.agents/skills`。true 则不需要为共享 skill 建链接
    fn reads_shared_skills(&self) -> bool {
        false
    }

    /// 是否已安装。默认看配置根目录在不在
    fn installed(&self) -> bool {
        self.config_dir().exists()
    }

    // ---- provider 管理 ----

    /// 这个 agent **在设计上**支持 provider 管理。
    ///
    /// 和 [`Agent::provider_ready`] 的区别很重要：
    /// - `supports_provider` 是「我实现了 provider_plan」，恒为真或恒为假
    /// - `provider_ready` 是「现在这台机器能注入吗」，看配置文件在不在
    ///
    /// 母版里的分配关系用前者校验 —— 用户可以在没装某个 agent 的机器上
    /// 配置好分配，换台机器同步过去。用 `provider_ready` 校验会把这种
    /// 分配静默丢掉。
    fn supports_provider(&self) -> bool {
        false
    }

    /// 现在能注入吗（配置文件在不在）
    fn provider_ready(&self) -> bool {
        false
    }

    /// 是否单槽位：同一时刻只能有一个 provider 生效。
    ///
    /// 只有 Claude Code 是这样 —— 它没有 provider 列表的概念，
    /// 只有一组 `ANTHROPIC_*` 环境变量。界面上勾选某个 provider 时
    /// 要自动把其他 provider 上的这个 agent 摘掉。
    fn single_slot(&self) -> bool {
        false
    }

    /// 这个 agent 支持哪些协议。
    ///
    /// 界面上给某个 agent 选协议时只列这些 —— 列了做不到的选项，
    /// 用户选了也会被注入器静默改写，等于骗人。
    ///
    /// 返回空表示「支持全部四种」，这是默认值：多数 agent 走
    /// OpenAI 兼容层，四种协议都能表达。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        &[]
    }

    /// provider 涉及的配置文件，用于界面展示和 diff 报告
    fn provider_targets(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// 从现有配置读出 provider。不支持时返回 None
    fn provider_scan(&self) -> Option<AgentScan> {
        None
    }

    /// 计算 provider 注入计划。只读，不写盘
    fn provider_plan(&self, _providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        Ok(Vec::new())
    }

    // ---- MCP 管理 ----

    /// 这个 agent 在设计上支持 MCP 管理。语义同 [`Agent::supports_provider`]
    fn supports_mcp(&self) -> bool {
        false
    }

    /// 现在能注入吗
    fn mcp_ready(&self) -> bool {
        false
    }

    /// 从现有配置读出 MCP server。不支持时返回 None
    fn mcp_scan(&self) -> Option<McpAgentScan> {
        None
    }

    /// 计算 MCP 注入计划。只读，不写盘
    fn mcp_plan(&self, _servers: &[&crate::mcp::model::McpServer]) -> anyhow::Result<Vec<McpFilePlan>> {
        Ok(Vec::new())
    }

    // ---- 凭证体检 ----

    /// 收集明文凭证。只读，密钥只在内存里用于算指纹
    fn credential_entries(&self) -> Vec<RawEntry> {
        Vec::new()
    }

    /// 读不到明文时的原因。Some 表示不纳入体检，界面上列为「不支持」
    fn credential_unsupported(&self) -> Option<&'static str> {
        None
    }
}

/// 返回全部 agent，按 [`Agent::order`] 排序。
///
/// **新增一个 agent 只需要在这里加一行。** 顺序由各实现自己的
/// `order()` 决定，本机在用的排前面。
pub fn all() -> Vec<Box<dyn Agent>> {
    let mut list: Vec<Box<dyn Agent>> = vec![
        Box::new(claude::Claude),
        Box::new(codex::Codex),
        Box::new(zcode::ZCode),
        Box::new(pi::Pi),
        Box::new(opencode::OpenCode),
        Box::new(workbuddy::WorkBuddy),
        Box::new(dsh::Dsh),
        Box::new(cursor::Cursor),
        Box::new(qoder::Qoder),
        Box::new(qoder_cn::QoderCn),
        Box::new(codebuddy::CodeBuddy),
        Box::new(trae_cn::TraeCn),
        Box::new(hermes::Hermes),
        Box::new(zed::Zed),
        Box::new(gemini::Gemini),
    ];
    list.sort_by_key(|a| a.order());
    list
}

/// 按 id 找一个 agent。
pub fn find(id: &str) -> Option<Box<dyn Agent>> {
    all().into_iter().find(|a| a.id() == id)
}

/// 全部已知的 agent id。用于过滤母版里的无效分配值。
pub fn ids() -> Vec<&'static str> {
    all().iter().map(|a| a.id()).collect()
}

/// 只返回支持 provider 注入的 agent。界面矩阵的列就是它。
pub fn provider_capable() -> Vec<Box<dyn Agent>> {
    all().into_iter().filter(|a| a.supports_provider()).collect()
}

/// 只返回支持 MCP 注入的 agent。
pub fn mcp_capable() -> Vec<Box<dyn Agent>> {
    all().into_iter().filter(|a| a.supports_mcp()).collect()
}
