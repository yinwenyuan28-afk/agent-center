//! DSH（DeepSeek Harness）。
//!
//! 两个能力：指令文件、provider。MCP 段在 `cordis.patch.yml` 里没有，
//! 机制未探明，所以不声明 mcp 能力。
//!
//! 具体注入逻辑在 [`crate::providers::dsh`]。
//!
//! 注意：`cordis.patch.yml` 的实际结构是 patch-entry 数组，
//! `providers:` 段缩进 4 而不是 0。这个问题记录在
//! `plan/providers-management.md` 的踩坑清单里，未在本轮修复。

use super::Agent;
use crate::providers::model::{FilePlan, Provider};
use crate::providers::scan::AgentScan;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Dsh;

impl Dsh {
    fn patch_path() -> PathBuf {
        home_dir()
            .join(".dsh")
            .join("profiles")
            .join("desktop")
            .join("cordis.patch.yml")
    }
}

impl Agent for Dsh {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn label(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn order(&self) -> u32 {
        70
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".dsh")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".dsh").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".dsh").join("skills")
    }

    fn supports_provider(&self) -> bool {
        true
    }

    fn provider_ready(&self) -> bool {
        Self::patch_path().exists()
            || home_dir().join(".dsh").join(".credentials.yaml").exists()
    }

    fn provider_targets(&self) -> Vec<PathBuf> {
        vec![
            Self::patch_path(),
            home_dir().join(".dsh").join(".credentials.yaml"),
        ]
    }

    fn provider_scan(&self) -> Option<AgentScan> {
        Some(crate::providers::scan::scan_dsh())
    }

    fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        use crate::providers::Injector;
        crate::providers::dsh::DshInjector.plan(providers)
    }
}
