// agentcenter 入口。
//
// 一个二进制两种用法：
// - 无参数（或 `ui`）→ 开图形窗口
// - 带子命令 → 执行 CLI 并退出
//
// ## 为什么是 `windows_subsystem = "windows"`
//
// 这个程序的两种用法对 PE 子系统要求相反：
//
// - console 子系统（3）：CLI 输出天然可用，但系统在进程创建那一刻就分配
//   控制台窗口 —— 双击时先闪一个黑窗口，我们的代码还没开始跑。事后
//   `FreeConsole` 只能缩短它，消不掉。这是用户实际反馈的问题。
// - windows 子系统（2）：系统不给控制台，双击零闪现。CLI 需要自己把
//   父进程的控制台接回来，由 `console::attach_to_parent_console` 负责。
//
// 选了后者，因为闪现是每次启动都能看到的困扰，而 CLI 输出可以在进程内补回来。
//
// 早先这里写着「AttachConsole 经常拿不到，结果 status 静默失败」，实测
// 那个结论不准确：真正漏的是附加后没有重新绑定标准句柄。详见 `console.rs`
// 顶部的实测记录。

#![windows_subsystem = "windows"]

use agentcenter_lib::cli::{Cli, Command};
use clap::Parser;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        // 没有子命令，或者显式要求 ui，都开窗口。
        // 这条路径不碰控制台：GUI 子系统进程本来就没有，也不该造出来。
        None | Some(Command::Ui) => {
            agentcenter_lib::run_gui();
        }
        Some(cmd) => {
            // CLI 分支：确认有可用的输出通道。
            // 继承来的句柄（终端 / 重定向文件 / 管道）会原样保留；
            // 没有的话再附加父控制台。见 console.rs 顶部的实测表。
            agentcenter_lib::console::ensure_output_channel();
            let code = agentcenter_lib::cli_run::run(cmd);
            std::process::exit(code);
        }
    }
}
