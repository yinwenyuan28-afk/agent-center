// agentcenter 入口。
//
// 一个二进制两种用法：
// - 无参数（或 `ui`）→ 开图形窗口
// - 带子命令 → 执行 CLI 并退出
//
// **这里不用 `windows_subsystem = "windows"`**。
// 那个属性只能整包生效，一旦设了，CLI 输出就得靠 AttachConsole 抢回来，
// 而 AttachConsole 在"父进程是正在等待的 cmd"这种场景下经常拿不到，
// 结果是命令静默失败——这正是之前 release 版 `status` 什么都不打印的原因。
//
// 代价是双击 exe 时会附带一个控制台窗口。对开发工具来说这反而有用：
// 能直接看到启动日志和错误。做成完全无窗口的版本需要拆成两个 exe
// 或引入更复杂的 Windows 引导逻辑，不值得。

use agentcenter_lib::cli::{Cli, Command};
use clap::Parser;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        // 没有子命令，或者显式要求 ui，都开窗口
        None | Some(Command::Ui) => {
            agentcenter_lib::run_gui();
        }
        Some(cmd) => {
            let code = agentcenter_lib::cli_run::run(cmd);
            std::process::exit(code);
        }
    }
}
