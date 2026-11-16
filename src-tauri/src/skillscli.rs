//! `npx skills` CLI 的封装。
//!
//! 三条来自实测的硬约束，直接决定了这里的实现方式：
//!
//! 1. **不能写死 CLI 路径**。本机没有全局安装，只有
//!    `_npx/<hash>/node_modules/skills/dist/cli.mjs` 这份缓存副本，
//!    hash 目录名会随缓存清理变化。所以优先找全局 `skills`，
//!    找不到就用 `npx skills` 现场解析。
//!
//! 2. **"版本"是内容哈希不是版本号**。`.skill-lock.json` 里 GitHub 源
//!    记的是 git tree SHA，其它源是 SHA-256。界面展示指纹，不假装有语义版本。
//!
//! 3. **`check`/`update` 没有 JSON 输出**。`check` 有更新时退出码为 1，
//!    只能靠退出码判断；`update` 输出彩色英文文本，原样透传到日志区。

use crate::skills::skill_lock_path;
use crate::types::{ManagedSkill, RunResult};
use std::process::Command;

/// 构造一个调用 `skills` 的命令。
///
/// 优先用 PATH 里的全局 `skills`；没有则退回 `npx skills`。
/// 在 Windows 上 `npx` 是 `.cmd`，必须走 `cmd /C` 才能找到，
/// 所以这里区分平台。
///
/// 一律用 [`crate::proc::new`] 构造：主程序没有控制台，直接起这些
/// console 子系统的命令会让系统给每个新进程弹黑窗口。
fn skills_command(args: &[&str]) -> Command {
    let has_global = which_skills();

    #[cfg(windows)]
    {
        let mut cmd = crate::proc::new("cmd");
        cmd.arg("/C");
        if has_global {
            cmd.arg("skills");
        } else {
            cmd.arg("npx").arg("--yes").arg("skills");
        }
        for a in args {
            cmd.arg(a);
        }
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = if has_global {
            crate::proc::new("skills")
        } else {
            let mut c = crate::proc::new("npx");
            c.arg("--yes").arg("skills");
            c
        };
        for a in args {
            cmd.arg(a);
        }
        cmd
    }
}

/// 检查 PATH 里是否有全局 `skills` 命令。
fn which_skills() -> bool {
    #[cfg(windows)]
    let probe = crate::proc::output("cmd", &["/C", "where skills"], None);
    #[cfg(not(windows))]
    let probe = crate::proc::output("sh", &["-c", "command -v skills"], None);

    probe.map(|o| o.status.success()).unwrap_or(false)
}

/// 清理终端控制序列，把带颜色的原始输出变成可读的纯文本。
///
/// `skills` CLI 用 ANSI 转义码上色，还会用 `\r` + 光标控制画加载动画。
/// 直接透传到界面就是一片 `[38;5;145m` 这种乱码，所以必须处理：
///
/// 1. 去掉 CSI 序列（`ESC [ ... 终止字节`），覆盖颜色、光标移动、清屏
/// 2. 去掉 OSC 序列（`ESC ] ... BEL/ST`），覆盖标题设置之类
/// 3. `\r` 退回行首重绘的动画帧只保留最后一次，否则会出现几百行重复
/// 4. 连续重复行去重，进一步压缩加载动画的残渣
fn strip_ansi(input: &str) -> String {
    // 第一遍：剥掉转义序列，保留 \r 供下一步处理
    let mut cleaned = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\x1b' {
            cleaned.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                // CSI：吃掉 '[' 之后直到 0x40..=0x7E 的终止字节
                chars.next();
                for c2 in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c2) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC：吃掉直到 BEL 或 ESC \
                chars.next();
                while let Some(c2) = chars.next() {
                    if c2 == '\x07' {
                        break;
                    }
                    if c2 == '\x1b' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // 单个 ESC + 一个字符的序列，整体丢掉
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    // 第二遍：逐行处理。同一行里出现多次 \r 说明是动画重绘，
    // 只保留最后一个 \r 之后的内容。
    let mut out = String::with_capacity(cleaned.len());
    for line in cleaned.split('\n') {
        let visible = match line.rfind('\r') {
            Some(idx) => &line[idx + 1..],
            None => line,
        };
        let trimmed = visible.trim_end();
        // 加载动画会留下大量空行，跳过
        if trimmed.is_empty() && !out.is_empty() {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
    }

    // 第三遍：连续重复的行只留一份
    let mut result = String::with_capacity(out.len());
    let mut prev: Option<&str> = None;
    for line in out.lines() {
        if Some(line) == prev {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev = Some(line);
    }

    result.trim().to_string()
}

/// 把命令输出整理成 [`RunResult`]。
///
/// 同时保留 stdout 和 stderr，因为 `skills` 的部分信息只出现在 stderr。
/// 输出里的 ANSI 控制序列会在这里被剥掉，界面拿到的就是纯文本。
fn to_result(out: std::process::Output) -> RunResult {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    let mut merged = String::new();
    let s = strip_ansi(&stdout);
    if !s.is_empty() {
        merged.push_str(&s);
    }
    let e = strip_ansi(&stderr);
    if !e.is_empty() {
        if !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(&e);
    }

    let code = out.status.code().unwrap_or(-1);
    RunResult {
        ok: out.status.success(),
        exit_code: code,
        output: merged,
    }
}

/// 检查更新。
///
/// **退出码 1 表示检测到有更新，不是执行失败**。调用方必须区分这两种情况，
/// 所以这里把 exit_code 原样传出去，让上层判断。
pub fn check_updates() -> RunResult {
    let out = skills_command(&["check", "-g"]).output();
    match out {
        Ok(o) => to_result(o),
        Err(e) => RunResult::failure(-1, format!("无法执行 skills 命令：{e}")),
    }
}

/// 更新指定的 skill；传空数组表示全部。
pub fn update(names: &[String]) -> RunResult {
    let mut args: Vec<&str> = vec!["update", "-g", "-y"];
    for n in names {
        args.push(n);
    }
    let out = skills_command(&args).output();
    match out {
        Ok(o) => to_result(o),
        Err(e) => RunResult::failure(-1, format!("无法执行 skills 命令：{e}")),
    }
}

/// 读取 lock 文件，组装成受管 skill 清单。
///
/// 直接读 JSON 而不是调 `skills list --json`，原因有两条：
/// 一是 `list` 的 `agents` 字段给的是显示名不是路径，信息量更低；
/// 二是读文件不受 CLI 是否存在的影响，界面不会因为没装 npx 就空白。
pub fn list_managed() -> anyhow::Result<Vec<ManagedSkill>> {
    let path = skill_lock_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&text)?;

    let Some(skills) = json.get("skills").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for (name, entry) in skills {
        let get_str = |k: &str| {
            entry
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };

        out.push(ManagedSkill {
            name: name.clone(),
            source: get_str("source"),
            source_type: get_str("sourceType"),
            r#ref: entry
                .get("ref")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            hash_short: get_str("skillFolderHash").chars().take(8).collect(),
            installed_at: get_str("installedAt"),
            updated_at: get_str("updatedAt"),
            // 检查过才有结论，初始为未检查
            has_update: None,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
