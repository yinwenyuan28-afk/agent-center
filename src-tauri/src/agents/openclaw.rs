//! OpenClaw。
//!
//! ## provider：多 provider 并存
//!
//! `~/.openclaw/openclaw.json` 的 `models.providers.<id>` 下可以同时
//! 存在多家。真正的"当前模型"在 `agents.defaults.model.primary`，
//! 归用户和 OpenClaw 管，本工具不碰。
//!
//! **本机未安装**，实现是按 cc-switch 的 `openclaw_config.rs` 移植的。

use super::Agent;
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct OpenClaw;

impl OpenClaw {
    fn config_path() -> PathBuf {
        home_dir().join(".openclaw").join("openclaw.json")
    }
}

impl Agent for OpenClaw {
    fn id(&self) -> &'static str {
        "openclaw"
    }

    fn label(&self) -> &'static str {
        "OpenClaw"
    }

    fn order(&self) -> u32 {
        200
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".openclaw")
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".openclaw").join("skills")
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
        Some(crate::providers::scan::scan_openclaw())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::openclaw::OpenClawInjector.plan(providers)
    }
}
