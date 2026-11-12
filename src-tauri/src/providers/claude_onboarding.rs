//! Claude Code 的首次启动引导（onboarding）开关。
//!
//! ## 它是什么
//!
//! Claude Code 第一次启动会走一遍引导：选主题、登录 Anthropic 账号。
//! 用第三方中转时那一步登录是多余的 —— key 已经写在
//! `ANTHROPIC_AUTH_TOKEN` 里了。
//!
//! 跳过的方式是在 `~/.claude.json` 的**根对象**写
//! `hasCompletedOnboarding: true`。关掉就是把那个字段删掉，下次启动
//! 重新出现引导。
//!
//! ## 为什么写 `~/.claude.json` 而不是 `settings.json`
//!
//! 两个文件职责不同：
//!
//! | 文件 | 装什么 |
//! | --- | --- |
//! | `~/.claude/settings.json` | provider 配置（`env`）、权限、hooks |
//! | `~/.claude.json` | 客户端状态：登录态、引导进度、MCP server、项目历史 |
//!
//! `hasCompletedOnboarding` 属于后者。写错文件不会报错，只是没效果。
//!
//! ## 为什么它不挂在 provider 上
//!
//! 这个标记描述的是「这台机器上的 Claude Code 是否已完成引导」，
//! 和当前用哪家 provider 无关。切换 provider 不应该重置它，所以它不进
//! `Provider` 结构，也不参与 `providers sync`。
//!
//! ## 只动这一个键
//!
//! `~/.claude.json` 里有几十个键（登录态、项目历史、插件记录），
//! 一个都不能碰。所以这里做的是**单键增删**，不是整份重写。
//! 这个原则与 cc-switch 的实现一致。

use crate::config::contract_tilde;
use serde_json::Value;
use std::path::PathBuf;

/// `~/.claude.json` 的路径。
pub fn config_path() -> PathBuf {
    super::store::home_dir().join(".claude.json")
}

/// 读当前状态。
///
/// 返回 `None` 表示 `~/.claude.json` 不存在（Claude Code 还没跑过），
/// 或者文件里没有这个键 —— 两种情况对用户都意味着「引导会出现」。
pub fn is_skipped() -> anyhow::Result<bool> {
    let path = config_path();
    if !path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&path)?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("{} 解析失败（{}）", path.display(), e))?;
    Ok(json
        .get("hasCompletedOnboarding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

/// 设置或清除这个标记。返回 `true` 表示文件真的变了。
///
/// `skip = true` 写 `hasCompletedOnboarding: true`；
/// `skip = false` 删掉这个键（不是写 false —— Claude Code 只认 true）。
pub fn set_skipped(skip: bool) -> anyhow::Result<(bool, String)> {
    let path = config_path();

    // 文件不存在时要分两种：
    // - 要开启：可以新建（只有这一个键，Claude Code 会自己补其余）
    // - 要关闭：本来就没有，直接返回"无需改动"
    if !path.exists() {
        if !skip {
            return Ok((false, "~/.claude.json 不存在，引导本来就会出现".to_string()));
        }
        let mut root = serde_json::Map::new();
        root.insert("hasCompletedOnboarding".into(), Value::Bool(true));
        write_json(&path, &Value::Object(root))?;
        return Ok((
            true,
            "已新建 ~/.claude.json 并写入 hasCompletedOnboarding = true".to_string(),
        ));
    }

    let text = std::fs::read_to_string(&path)?;
    let mut root: Value = serde_json::from_str(&text).map_err(|e| {
        anyhow::anyhow!(
            "{} 解析失败（{}），已中止，不会写入",
            path.display(),
            e
        )
    })?;

    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} 顶层不是对象", path.display()))?;

    if skip {
        let already = obj
            .get("hasCompletedOnboarding")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if already {
            return Ok((false, "已经跳过首次引导，无需改动".to_string()));
        }
        obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
        write_json(&path, &root)?;
        Ok((
            true,
            "已写入 hasCompletedOnboarding = true，下次启动跳过引导".to_string(),
        ))
    } else {
        if obj.remove("hasCompletedOnboarding").is_none() {
            return Ok((false, "本来就没有跳过标记，无需改动".to_string()));
        }
        write_json(&path, &root)?;
        Ok((
            true,
            "已删除 hasCompletedOnboarding，下次启动会重新出现引导".to_string(),
        ))
    }
}

/// 写回 `~/.claude.json`。
///
/// 这个文件里有登录态和项目历史，属于敏感文件：
/// Unix 上收紧到 0600；Windows 靠目录权限。
fn write_json(path: &PathBuf, value: &Value) -> anyhow::Result<()> {
    use std::io::Write;

    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');

    let mut f = std::fs::File::create(path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = f.metadata()?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}

/// 给界面展示的当前状态。
///
/// `available` 为 false 表示 Claude Code 没装（`~/.claude.json` 和
/// `~/.claude` 都不存在），界面上这一项应该灰掉。
pub fn status() -> anyhow::Result<crate::types::ClaudeOnboardingStatus> {
    let file = config_path();
    let dir = super::store::home_dir().join(".claude");
    Ok(crate::types::ClaudeOnboardingStatus {
        path: contract_tilde(&file),
        available: file.exists() || dir.exists(),
        skipped: is_skipped()?,
    })
}
