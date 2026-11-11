//! Zed。指令文件和 skills。它按 `%APPDATA%` 存放，不是 `~/.config`。

use super::Agent;
use std::path::PathBuf;

pub struct Zed;

/// Zed 是真正按 `%APPDATA%` 存放的 GUI 应用。
fn config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Zed")
}

impl Agent for Zed {
    fn id(&self) -> &'static str {
        "zed"
    }

    fn label(&self) -> &'static str {
        "Zed"
    }

    fn order(&self) -> u32 {
        170
    }

    fn config_dir(&self) -> PathBuf {
        config_root()
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(config_root().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        config_root().join("skills")
    }
}
