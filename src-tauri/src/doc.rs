//! 母版 AGENTS.md 的读取、创建与写入。
//!
//! 单独成一个模块，因为它是整个工具唯一一份"内容本体"：
//! 链接引擎只负责把它映射出去，这里负责它的内容本身。
//!
//! 三条安全约定：
//! 1. 读取时文件不存在不算错误，返回 exists=false 让界面引导创建
//! 2. `create` 只在文件不存在时写，避免覆盖已有内容
//! 3. `write` 直接覆盖，但调用方（界面）必须先让用户看到原内容

use crate::config::{contract_tilde, Config};
use crate::types::RunResult;
use serde::Serialize;

/// 母版文件的读取结果。
#[derive(Debug, Clone, Serialize)]
pub struct DocContent {
    /// 文件是否存在
    pub exists: bool,
    /// 文件内容。不存在时为空串
    pub content: String,
}

/// 母版文件的实际路径，从配置里取（默认 `~/.agents/AGENTS.md`）。
pub fn doc_path() -> std::path::PathBuf {
    Config::load()
        .map(|c| c.source_path())
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".agents")
                .join("AGENTS.md")
        })
}

/// 读取母版内容。
pub fn read() -> anyhow::Result<DocContent> {
    let path = doc_path();
    if !path.exists() {
        return Ok(DocContent {
            exists: false,
            content: String::new(),
        });
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(DocContent {
        exists: true,
        content,
    })
}

/// 覆盖写入母版。父目录不存在时自动创建。
pub fn write(content: &str) -> RunResult {
    let path = doc_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return RunResult::failure(1, format!("创建目录失败：{e}"));
        }
    }
    match std::fs::write(&path, content) {
        Ok(()) => RunResult::success(format!(
            "已保存 {}（{} 字节）",
            contract_tilde(&path),
            content.len()
        )),
        Err(e) => RunResult::failure(1, format!("写入失败：{e}")),
    }
}

/// 创建母版文件。已存在时拒绝，把判断留给用户而不是直接覆盖。
pub fn create(content: &str) -> RunResult {
    let path = doc_path();
    if path.exists() {
        return RunResult::failure(
            1,
            format!(
                "{} 已存在，未覆盖。要修改请用编辑功能",
                contract_tilde(&path)
            ),
        );
    }
    write(content)
}
