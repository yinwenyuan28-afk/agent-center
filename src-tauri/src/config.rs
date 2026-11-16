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
    /// 用户主动取消了这个映射。
    ///
    /// 取消映射会把目标位置的链接或副本删掉。如果只删不记，下一次
    /// 「同步全部」会立刻把它建回来，用户会以为按钮没生效。所以
    /// 取消动作同时写这个标记，同步时跳过。
    #[serde(default)]
    pub disabled: bool,
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

/// `[usage]` 段：token 用量统计的开关。
///
/// 默认**空名单 = 一个都不统计**。这些 agent 的用量库加起来上 GB，
/// 读不读、读谁，由用户显式勾选决定，工具不替用户做主。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageConfig {
    /// 纳入统计的 agent id 列表
    #[serde(default)]
    pub enabled: Vec<String>,
}

impl UsageConfig {
    /// 某个 agent 是否已启用统计。
    pub fn is_enabled(&self, agent: &str) -> bool {
        self.enabled.iter().any(|a| a == agent)
    }

    /// 打开或关闭某个 agent 的统计，返回操作后的状态。
    ///
    /// 幂等：重复打开不会往列表里塞重复项，重复关闭也不会报错。
    pub fn set_enabled(&mut self, agent: &str, on: bool) -> bool {
        let present = self.is_enabled(agent);
        if on && !present {
            self.enabled.push(agent.to_string());
            // 排序让写回配置的 diff 稳定，不受点击顺序影响
            self.enabled.sort();
            self.enabled.dedup();
        } else if !on && present {
            self.enabled.retain(|a| a != agent);
        }
        self.is_enabled(agent)
    }
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
    /// token 用量统计的启用名单
    #[serde(default)]
    pub usage: UsageConfig,
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

            let (target, mode, disabled) = match override_cfg {
                Some(m) => (expand_tilde(&m.to), m.mode, m.disabled),
                None => (default_target, LinkMode::Auto, false),
            };

            out.push(ResolvedMapping {
                agent: agent.id().to_string(),
                label: agent.label().to_string(),
                source: self.source_path(),
                target,
                mode,
                disabled,
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
                    disabled: m.disabled,
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
    /// 用户是否取消了这个映射。为 true 时同步跳过，界面上显示「已取消」
    pub disabled: bool,
}

impl Config {
    /// 设置某个 agent 的映射是否被取消，返回实际写入后的值。
    ///
    /// 用户对内置 agent 只写「覆盖项」，所以这里的第一种情况是：
    /// 配置里已有该 agent 的条目 → 直接改 `disabled`。
    /// 第二种情况是配置文件里没有这个 agent（用的是内置默认路径）→
    /// 补一条完整条目，把默认目标路径和当前模式写进去，避免
    /// 因为取消一个映射而把用户的默认路径语义改掉。
    ///
    /// 取消后再恢复时，若这条 override 的路径和内置默认完全一致，
    /// 就整条删掉，让配置文件回到「没写过这个 agent」的干净状态。
    pub fn set_mapping_disabled(&mut self, agent: &str, disabled: bool) -> anyhow::Result<()> {
        let default_target = crate::agents::find(agent).and_then(|a| a.instruction_file());

        if let Some(idx) = self.mappings.iter().position(|m| m.agent == agent) {
            // 恢复时，如果这条覆盖项本来就和内置默认一模一样
            // （很可能是上次取消时补上的），整条删掉，
            // 让配置文件回到「没写过这个 agent」的干净状态。
            let removable = !disabled
                && self.mappings[idx].mode == LinkMode::Auto
                && default_target
                    .as_ref()
                    .map(|d| expand_tilde(&self.mappings[idx].to) == *d)
                    .unwrap_or(false);
            if removable {
                self.mappings.remove(idx);
            } else {
                self.mappings[idx].disabled = disabled;
            }
            return Ok(());
        }

        // 配置里还没有这个 agent 的条目
        if !disabled {
            // 恢复一个本来就没写过的映射：什么都不用做
            return Ok(());
        }

        // 取消：需要落一条覆盖项，带上当前生效的目标路径
        let default_target = default_target
            .ok_or_else(|| anyhow::anyhow!("找不到 agent 的指令文件路径：{agent}"))?;

        self.mappings.push(MappingConfig {
            agent: agent.to_string(),
            to: contract_tilde(&default_target),
            mode: LinkMode::Auto,
            disabled: true,
        });
        Ok(())
    }
}
