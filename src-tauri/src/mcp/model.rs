//! MCP server 统一管理的数据模型。
//!
//! 一套模型覆盖三家差异巨大的配置格式：
//!
//! - `stdio`：本地进程，有 `command` / `args` / `env` / `cwd`
//! - `sse`：Server-Sent Events 远程端点，有 `url`
//! - `http`：Streamable HTTP 远程端点，有 `url` / `headers`
//!
//! 各 agent 的字段名和语义差异见 `plan/mcp-management.md` 的映射表。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 传输协议。三种的字段集完全不同，所以用枚举而不是合并成一个结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// 本地子进程，走 stdin/stdout 通信
    #[default]
    Stdio,
    /// Server-Sent Events 远程端点（较旧）
    Sse,
    /// Streamable HTTP 远程端点（较新，取代 sse）
    Http,
}

impl Transport {
    /// 解析字符串。宽容处理各家的写法差异。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "sse" => Self::Sse,
            // WorkBuddy 用 streamableHttp / streamable-http 两种写法
            "http" | "streamable-http" | "streamablehttp" => Self::Http,
            _ => Self::Stdio,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Sse => "sse",
            Self::Http => "http",
        }
    }
}

/// 母版里的一条 MCP server 定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    /// 稳定标识，用作各 agent 配置里的键
    pub id: String,
    /// 展示名。留空时用 id
    #[serde(default)]
    pub name: String,
    /// 传输协议
    #[serde(default)]
    pub transport: Transport,

    // ---- stdio 专用 ----
    /// 可执行文件。stdio 必填
    #[serde(default)]
    pub command: String,
    /// 命令行参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 工作目录。csv 里很多 server 依赖它
    #[serde(default)]
    pub cwd: String,

    // ---- sse / http 专用 ----
    /// 远程端点地址
    #[serde(default)]
    pub url: String,
    /// 静态请求头（如 Authorization）
    #[serde(default)]
    pub headers: BTreeMap<String, String>,

    // ---- 通用 ----
    /// 环境变量。stdio 传给子进程，远程型可能被忽略
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 是否启用。默认启用
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 要注入到哪些 agent
    #[serde(default)]
    pub agents: Vec<String>,
    /// 备注
    #[serde(default)]
    pub note: String,
}

fn default_true() -> bool {
    true
}

/// 母版文件结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpFile {
    #[serde(default, rename = "server")]
    pub servers: Vec<McpServer>,
}

/// 一条差异。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiffLine {
    pub agent: String,
    pub path: String,
    /// add / update / unchanged / conflict
    pub action: String,
    pub target: String,
    pub detail: String,
}

/// 差异报告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiffReport {
    pub lines: Vec<McpDiffLine>,
    pub agent_count: usize,
    pub changed_files: usize,
}

/// 同步结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSyncReport {
    pub ok: bool,
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub backups: Vec<String>,
    pub log: Vec<String>,
}

impl McpServer {
    /// 展示名：name 为空时回退到 id。
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    /// 是否远程型（sse / http）。
    pub fn is_remote(&self) -> bool {
        !matches!(self.transport, Transport::Stdio)
    }

    /// 基本校验。返回错误说明，通过时返回 None。
    pub fn validate(&self) -> Option<String> {
        if self.id.trim().is_empty() {
            return Some("id 不能为空".to_string());
        }
        if self.is_remote() {
            if self.url.trim().is_empty() {
                return Some(format!("{}: 远程型必须填 url", self.id));
            }
        } else if self.command.trim().is_empty() {
            return Some(format!("{}: stdio 型必须填 command", self.id));
        }
        None
    }
}
