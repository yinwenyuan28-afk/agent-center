//! Qoder CN。只有指令文件和 skills，其余能力与 Qoder 相同但未验证。

use super::Agent;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct QoderCn;

impl Agent for QoderCn {
    fn id(&self) -> &'static str {
        "qoder-cn"
    }

    fn label(&self) -> &'static str {
        "Qoder CN"
    }

    fn order(&self) -> u32 {
        130
    }

    fn config_dir(&self) -> PathBuf {
        home_dir().join(".qoder-cn")
    }

    fn instruction_file(&self) -> Option<PathBuf> {
        Some(home_dir().join(".qoder-cn").join("AGENTS.md"))
    }

    fn skills_dir(&self) -> PathBuf {
        home_dir().join(".qoder-cn").join("skills")
    }

    fn credential_unsupported(&self) -> Option<&'static str> {
        Some("使用浏览器 OAuth 登录，本地无明文 key")
    }
}
