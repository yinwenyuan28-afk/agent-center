//! 控制台归属处理：让 GUI 启动零黑窗口，同时保持 CLI 输出（含重定向）可用。
//!
//! ## 问题
//!
//! 一个 exe 两用：不带参数开窗口，带子命令输出文本。两种用法对 PE 子系统
//! （Subsystem）的要求相反：
//!
//! - **console 子系统（3）**：CLI 天然可用，但系统在**进程创建那一刻**就分配
//!   控制台窗口。双击时先闪一个黑窗口，我们的代码还没开始跑，来不及做任何事。
//!   事后 `FreeConsole` 只能缩短它，消不掉闪现 —— 这是用户反馈的实际问题。
//! - **windows 子系统（2）**：系统不给控制台，双击零闪现。但 CLI 启动时
//!   stdout 可能是空的，直接 `println!` 不输出。
//!
//! 选了后者。闪现每次启动都能看到，CLI 输出的问题可以在进程内补回来。
//!
//! ## 补法：先看继承来的句柄，再决定要不要附加控制台
//!
//! 实测（GUI 子系统进程从 cmd 启动）：
//!
//! ```text
//! 场景                 GetStdHandle(OUT)      直接用能写出吗
//! cmd 直接运行         有效句柄                是
//! cmd 重定向到文件     有效句柄（指向文件）      是
//! cmd 接管道           有效句柄（指向管道）      是
//! 双击（无终端）       null / 无效              —— CLI 不会走这条路径
//! ```
//!
//! 所以顺序必须是：
//!
//! 1. **先用 `GetStdHandle(STD_OUTPUT_HANDLE)` 看有没有可用的句柄**。
//!    有就什么都不做 —— 它可能指向终端、文件或管道，接管成 `CONOUT$` 会
//!    把重定向和管道输出整个丢掉。
//! 2. 只有句柄无效时才 `AttachConsole(ATTACH_PARENT_PROCESS)` 接父控制台，
//!    再把标准句柄绑到 `CONOUT$`。
//!
//! 早先 `main.rs` 记的经验「AttachConsole 在父进程是正在等待的 cmd 时经常
//! 拿不到，导致 status 静默失败」—— 实测 AttachConsole 本身是成功的。
//! 当时静默失败的真实原因是附加后**没有重新绑定标准句柄**，std 缓存的仍是
//! 启动时那个无效值，输出被直接丢弃。
//!
//! ## 已知边界
//!
//! 双击启动后走 GUI 分支，本函数不参与。若双击一个带子命令的场景
//! （如把 exe 拖进命令行带参数），父进程可能没有控制台，此时没有输出目的地，
//! 属于可接受行为。

/// 确保 CLI 分支有可用的标准输出。
///
/// 返回是否有一条可写的输出通道（继承的句柄或接上的父控制台）。
/// 必须在任何输出之前调用。
#[cfg(windows)]
pub fn ensure_output_channel() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::GetStdHandle;
    use windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE;

    // 第一步：继承来的句柄是否可用。
    // SAFETY: 无参数，返回当前进程的标准输出句柄，可能为 null 或 INVALID。
    let inherited = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if !inherited.is_null() && inherited != INVALID_HANDLE_VALUE as _ {
        // 句柄存在。它可能指向终端、重定向的文件或管道 —— 一律保留原样，
        // 绝不能改绑到 CONOUT$，否则 `> out.txt` 和 `| more` 全都会失效。
        return true;
    }

    // 第二步：没有可用句柄，说明父进程可能是个带控制台的终端。
    // 附加过去，然后把标准句柄绑到 CONOUT$。
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
    if !attached {
        return false;
    }
    rebind_std_to_console();
    true
}

/// 把标准流重新绑到控制台设备。
///
/// 用 std 的 `OpenOptions` 打开 `CONOUT$` 而不是 `CreateFileW`：后者需要
/// 额外的 `Win32_Security` 特性。
#[cfg(windows)]
fn rebind_std_to_console() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    for (name, handle_id) in [
        ("CONOUT$", STD_OUTPUT_HANDLE),
        ("CONOUT$", STD_ERROR_HANDLE),
        ("CONIN$", STD_INPUT_HANDLE),
    ] {
        if let Ok(f) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
        {
            // SAFETY: f 是有效的文件句柄，生命周期由后续 forget 保证到进程结束。
            unsafe {
                let _ = SetStdHandle(handle_id, f.as_raw_handle() as _);
            }
            // 刻意泄漏：进程马上要走 CLI 分支并 exit，
            // 提前 drop 会关掉刚绑上的句柄，之后的输出全部失效。
            std::mem::forget(f);
        }
    }
}

/// 非 Windows 平台的控制台行为本来就符合预期，无需处理。
#[cfg(not(windows))]
pub fn ensure_output_channel() -> bool {
    true
}
