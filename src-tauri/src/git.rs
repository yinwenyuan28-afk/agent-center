//! git 封装。
//!
//! 设计原则：**绝不自动提交，绝不用 `git add -A`**。
//!
//! 调用系统 `git` 而不是引入 Rust 的 git 库，理由：
//! 用户的 git 配置、钩子、凭据助手、SSH 密钥全部照常生效。
//! 用库重新实现一遍这些行为既容易出错，也会让用户困惑
//! "为什么我的 pre-commit 钩子没跑"。
//!
//! 所有 git 调用都走 [`crate::proc`]：主程序是 windows 子系统（无控制台），
//! 直接起 `git.exe` 会让系统给每个子进程新建控制台窗口 —— 读一次状态要跑
//! 四条命令，界面上就会连闪四个黑窗口。

use crate::types::{GitChange, GitStatus, RunResult};
use std::path::{Path, PathBuf};

/// 被管理的仓库：`~/.agents`。
///
/// 走 `store::home_dir()` 以跟随 `AGENTCENTER_HOME`。
pub fn repo_dir() -> PathBuf {
    crate::providers::store::home_dir().join(".agents")
}

/// 在仓库目录里执行一条 git 命令。
fn git(args: &[&str]) -> std::io::Result<std::process::Output> {
    crate::proc::output("git", args, Some(&repo_dir()))
}

/// 执行并返回整理后的结果。
fn git_result(args: &[&str]) -> RunResult {
    match git(args) {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let mut merged = String::new();
            if !stdout.trim().is_empty() {
                merged.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !merged.is_empty() {
                    merged.push('\n');
                }
                merged.push_str(stderr.trim_end());
            }
            RunResult {
                ok: out.status.success(),
                exit_code: out.status.code().unwrap_or(-1),
                output: merged,
            }
        }
        Err(e) => RunResult::failure(-1, format!("无法执行 git：{e}")),
    }
}

/// 读取仓库状态。
///
/// 一次性取分支、领先/落后、改动清单。任一环节失败都不让整个函数报错，
/// 而是返回能拿到的部分 —— 界面宁可显示不完整也不要一片空白。
pub fn status() -> GitStatus {
    let dir = repo_dir();

    // 先确认是不是 git 仓库
    if !dir.join(".git").exists() {
        return GitStatus {
            is_repo: false,
            branch: String::new(),
            has_upstream: false,
            ahead: 0,
            behind: 0,
            changed: Vec::new(),
            remote: String::new(),
        };
    }

    // 分支名
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // 领先/落后上游。用 --left-right --count 一次拿到两个数。
    // 失败通常就是「当前分支没配上游」；两个数保持 0 并显式标记，
    // 界面对这种情况要显示「未设置上游」，不能显示成「领先 0」。
    let (mut ahead, mut behind) = (0usize, 0usize);
    let mut has_upstream = false;
    if let Ok(o) = git(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
        if o.status.success() {
            has_upstream = true;
            let text = String::from_utf8_lossy(&o.stdout);
            let mut parts = text.split_whitespace();
            ahead = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            behind = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    // 远端地址
    let remote = git(&["remote", "get-url", "origin"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // 改动清单。用 porcelain 格式，每个路径一行
    let mut changed = Vec::new();
    if let Ok(o) = git(&["status", "--porcelain=v1"]) {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                if line.len() < 4 {
                    continue;
                }
                let code = line[..2].trim().to_string();
                let path = line[3..].trim().to_string();
                // 改名记录形如 "R  old -> new"，取箭头后面那个
                let path = if let Some(idx) = path.find(" -> ") {
                    path[idx + 4..].to_string()
                } else {
                    path
                };
                changed.push(GitChange { path, status: code });
            }
        }
    }

    GitStatus {
        is_repo: true,
        branch,
        has_upstream,
        ahead,
        behind,
        changed,
        remote,
    }
}

/// 按指定路径提交。
///
/// 关键点：**逐个 `git add <path>`，绝不 `git add -A` 或 `git add .`**。
/// 空路径列表直接拒绝，避免"提交了个空提交"这种事。
pub fn commit(paths: &[String], message: &str) -> RunResult {
    if paths.is_empty() {
        return RunResult::failure(1, "没有指定要提交的路径，已取消");
    }
    if message.trim().is_empty() {
        return RunResult::failure(1, "提交信息不能为空，已取消");
    }

    // 先校验：所有路径都必须落在仓库内，避免误把仓库外的东西加进来
    let repo = repo_dir();
    for p in paths {
        let full = repo.join(p);
        let normalized = normalize_path(&full);
        if !normalized.starts_with(normalize_path(&repo)) {
            return RunResult::failure(1, format!("路径不在仓库内，已拒绝：{p}"));
        }
    }

    // 逐个 add
    let mut added = Vec::new();
    for p in paths {
        let r = git_result(&["add", "--", p]);
        if !r.ok {
            return RunResult::failure(
                r.exit_code,
                format!("添加路径失败：{p}\n{}", r.output),
            );
        }
        added.push(p.clone());
    }

    // 提交
    let r = git_result(&["commit", "-m", message]);
    let mut output = r.output;
    if r.ok {
        output.push_str(&format!("\n共提交 {} 个路径：", added.len()));
        for p in &added {
            output.push_str("\n  ");
            output.push_str(p);
        }
    }
    RunResult {
        ok: r.ok,
        exit_code: r.exit_code,
        output,
    }
}

/// 推送到当前分支的上游。
pub fn push() -> RunResult {
    git_result(&["push"])
}

/// 把路径里的 `..` 和 `.` 消掉，用于安全比较。
///
/// 不追求和内核完全一致的语义，只要能挡住 `../` 逃逸就够。
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
