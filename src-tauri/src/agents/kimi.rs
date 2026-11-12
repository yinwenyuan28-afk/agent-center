//! Kimi Code。
//!
//! ## provider：多 provider 并存 + 原生"当前模型"
//!
//! `~/.kimi-code/config.toml` 里 `[providers.<id>]` 可以有多家；
//! `default_model` 指向一个模型别名，别名再经 `[models.<alias>].provider`
//! 反查出供应商。本工具写 provider 和模型别名，但不主动改
//! `default_model`（用户的运行时选择），只在它指向被删条目时纠正。
//!
//! **本机未安装**，实现是按 cc-switch 的 `kimi_config.rs` 移植的。

use super::Agent;
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use std::path::PathBuf;

pub struct Kimi;

impl Kimi {
    fn dir() -> PathBuf {
        if let Ok(v) = std::env::var("KIMI_CODE_HOME") {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
        crate::providers::store::home_dir().join(".kimi-code")
    }

    fn config_path() -> PathBuf {
        Self::dir().join("config.toml")
    }
}

impl Agent for Kimi {
    fn id(&self) -> &'static str {
        "kimi"
    }

    fn label(&self) -> &'static str {
        "Kimi Code"
    }

    fn order(&self) -> u32 {
        210
    }

    fn config_dir(&self) -> PathBuf {
        Self::dir()
    }

    fn skills_dir(&self) -> PathBuf {
        Self::dir().join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::config_path().exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_kimi())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::kimi::KimiInjector.plan(providers)
    }
}
