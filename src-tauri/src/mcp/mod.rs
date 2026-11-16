//! MCP server 统一管理。
//!
//! 模块划分与 `providers` 对称：
//!
//! - [`model`] —— 数据结构
//! - [`store`] —— 母版 `~/.agents/mcp.toml` 读写
//! - [`scan`] —— 从各 agent 现有配置读 server（只读）
//! - [`diff`] —— 母版 vs 现状对比
//! - [`inject`] —— 每个工具一个文件，共 18 个注入器
//! - [`matrix`] —— server 池 × agent 的分配矩阵

pub mod diff;
pub mod inject;
pub mod matrix;
pub mod model;
pub mod scan;
pub mod store;

pub use diff::run as diff_run;
pub use inject::sync as sync_run;
