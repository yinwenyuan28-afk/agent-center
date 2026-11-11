//! 母版 `~/.agents/providers.toml` 的读写。
//!
//! 母版是所有 provider 的唯一真相来源。这个模块只负责它的解析与保存，
//! 不涉及任何 agent 配置的改写。
//!
//! 安全约定：
//! - 读不到文件返回空列表，不报错（首次使用就是这种情况）
//! - 保存时如果父目录不存在会自动创建
//! - 保存后检查 `.gitignore` 里有没有 `providers.toml`，没有就警告
//!   （文件里有明文 key，误提交到 git 就是泄露）

use super::model::{Provider, ProviderFile};
use std::path::PathBuf;

/// 母版文件路径：`~/.agents/providers.toml`。
///
/// **测试专用覆盖**：设置环境变量 `AGENTCENTER_HOME` 可以让整个工具
/// 把另一个目录当家目录。这是唯一能在不碰真实配置的前提下做端到端
/// 测试的办法 —— Windows 上 `dirs::home_dir()` 走的是
/// `SHGetKnownFolderPath`，改 `USERPROFILE` 是无效的。
pub fn home_dir() -> PathBuf {
    if let Ok(v) = std::env::var("AGENTCENTER_HOME") {
        let v = v.trim();
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn path() -> PathBuf {
    home_dir().join(".agents").join("providers.toml")
}

/// 从模块外部直接插入一条记录用的临时入口，
/// 真正的解析逻辑在 [`load_from`] 里。
pub fn load_or_default() -> anyhow::Result<Vec<Provider>> {
    load_from(&path())
}

/// 从指定路径读取母版。供测试和预览用。
pub fn load_from(p: &PathBuf) -> anyhow::Result<Vec<Provider>> {
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(p)?;
    let file: ProviderFile = toml::from_str(&text)?;
    Ok(file.providers)
}

/// 把 provider 列表写回母版。
///
/// 写之前做两道检查：
/// 1. id 不能重复（重复会让注入到各 agent 时互相覆盖）
/// 2. api_key 不能为空
pub fn save(providers: &[Provider]) -> anyhow::Result<()> {
    save_to(&path(), providers)
}

/// 写到指定路径。供测试用。
pub fn save_to(p: &PathBuf, providers: &[Provider]) -> anyhow::Result<()> {
    // 校验：id 唯一
    let mut seen = std::collections::BTreeSet::new();
    for item in providers {
        if item.id.trim().is_empty() {
            anyhow::bail!("provider id 不能为空");
        }
        if !seen.insert(item.id.clone()) {
            anyhow::bail!("provider id 重复：{}", item.id);
        }
        if item.api_key.trim().is_empty() {
            anyhow::bail!("provider {} 的 api_key 不能为空", item.id);
        }
        if item.base_url.trim().is_empty() {
            anyhow::bail!("provider {} 的 base_url 不能为空", item.id);
        }
    }

    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = ProviderFile {
        providers: providers.to_vec(),
    };
    // toml::to_string_pretty 不保留注释。母版头部的说明文字是人写的，
    // 每次保存都丢一次会让人以为文件被覆盖了。所以把原文件第一个
    // `[[provider]]` 之前的内容原样接回去。
    let header = read_header(p);
    let body = toml::to_string_pretty(&file)?;
    let text = format!("{header}{body}");
    // 文件里有明文 key，Unix 上设成只有自己能读。
    // Windows 上这个调用是空操作，靠目录权限和 .gitignore 兜底。
    write_private(p, text.as_bytes())?;
    Ok(())
}

/// 读出母版文件里第一个 `[[provider]]` 之前的内容（含注释和空行）。
///
/// 文件不存在或没有表头时返回空串。只保留表头，因为表后面的注释
/// 位置和具体 provider 绑定，重新序列化后位置会变 —— 与其错位不如不保留。
fn read_header(p: &PathBuf) -> String {
    let Ok(text) = std::fs::read_to_string(p) else {
        return String::new();
    };
    let mut out = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("[[provider]]") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    // 表头之后补一个空行，避免和第一个 [[provider]] 粘在一起
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out
}

/// 写文件，并在 Unix 上把权限收紧到 0600。
fn write_private(p: &PathBuf, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;

    let mut f = std::fs::File::create(p)?;
    f.write_all(bytes)?;
    f.sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = f.metadata()?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(p, perms)?;
    }

    Ok(())
}

/// 检查 `~/.agents/.gitignore` 里有没有忽略 `providers.toml`。
///
/// 返回 None 表示已忽略或文件不存在时无妨，返回 Some(提示) 表示存在泄露风险。
pub fn check_gitignore() -> Option<String> {
    let agents = home_dir().join(".agents");
    let gi = agents.join(".gitignore");

    // 没有 .git 说明不是仓库，不需要管
    if !agents.join(".git").exists() {
        return None;
    }

    let text = std::fs::read_to_string(&gi).unwrap_or_default();
    let ignored = text.lines().any(|l| {
        let t = l.trim();
        t == "providers.toml" || t == "/providers.toml" || t == "*.toml"
    });

    if ignored {
        None
    } else {
        Some(
            "~/.agents/providers.toml 含有明文 API key，但 .gitignore 里没有忽略它。请加一行 `providers.toml` 防止误提交。"
                .to_string(),
        )
    }
}

/// 生成一份示例母版，供用户参考格式。
pub fn example_content() -> String {
    r#"# provider 统一管理母版
#
# 这个文件含有明文 API key，务必加入 ~/.agents/.gitignore，不要提交到 git。
#
# 一个 provider 只保留一把 key。agents 字段声明要注入到哪些 AI 工具。

[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "sk-替换成真实key"
models = ["deepseek/deepseek-v4.1-flash"]
# 要注入到哪些 agent。可用值：claude / zcode / pi / opencode / codex
agents = ["claude", "zcode", "pi"]
note = "2026-11-10 从 commandcode 控制台获取"
"#
    .to_string()
}
