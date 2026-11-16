//! 子进程启动的 Windows 统一封装。
//!
//! ## 为什么需要这个
//!
//! 主程序是 **windows 子系统**（PE `Subsystem = 2`），自己不带控制台，双击启动
//! 零黑窗口。但它启动的子进程（`git.exe`、`npx`、`cmd`、`powershell`、
//! `opencode`）大多是 **console 子系统**程序。
//!
//! Windows 的规则：一个没有控制台的进程启动 console 子系统子进程时，系统会
//! **给这个子进程新建一个控制台窗口**。所以打开 Git 页面时，`status()` 内部连续
//! 跑 `rev-parse`、`rev-list`、`remote get-url`、`status --porcelain` 四条 git
//! 命令，就会连闪四个黑窗口。
//!
//! 解决办法是启动时传 `CREATE_NO_WINDOW`：告诉系统「给这个子进程建控制台，
//! 但不要显示窗口」。子进程照常工作，用户看不到任何窗口。
//!
//! ## 布局约定
//!
//! 需要拿到子进程 stdout 的用 [`output`]（内部已 `output()`，自动带管道）；
//! 只需要「跑一下、看退出码」的用 [`status`]；要自己接管输入输出的用
//! [`new`] 拿到一个已经设好标志的 `Command`。
//!
//! 非 Windows 平台这些函数直通 `std::process::Command`，不做任何额外处理。

use std::process::Command;

/// Windows 上隐藏子进程的控制台窗口。
///
/// `CREATE_NO_WINDOW` 的值为 `0x0800_0000`。用常量而不是引 `windows-sys`
/// 的 `CREATE_NO_WINDOW`，因为那个常量在 `Win32_System_Threading` 特性下，
/// 为一个数字开一整个特性不值得。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 造一个已设好「不显示窗口」标志的命令。
///
/// 调用方可以继续 `.arg()` / `.current_dir()`。
#[cfg(windows)]
pub fn new(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(program);
    // SAFETY: creation_flags 只是把一个整数位标志交给 CreateProcess，
    // 不涉及内存或句柄所有权，没有安全性问题。
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// 非 Windows 平台直接透传。
#[cfg(not(windows))]
pub fn new(program: &str) -> Command {
    Command::new(program)
}

/// 执行命令并收集输出（stdout + stderr）。
///
/// `current_dir` 为 `None` 时用当前工作目录。
pub fn output(
    program: &str,
    args: &[&str],
    current_dir: Option<&std::path::Path>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = new(program);
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    cmd.output()
}

/// 执行命令、丢弃输出，只返回是否成功。
///
/// 用于「跑一下看能不能跑通」这类探测，不需要读取 output。
pub fn status(
    program: &str,
    args: &[&str],
    current_dir: Option<&std::path::Path>,
) -> std::io::Result<std::process::ExitStatus> {
    let mut cmd = new(program);
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    cmd.status()
}
