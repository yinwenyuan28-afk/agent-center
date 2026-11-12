//! Grok Build CLI。
//!
//! ## provider：单槽位
//!
//! `~/.grok/config.toml` 里 `models.default` 指向唯一一张生效的
//! `[model."..."]` 表。同一时刻只有一家。
//!
//! **本机未安装**，实现是按 cc-switch 的 `grok_config.rs` 移植的。

use super::Agent;
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Grok;

impl Grok {
    fn config_path() -> PathBuf {
        home_dir().join(".grok").join("config.toml")
    }
}

impl Agent for Grok {
    fn id(&self) -> &'static str {
        "grok"
    }

    fn label(&self) -> &'static str {
        "Grok Build"
    }

    fn order(&self) -> u32 {
        190
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".grok")
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".grok").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::config_path().exists()
    }

    fn single_slot(&self) -> bool {
        true
    }

    /// Grok 的 `api_backend` 已知两种：`responses` / `chat_completions`。
    fn supported_kinds(&self) -> &'static [crate::providers::model::ProviderKind] {
        use crate::providers::model::ProviderKind::{OpenaiChat, OpenaiResponses};
        &[OpenaiChat, OpenaiResponses]
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_grok())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::grok::GrokInjector.plan(providers)
    }
}
