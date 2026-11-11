//! `~/.agents/agentcenter.toml` 的读写。
//!
//! 设计取舍：配置文件只写"覆盖项"。内置注册表提供全部默认值，
//! 用户想改哪个 agent 的路径才写哪一条。这样新增 agent 时不需要用户改配置，
//! 也不会出现"配置文件里少一条就功能缺失"的情况。

use crate::types::LinkMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 单条映射的用户配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingConfig {
    /// 对应哪个 agent
    pub agent: String,
    /// 目标文件路径。支持 `~` 前缀
    pub to: String,
    /// 链接方式，默认 auto
    #[serde(default = "default_mode")]
    pub mode: LinkMode,
}

fn default_mode() -> LinkMode {
    LinkMode::Auto
}

/// `[source]` 段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    /// 母版路径。默认 `~/.agents/AGENTS.md`
    #[serde(default = "default_source")]
    pub instructions: String,
}

fn default_source() -> String {
    "~/.agents/AGENTS.md".to_string()
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            instructions: default_source(),
        }
    }
}

/// `[skills.tags]` 段。key 是 skill 名，value 是标签列表。
///
/// 标签存在这里而不是写进 skill 目录，是为了让 `npx skills update`
/// 不会因为目录内多出一个文件而报冲突。
pub type SkillTags = BTreeMap<String, Vec<String>>;

/// `[skills]` 段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub tags: SkillTags,
}

/// 完整的配置文件结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub source: SourceConfig,
    /// 覆盖内置映射的条目。没写到的 agent 用注册表默认值。
    #[serde(default)]
    pub mappings: Vec<MappingConfig>,
    #[serde(default)]
    pub skills: SkillsConfig,
}

/// 把配置里的 `~` 展开成真实家目录。
///
/// 只处理开头的 `~`，不处理 `~user` 形式 —— 跨平台的后者语义不一致，
/// 与其猜错不如不做。
pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/").or_else(|| input.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if input == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(input)
}

/// 把绝对路径压回 `~` 形式，用于界面展示和写回配置。
pub fn contract_tilde(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            let s = rest.to_string_lossy().replace('\\', "/");
            return format!("~/{s}");
        }
    }
    path.to_string_lossy().to_string()
}

/// 配置文件的位置：`~/.agents/agentcenter.toml`。
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agents")
        .join("agentcenter.toml")
}

impl Config {
    /// 从磁盘读取。文件不存在时返回全默认值，不报错 ——
    /// 首次运行就是这种情况，不该被当成错误。
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }

    /// 写回磁盘。父目录不存在时自动创建。
    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// 母版文件的绝对路径。
    pub fn source_path(&self) -> PathBuf {
        expand_tilde(&self.source.instructions)
    }

    /// 解析出最终的映射列表：内置默认值 + 用户覆盖。
    ///
    /// 用户配置里写了的 agent 用用户的路径，没写的用 agent 实现的默认路径。
    /// 没有单一指令文件的 agent（如 workbuddy）不会出现在结果里。
    pub fn resolved_mappings(&self) -> Vec<ResolvedMapping> {
        let mut out = Vec::new();

        for agent in crate::agents::all() {
            let Some(default_target) = agent.instruction_file() else {
                continue;
            };

            // 找用户是否覆盖了这个 agent
            let override_cfg = self.mappings.iter().find(|m| m.agent == agent.id());

            let (target, mode) = match override_cfg {
                Some(m) => (expand_tilde(&m.to), m.mode),
                None => (default_target, LinkMode::Auto),
            };

            out.push(ResolvedMapping {
                agent: agent.id().to_string(),
                label: agent.label().to_string(),
                source: self.source_path(),
                target,
                mode,
                config_dir: agent.config_dir(),
            });
        }

        // 用户在配置里写了已知 agent 之外的条目，也一并支持。
        // 这样用户能接入任何新工具，不必等工具更新版本。
        for m in &self.mappings {
            if crate::agents::find(&m.agent).is_none() {
                out.push(ResolvedMapping {
                    agent: m.agent.clone(),
                    label: m.agent.clone(),
                    source: self.source_path(),
                    target: expand_tilde(&m.to),
                    mode: m.mode,
                    // 自定义 agent 的"是否安装"判断用目标文件所在目录
                    config_dir: expand_tilde(&m.to)
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from(".")),
                });
            }
        }

        out
    }
}

/// 解析完成的一条映射，已把 `~` 展开成真实路径。
#[derive(Debug, Clone)]
pub struct ResolvedMapping {
    pub agent: String,
    pub label: String,
    pub source: PathBuf,
    pub target: PathBuf,
    pub mode: LinkMode,
    /// 用于判断 agent 是否安装的目录
    pub config_dir: PathBuf,
}
