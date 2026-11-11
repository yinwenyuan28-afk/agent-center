//! Gemini CLI。
//!
//! 本机未安装，各项能力都未实测。这里只登记它的身份，
//! 把原先游离在 `credentials.rs::unsupported_agents()` 里的那条
//! 「gemini 用浏览器 OAuth」收进来，让 agent 清单只剩一处。
//!
//! 等装了之后按 opencode 的模式补 provider（`~/.gemini/settings.json`）。

use super::Agent;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Gemini;

impl Agent for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn label(&self) -> &'static str {
        "Gemini CLI"
    }

    fn order(&self) -> u32 {
        180
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".gemini")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".gemini").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".gemini").join("skills")
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
