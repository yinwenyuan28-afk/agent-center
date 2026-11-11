//! Trae CN。指令文件和 skills，provider 未验证。

use super::Agent;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct TraeCn;

impl Agent for TraeCn {
    fn id(&self) -> &'static str {
        "trae-cn"
    }

    fn label(&self) -> &'static str {
        "Trae CN"
    }

    fn order(&self) -> u32 {
        150
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".trae-cn")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".trae-cn").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".trae-cn").join("skills")
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
