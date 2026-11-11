//! CodeBuddy。只有 skills，其余能力未验证。

use super::Agent;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct CodeBuddy;

impl Agent for CodeBuddy {
    fn id(&self) -> &'static str {
        "codebuddy"
    }

    fn label(&self) -> &'static str {
        "CodeBuddy"
    }

    fn order(&self) -> u32 {
        140
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".codebuddy")
    }

    // instruction_file 用默认 None：没有单一指令文件

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".codebuddy").join("skills")
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
