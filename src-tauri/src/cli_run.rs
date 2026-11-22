//! CLI 子命令的实际执行，输出面向终端的纯文本。
//!
//! 与 IPC 命令层的区别：这里不返回结构化数据，直接把结果打印出来，
//! 便于脚本调用和人工排查。所有输出保持 ASCII 安全，避免 Windows
//! 终端在 GBK 代码页下花屏。

use crate::cli::{BridgeCmd, Command, DocCmd, GitCmd, SkillsCmd};
use crate::config::Config;
use crate::{git, link, skills, skillscli};

// 一次 CLI 调用的全局开关。
//
// `--json` 是全局参数，但执行函数拿不到 `Cli` 结构（它只有 `command`）。
// 所以由 `main` 在分派前写进这个线程局部变量，执行函数从这里读。
//
// 用线程局部而不是全局静态：CLI 只在主线程跑，线程局部语义更窄，
// 测试并行跑也不会互相污染。
thread_local! {
    static JSON_MODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 设置是否走 JSON 输出。
pub fn set_json_mode(on: bool) {
    JSON_MODE.with(|c| c.set(on));
}

/// 当前是否走 JSON 输出。
fn json_mode() -> bool {
    JSON_MODE.with(|c| c.get())
}

/// 输出一个可序列化的值。
///
/// JSON 模式下打印缩进 JSON；非 JSON 模式返回 false，让调用方走原来的
/// 文本输出路径。这样每个读命令只需要在一个地方开头试一次。
fn print_json<T: serde::Serialize>(value: &T) -> bool {
    if !json_mode() {
        return false;
    }
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(e) => {
            // 序列化失败极少见（都是我们自己定义的结构），但真发生时
            // 静默输出空对象会让调用方以为「没有数据」，所以打到 stderr
            // 并以非零退出，由调用方决定。
            eprintln!("JSON 序列化失败: {e}");
            true
        }
    }
}


/// 执行一条子命令，返回进程退出码。
pub fn run(cmd: Command) -> i32 {
    match cmd {
        Command::Ui => {
            // 由 main 处理，这里不该被调用
            eprintln!("ui 子命令应由主入口处理");
            1
        }
        Command::Mappings => {
            let cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("读取配置失败: {e}");
                    return 1;
                }
            };
            let specs = cfg.resolved_mappings();
            // JSON 模式下把所有映射当成一个数组输出，agent 直接按字段取值，
            // 不用解析列宽对齐的文本
            if print_json(
                &specs
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "agent": s.agent,
                            "label": s.label,
                            "source": crate::config::contract_tilde(&s.source),
                            "target": crate::config::contract_tilde(&s.target),
                            "mode": s.mode,
                            "disabled": s.disabled,
                            "installed": s.config_dir.exists(),
                        })
                    })
                    .collect::<Vec<_>>(),
            ) {
                return 0;
            }
            for spec in specs {
                println!(
                    "{:<12} {:<40} -> {}",
                    spec.agent,
                    spec.source.display(),
                    spec.target.display()
                );
            }
            0
        }
        Command::Status => {
            let cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("读取配置失败: {e}");
                    return 1;
                }
            };
            let mut problems = 0;
            let mut rows = Vec::new();
            for spec in cfg.resolved_mappings() {
                let m = link::inspect(&spec);
                // 计数只关心「装了、没取消、但不 ok」
                if !matches!(m.health, crate::types::HealthState::Ok) && m.installed && !m.disabled {
                    problems += 1;
                }
                rows.push(m);
            }

            if print_json(&rows) {
                // JSON 模式下把「需要处理」的项数也带出去，退出码保持同样语义，
                // 调用方既能读结构化结果又能靠退出码做粗判断
                return if problems > 0 { 2 } else { 0 };
            }

            for m in &rows {
                let health = format!("{:?}", m.health).to_lowercase();
                let kind = m
                    .actual
                    .map(|k| format!("{k:?}").to_lowercase())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "{:<12} {:<10} {:<10} {}",
                    m.agent, health, kind, m.target
                );
                if m.disabled {
                    println!("             └ 已取消映射，同步会跳过");
                }
            }
            println!("\n{problems} 项需要处理");
            // 有需要处理的项时返回 2，方便脚本判断
            if problems > 0 {
                2
            } else {
                0
            }
        }
        Command::Sync { dry_run } => {
            let cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("读取配置失败: {e}");
                    return 1;
                }
            };

            // 先刷新合成文件再建链接，和 GUI 的 `sync_mappings_inner` 保持一致。
            // 顺序不能反：`sync_one` 会检查源文件存在，而合成文件是派生物。
            let mut failed = 0;
            for o in crate::doc::compose_all(!dry_run) {
                match o.action.as_str() {
                    "error" => {
                        println!("[fail] 文档合成 {}: {}", o.agent, o.message);
                        failed += 1;
                    }
                    "created" | "updated" | "removed" => {
                        println!("[doc]  {}: {}", o.agent, o.message)
                    }
                    "skipped" if dry_run => {
                        println!("[plan] 文档合成 {}: {}", o.agent, o.message)
                    }
                    _ => {}
                }
            }

            for spec in cfg.resolved_mappings() {
                if spec.disabled {
                    println!("[skip] {}: 已取消映射", spec.label);
                    continue;
                }
                if !spec.config_dir.exists() {
                    println!("[skip] {}: 配置目录不存在", spec.label);
                    continue;
                }
                if dry_run {
                    println!(
                        "[plan] {}: {} -> {}",
                        spec.label,
                        spec.source.display(),
                        spec.target.display()
                    );
                    continue;
                }
                match link::sync_one(&spec) {
                    Ok((kind, note)) => {
                        println!("[ok]   {}: {:?} - {}", spec.label, kind, note)
                    }
                    Err(e) => {
                        println!("[fail] {}: {}", spec.label, e);
                        failed += 1;
                    }
                }
            }
            if failed > 0 {
                1
            } else {
                0
            }
        }
        Command::Unmap { agent } => run_unmap(&agent),
        Command::Remap { agent } => run_remap(&agent),
        Command::Doc(sub) => run_doc(sub),
        Command::Skills(sub) => run_skills(sub),
        Command::Creds(sub) => run_creds(sub),
        Command::Providers(sub) => run_providers(sub),
        Command::Claude(sub) => run_claude(sub),
        Command::Bridge(sub) => run_bridge(sub),
        Command::Mcp(sub) => run_mcp(sub),
        Command::Usage(sub) => run_usage(sub),
        Command::Agents => run_agents(),
        Command::Models(sub) => run_models(sub),
        Command::Git(sub) => run_git(sub),
    }
}

/// Claude 本地协议转换端口的控制。
fn run_bridge(cmd: BridgeCmd) -> i32 {
    use crate::providers::claude_bridge as bridge;

    match cmd {
        BridgeCmd::Status => {
            let st = bridge::status_report();
            if print_json(&st) {
                return 0;
            }
            println!(
                "端口      {} ({})",
                st.port,
                if !st.listening {
                    "没有进程在听"
                } else if st.ours {
                    "agentcenter 转换层在听"
                } else {
                    "被其它程序占用"
                }
            );
            println!("转换配置  {}{}", st.config_path, if st.configured { "" } else { "（不存在）" });
            if st.configured {
                println!("上游      {} ({})", st.upstream, st.kind);
            }
            match st.pid {
                Some(pid) => println!("后台进程  pid {pid}"),
                None => println!("后台进程  无"),
            }
            if !st.listening && st.configured {
                println!();
                println!("Claude 走非 Anthropic 协议时需要这个端口。用 `agentcenter bridge start` 拉起。");
            }
            0
        }

        BridgeCmd::Run => match bridge::run_blocking() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },

        BridgeCmd::Start => match bridge::start_detached() {
            Ok(pid) => {
                println!("转换端口已启动，pid {pid}");
                println!("Claude 的 ANTHROPIC_BASE_URL 指向 {}", bridge::bridge_base_url());
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },

        BridgeCmd::Stop => match bridge::stop_detached() {
            Ok(Some(pid)) => {
                println!("已停掉转换进程 pid {pid}");
                0
            }
            Ok(None) => {
                println!("没有后台运行的转换进程");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
    }
}

/// 母版指令文件的读写。
///
/// 边界和图形界面一致：`write` 覆盖，`create` 只在不存在时写。
/// 内容来源支持 `--file` 和标准输入两种 —— agent 调用时用管道更自然
/// （避免为了写一段文本先落一个临时文件）。
fn run_doc(cmd: DocCmd) -> i32 {
    use crate::doc;

    match cmd {
        DocCmd::Path => {
            let p = doc::doc_path();
            let shown = crate::config::contract_tilde(&p);
            if print_json(&serde_json::json!({
                "path": shown,
                // 绝对路径里的分隔符统一成反斜杠：这是给调用方直接传给
                // 文件 API 用的，混着 "C:\\Users\\leaf\\.agents/AGENTS.md"
                // 这种形态在某些工具里会被当成两个不同路径
                "absolutePath": p.display().to_string().replace('/', "\\"),
                "exists": p.exists(),
                "bytes": if p.exists() {
                    std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                },
            })) {
                return 0;
            }
            println!("{shown}");
            println!("是否存在  {}", if p.exists() { "是" } else { "否" });
            0
        }

        DocCmd::Read => match doc::read() {
            Ok(d) => {
                if print_json(&serde_json::json!({
                    "path": crate::config::contract_tilde(&doc::doc_path()),
                    "exists": d.exists,
                    "content": d.content,
                    "bytes": d.content.len(),
                })) {
                    return 0;
                }
                if !d.exists {
                    eprintln!("母版不存在：{}", doc::doc_path().display());
                    return 1;
                }
                // 原样输出，不加任何装饰 —— 这是文件内容，调用方可能要重定向
                print!("{}", d.content);
                if !d.content.ends_with('\n') {
                    println!();
                }
                0
            }
            Err(e) => {
                eprintln!("读取失败: {e}");
                1
            }
        },

        DocCmd::Write { file } => {
            let content = match read_content_arg(file.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            let r = doc::write(&content);
            println!("{}", r.output);
            if r.ok {
                0
            } else {
                r.exit_code
            }
        }

        DocCmd::Create { file } => {
            let content = match read_content_arg(file.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            match doc::create(&content) {
                r if r.ok => {
                    println!("{}", r.output);
                    0
                }
                r => {
                    // 已存在时 create 会拒绝。这是有意的保护，
                    // 但要给出可执行的下一步，否则调用方只能猜
                    eprintln!("{}", r.output);
                    eprintln!("要覆盖请用 `agentcenter doc write`");
                    r.exit_code
                }
            }
        }

        DocCmd::List => {
            let docs = doc::list_agent_docs();
            if print_json(&docs) {
                return 0;
            }
            if docs.is_empty() {
                println!("没有可接管指令文件的 agent。");
                return 0;
            }
            println!("{:<12} {:<10} {:<10} {:>8}  {}", "agent", "专用片段", "合成", "字节", "目标");
            for d in &docs {
                let specific = if d.specific_exists { "有" } else { "-" };
                let composed = match (&d.composed_path, d.composed_stale) {
                    (None, _) => "-",
                    (Some(_), true) => "待更新",
                    (Some(_), false) => "最新",
                };
                println!(
                    "{:<12} {:<10} {:<10} {:>8}  {}",
                    d.agent, specific, composed, d.bytes, d.target
                );
            }
            let stale = docs.iter().filter(|d| d.composed_stale).count();
            if stale > 0 {
                println!("\n{stale} 个合成文件待更新，跑 `agentcenter doc compose` 或 `sync`。");
            }
            0
        }

        DocCmd::Specific {
            agent,
            file,
            clear,
        } => {
            if crate::agents::find(&agent).is_none() {
                eprintln!("不认识的 agent：{agent}");
                eprintln!("可用值：{}", crate::agents::ids().join(" / "));
                return 1;
            }

            // 只读模式：打印当前专用片段
            if file.is_none() && !clear {
                return match doc::specific_read(&agent) {
                    Ok(d) => {
                        if print_json(&serde_json::json!({
                            "agent": agent,
                            "path": crate::config::contract_tilde(&doc::specific_path(&agent)),
                            "exists": d.exists,
                            "content": d.content,
                            "bytes": d.content.len(),
                        })) {
                            return 0;
                        }
                        if !d.exists {
                            eprintln!("{agent} 没有专用片段，它直接读通用母版。");
                            eprintln!(
                                "要新建：`agentcenter doc specific {agent} --file <路径>`"
                            );
                            return 1;
                        }
                        // 原样输出，方便重定向到文件
                        print!("{}", d.content);
                        if !d.content.ends_with('\n') {
                            println!();
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("读取失败: {e}");
                        1
                    }
                };
            }

            // 写入模式。--clear 优先，其次读 --file / 标准输入
            let content = if clear {
                String::new()
            } else {
                match read_content_arg_allow_empty(file.as_deref()) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("{e}");
                        return 1;
                    }
                }
            };

            let r = doc::specific_write(&agent, &content);
            println!("{}", r.output);
            if r.ok {
                // 写完顺手合成，让 agent 立刻能读到新内容
                for o in doc::compose_all(true) {
                    if o.agent == agent {
                        println!("[合成] {}", o.message);
                    }
                }
                0
            } else {
                r.exit_code
            }
        }

        DocCmd::Compose { dry_run } => {
            let outcomes = doc::compose_all(!dry_run);
            if print_json(&outcomes) {
                return 0;
            }
            if outcomes.is_empty() {
                println!("没有需要合成的文档。");
                return 0;
            }
            for o in &outcomes {
                println!("[{:<8}] {}：{}", o.action, o.agent, o.message);
            }
            0
        }
    }
}

/// 和 [`read_content_arg`] 一样，但允许空内容。
///
/// 专用片段写空内容是有意义的语义 —— "取消这个 agent 的专用内容"，
/// 所以不能沿用母版那条"空内容一律拒绝"的规则。专用片段不是唯一真实来源，
/// 清空它只是回到通用母版，不会丢失别处的数据。
fn read_content_arg_allow_empty(file: Option<&str>) -> Result<String, String> {
    use std::io::Read;

    match file {
        Some(path) => {
            std::fs::read_to_string(path).map_err(|e| format!("读取 {path} 失败: {e}"))
        }
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("读取标准输入失败: {e}"))?;
            Ok(buf)
        }
    }
}

/// 读 `--file` 指定的文件，或从标准输入读全部内容。
///
/// 标准输入为空时报错：`doc write` 拿到空内容会把母版清空，
/// 这在脚本里很容易因为管道接错而发生，所以宁可拒绝。
fn read_content_arg(file: Option<&str>) -> Result<String, String> {
    use std::io::Read;

    let content = match file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| format!("读取 {path} 失败: {e}"))?,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("读取标准输入失败: {e}"))?;
            buf
        }
    };

    if content.trim().is_empty() {
        return Err(
            "内容为空，已中止。要从文件读请用 --file <路径>，或通过管道传入内容".to_string(),
        );
    }
    Ok(content)
}

/// 取消映射：删掉目标链接并写「已取消」标记。
///
/// 删除边界由 [`link::remove_one`] 保证 —— 母版不动，被改过的目标进备份。
fn run_unmap(agent: &str) -> i32 {
    let mut cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置失败: {e}");
            return 1;
        }
    };
    let resolved = cfg.resolved_mappings();
    let Some(m) = resolved.iter().find(|m| m.agent == agent) else {
        eprintln!("找不到 agent: {agent}");
        return 1;
    };

    match link::remove_one(m) {
        Ok(note) => println!("[{}] {note}", m.label),
        Err(e) => {
            eprintln!("[{}] 取消映射失败: {e}", m.label);
            return 1;
        }
    }

    if let Err(e) = cfg.set_mapping_disabled(agent, true).and_then(|_| cfg.save()) {
        eprintln!("写入配置失败: {e}");
        return 1;
    }
    println!("已记下「已取消」，后续 sync 会跳过。用 `agentcenter remap {agent}` 恢复。");
    0
}

/// 恢复映射：清掉「已取消」标记并立刻重建链接。
fn run_remap(agent: &str) -> i32 {
    let mut cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置失败: {e}");
            return 1;
        }
    };

    if let Err(e) = cfg.set_mapping_disabled(agent, false).and_then(|_| cfg.save()) {
        eprintln!("写入配置失败: {e}");
        return 1;
    }

    // 重新加载，让 resolved 里的 disabled 已经是 false
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置失败: {e}");
            return 1;
        }
    };
    let resolved = cfg.resolved_mappings();
    let Some(m) = resolved.iter().find(|m| m.agent == agent) else {
        eprintln!("找不到 agent: {agent}");
        return 1;
    };

    match link::sync_one(m) {
        Ok((kind, note)) => {
            println!("[{}] {:?} - {note}", m.label, kind);
            0
        }
        Err(e) => {
            eprintln!("[{}] 恢复失败: {e}", m.label);
            1
        }
    }
}

/// 列出所有 agent 及其支持的能力。
///
/// 这是「一个实现文件 = 一个 agent」重构之后的自检入口：
/// 新增 agent 后跑一下，能确认清单、路径、能力标记都对。
fn run_agents() -> i32 {
    use crate::agents;

    // JSON 模式：直接给出结构化能力表。这是 skill 最需要的入口 ——
    // 「这个 agent 支持哪种注入」全靠它判断，解析定宽表格太脆
    if print_json(
        &agents::all()
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id(),
                    "label": a.label(),
                    "order": a.order(),
                    "installed": a.installed(),
                    "configDir": crate::config::contract_tilde(&a.config_dir()),
                    "instructionFile": a
                        .instruction_file()
                        .map(|p| crate::config::contract_tilde(&p)),
                    "skillsDir": crate::config::contract_tilde(&a.skills_dir()),
                    "readsSharedSkills": a.reads_shared_skills(),
                    "supportsProvider": a.supports_provider(),
                    "providerReady": a.provider_ready(),
                    "providerTargets": a
                        .provider_targets()
                        .iter()
                        .map(|p| crate::config::contract_tilde(p))
                        .collect::<Vec<_>>(),
                    "singleSlot": a.single_slot(),
                    "supportedKinds": a
                        .supported_kinds()
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>(),
                    "supportsMcp": a.supports_mcp(),
                    "mcpReady": a.mcp_ready(),
                    "supportsUsage": a.supports_usage(),
                    "usageSources": a
                        .usage_sources()
                        .iter()
                        .map(|p| crate::config::contract_tilde(p))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
    ) {
        return 0;
    }

    println!(
        "{:<12} {:<20} {:<6} {:<9} {:<9} {:<6} {}",
        "id", "名称", "已安装", "provider", "mcp", "单槽位", "指令文件"
    );
    println!("{}", "-".repeat(100));

    for a in agents::all() {
        let installed = if a.installed() { "是" } else { "否" };
        let prov = if a.supports_provider() {
            if a.provider_ready() {
                "可注入"
            } else {
                "未就位"
            }
        } else {
            "—"
        };
        let mcp = if a.supports_mcp() {
            if a.mcp_ready() {
                "可注入"
            } else {
                "未就位"
            }
        } else {
            "—"
        };
        let slot = if a.single_slot() { "是" } else { "" };
        let instr = a
            .instruction_file()
            .map(|p| crate::config::contract_tilde(&p))
            .unwrap_or_else(|| "—".to_string());

        println!(
            "{:<12} {:<20} {:<6} {:<9} {:<9} {:<6} {}",
            a.id(),
            a.label(),
            installed,
            prov,
            mcp,
            slot,
            instr
        );
    }

    println!("\n共 {} 个 agent。", agents::all().len());
    0
}

/// 模型元数据子命令。
fn run_models(cmd: crate::cli::ModelsCmd) -> i32 {
    use crate::cli::ModelsCmd;

    match cmd {
        ModelsCmd::Backfill { dry_run } => {
            let report = match crate::providers::backfill::run(dry_run) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("回填失败: {e}");
                    return 1;
                }
            };

            if report.lines.is_empty() {
                println!("没有可回填的字段（母版里的模型元数据都已齐全，或各 agent 配置里读不到）。");
            } else {
                for line in &report.lines {
                    println!(
                        "  {} / {}  ← {}",
                        line.provider, line.model, line.source
                    );
                    println!("      {}", line.filled.join(", "));
                }
                println!(
                    "\n共补 {} 个字段{}。",
                    report.filled_fields,
                    if report.wrote {
                        "，已写入母版"
                    } else if dry_run {
                        "（预览，未写盘）"
                    } else {
                        ""
                    }
                );
            }

            if !report.skipped_providers.is_empty() {
                println!("\n母版里没有对应端点的 provider（未导入，仅供核对）：");
                for s in &report.skipped_providers {
                    println!("  {s}");
                }
            }

            // 协议不一致：这是最容易出事的一类。
            // 母版写 anthropic 但 agent 里实际跑 openai 时，注入会按母版
            // 改回去，可能把本来能用的配置改坏。所以单独、醒目地列出来。
            if !report.kind_mismatches.is_empty() {
                println!("\n[注意] 这些 agent 里实际用的协议和母版不同：");
                for m in &report.kind_mismatches {
                    println!(
                        "  {} / {}：母版 {} → 实际 {}",
                        m.provider, m.agent, m.master, m.actual
                    );
                }
                println!("  注入会按母版的值写回。如果实际值才是对的，改母版：");
                println!("  agentcenter providers set <id> --agent-kind <agent>=<协议>");
                println!("  或者用界面里「各 agent 实际用的协议」那一组下拉。");
            }

            0
        }
    }
}

/// 把一条 MCP server 转成 JSON 视图。
///
/// `env` / `headers` 只给**键名**，不给值 —— 和界面一致：远程型的
/// headers 里常放 Authorization，stdio 型的 env 里常放 key。
/// 需要看值就打开 `~/.agents/mcp.toml`，那是本地明文文件。
fn mcp_server_json(s: &crate::mcp::model::McpServer) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "name": s.display_name(),
        "transport": s.transport.as_str(),
        "command": s.command,
        "args": s.args,
        "cwd": s.cwd,
        "url": s.url,
        "envKeys": s.env.keys().collect::<Vec<_>>(),
        "headerKeys": s.headers.keys().collect::<Vec<_>>(),
        "enabled": s.enabled,
        "agents": s.agents,
        "note": s.note,
    })
}

/// MCP server 统一管理子命令。
fn run_mcp(cmd: crate::cli::McpCmd) -> i32 {
    use crate::cli::McpCmd;
    use crate::mcp::store;

    match cmd {
        McpCmd::List => {
            let list = match store::load() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };
            if print_json(&serde_json::json!({
                "masterPath": store::path().display().to_string(),
                "servers": list.iter().map(mcp_server_json).collect::<Vec<_>>(),
            })) {
                return 0;
            }
            if list.is_empty() {
                println!("母版里还没有 MCP server。");
                println!("用 `agentcenter mcp example` 看格式。");
                return 0;
            }
            println!("母版：{}\n", store::path().display());
            for s in &list {
                println!("{}  ({})", s.id, s.display_name());
                println!("  transport  {}", s.transport.as_str());
                if s.is_remote() {
                    println!("  url        {}", s.url);
                } else {
                    println!("  command    {} {}", s.command, s.args.join(" "));
                    if !s.cwd.is_empty() {
                        println!("  cwd        {}", s.cwd);
                    }
                }
                println!("  enabled    {}", s.enabled);
                if !s.env.is_empty() {
                    println!("  env        {} 个变量", s.env.len());
                }
                println!("  agents     {}", s.agents.join(", "));
                println!();
            }
            0
        }

        McpCmd::Diff => {
            let report = match crate::mcp::diff_run() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("对比失败: {e}");
                    return 1;
                }
            };

            if print_json(&report) {
                return 0;
            }

            if report.lines.is_empty() {
                println!("母版是空的，或各 agent 都没有 MCP 配置。");
                return 0;
            }

            println!("=== MCP 差异报告（不写盘）===\n");
            let mut last = String::new();
            for l in &report.lines {
                if l.agent != last {
                    println!("[{}]", l.agent);
                    last = l.agent.clone();
                }
                let mark = match l.action.as_str() {
                    "add" => "  + ",
                    "update" => "  ~ ",
                    _ => "  = ",
                };
                println!("{mark}{}  {}", l.target, l.detail);
            }

            println!(
                "\n汇总：{} 个 agent 受影响，{} 处改动",
                report.agent_count, report.changed_files
            );
            0
        }

        McpCmd::Sync { dry_run } => {
            let list = match store::load() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };
            if list.is_empty() {
                eprintln!("母版里没有 MCP server，无事可做。");
                return 1;
            }

            // 运行中的 agent 会在退出时用内存状态覆盖配置。
            // 只在真正写入前提醒，dry-run 不需要
            if !dry_run {
                let running = crate::agents::running_agents();
                if !running.is_empty() {
                    eprintln!(
                        "[提醒] 这些 agent 正在运行，它们可能会用内存状态覆盖刚写入的配置：{}",
                        running.join(", ")
                    );
                    eprintln!("       建议先退出它们再同步。");
                }
            }

            match crate::mcp::sync_run(&list, dry_run) {
                Ok(r) => {
                    if print_json(&serde_json::json!({
                        "dryRun": dry_run,
                        "ok": r.ok,
                        "written": r.written,
                        "skipped": r.skipped,
                        "backups": r.backups,
                        "log": r.log,
                    })) {
                        return if r.ok { 0 } else { 1 };
                    }
                    if dry_run {
                        println!("=== 预览（未写入）===\n");
                    }
                    for line in &r.log {
                        println!("{line}");
                    }
                    if !r.backups.is_empty() {
                        println!("\n备份：");
                        for b in &r.backups {
                            println!("  {b}");
                        }
                    }
                    println!(
                        "\n{}：写入 {} 个文件，跳过 {} 个。",
                        if dry_run { "将" } else { "完成" },
                        r.written.len(),
                        r.skipped.len()
                    );
                    if r.ok {
                        0
                    } else {
                        1
                    }
                }
                Err(e) => {
                    eprintln!("同步失败: {e}");
                    1
                }
            }
        }

        McpCmd::Add {
            id,
            name,
            transport,
            command,
            args,
            cwd,
            url,
            env,
            headers,
            disabled,
            agents,
            note,
            clear_env,
            clear_headers,
        } => run_mcp_add(McpAddArgs {
            id,
            name,
            transport,
            command,
            args,
            cwd,
            url,
            env,
            headers,
            disabled,
            agents,
            note,
            clear_env,
            clear_headers,
        }),

        McpCmd::Remove { id } => {
            let mut list = match store::load() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };
            let before = list.len();
            list.retain(|s| s.id != id);
            if list.len() == before {
                eprintln!("母版里没有 {id}");
                return 1;
            }
            if let Err(e) = store::save(&list) {
                eprintln!("保存失败: {e}");
                return 1;
            }
            println!("已从母版移除 {id}");
            println!("已注入到各 agent 的条目要再跑一次 `agentcenter mcp sync` 才会清理。");
            0
        }

        McpCmd::Assign { id, agents } => match crate::mcp::matrix::set_row(&id, &agents) {
            Ok(()) => {
                println!(
                    "已设置 {id} 的分配：{}",
                    if agents.is_empty() {
                        "（无）".to_string()
                    } else {
                        agents.join(", ")
                    }
                );
                println!("跑 `agentcenter mcp sync` 让它生效。");
                0
            }
            Err(e) => {
                eprintln!("设置失败: {e}");
                1
            }
        },

        McpCmd::Matrix => match crate::mcp::matrix::build() {
            Ok(m) => {
                if print_json(&m) {
                    return 0;
                }
                if m.rows.is_empty() {
                    println!("母版里还没有 MCP server。");
                    return 0;
                }
                let labels: Vec<&str> = m.columns.iter().map(|c| c.label.as_str()).collect();
                println!("{:<24} {:<10} {}", "server", "transport", labels.join("  "));
                println!("{}", "-".repeat(24 + 1 + 10 + 1 + labels.len() * 8));
                for r in &m.rows {
                    let marks: Vec<&str> = m
                        .columns
                        .iter()
                        .map(|c| {
                            if !c.available {
                                "  - "
                            } else if r.assigned.iter().any(|a| a == &c.agent) {
                                "  ✓ "
                            } else {
                                "    "
                            }
                        })
                        .collect();
                    println!("{:<24} {:<10} {}", r.id, r.transport, marks.join(""));
                }
                let available = m.columns.iter().filter(|c| c.available).count();
                println!("\n共 {} 个 server，{} 个可用 agent。", m.rows.len(), available);
                0
            }
            Err(e) => {
                eprintln!("读取失败: {e}");
                1
            }
        },

        McpCmd::Import => match crate::mcp::matrix::import_from_agents() {
            Ok(added) => {
                if added.is_empty() {
                    println!("没有新的 server 可导入");
                } else {
                    println!("已导入 {} 个 server：{}", added.len(), added.join(", "));
                }
                0
            }
            Err(e) => {
                eprintln!("导入失败: {e}");
                1
            }
        },

        McpCmd::Example => {
            println!("{}", store::example_content());
            0
        }
    }
}

/// `mcp add` 的参数束。
///
/// 字段多，直接展开到函数签名会让调用处难以阅读，所以打成一个结构。
/// 每个字段的含义和 CLI 参数一一对应，见 [`crate::cli::McpCmd::Add`]。
struct McpAddArgs {
    id: String,
    name: Option<String>,
    transport: Option<String>,
    command: Option<String>,
    args: Vec<String>,
    cwd: Option<String>,
    url: Option<String>,
    env: Vec<String>,
    headers: Vec<String>,
    disabled: bool,
    agents: Vec<String>,
    note: Option<String>,
    clear_env: bool,
    clear_headers: bool,
}

/// `mcp add`：新增或更新母版里的一条 server。
///
/// 更新时只覆盖显式给出的字段，没给的保持原值 —— 和 `providers set`
/// 的语义一致。`--clear-env` / `--clear-headers` 是显式的清空开关，
/// 因为「没传 --env」和「传了空的 --env」在命令行上没法区分。
fn run_mcp_add(a: McpAddArgs) -> i32 {
    use crate::mcp::model::{McpServer, Transport};
    use crate::mcp::store;

    let mut list = match store::load() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("读取母版失败: {e}");
            return 1;
        }
    };

    let env_pairs = match parse_pairs(&a.env, "=", "--env") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let header_pairs = match parse_pairs(&a.headers, ":", "--header") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    let existing = list.iter().position(|s| s.id == a.id);
    let is_new = existing.is_none();

    let mut server = match existing {
        Some(i) => list[i].clone(),
        None => McpServer {
            id: a.id.clone(),
            name: String::new(),
            transport: Transport::Stdio,
            command: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            url: String::new(),
            headers: Default::default(),
            env: Default::default(),
            enabled: true,
            agents: Vec::new(),
            note: String::new(),
        },
    };

    if let Some(v) = a.name {
        server.name = v;
    }
    if let Some(v) = a.transport {
        server.transport = Transport::parse(&v);
    }
    if let Some(v) = a.command {
        server.command = v;
    }
    if !a.args.is_empty() {
        server.args = a.args;
    }
    if let Some(v) = a.cwd {
        server.cwd = v;
    }
    if let Some(v) = a.url {
        server.url = v;
    }
    if a.clear_env {
        server.env.clear();
    }
    for (k, v) in env_pairs {
        server.env.insert(k, v);
    }
    if a.clear_headers {
        server.headers.clear();
    }
    for (k, v) in header_pairs {
        server.headers.insert(k, v);
    }
    if a.disabled {
        server.enabled = false;
    }
    if !a.agents.is_empty() {
        // 和 matrix::set_row 同一条规则：只接受已知且支持 MCP 注入的 agent
        let known: Vec<&'static str> = crate::agents::mcp_capable()
            .iter()
            .map(|x| x.id())
            .collect();
        server.agents = a
            .agents
            .into_iter()
            .filter(|x| known.contains(&x.as_str()))
            .fold(Vec::new(), |mut acc, x| {
                if !acc.contains(&x) {
                    acc.push(x);
                }
                acc
            });
    }
    if let Some(v) = a.note {
        server.note = v;
    }

    if let Some(err) = server.validate() {
        eprintln!("{err}");
        return 1;
    }

    match existing {
        Some(i) => list[i] = server,
        None => list.push(server),
    }

    if let Err(e) = store::save(&list) {
        eprintln!("保存失败: {e}");
        return 1;
    }

    println!("{} MCP server {}", if is_new { "已新增" } else { "已更新" }, a.id);
    if let Some(warn) = store::check_gitignore() {
        println!("\n[警告] {warn}");
    }
    println!("跑 `agentcenter mcp diff` 看会发生什么，`agentcenter mcp sync` 生效。");
    0
}

/// 解析 `KEY=VALUE` / `KEY: VALUE` 形式的重复参数。
///
/// 只在**第一个**分隔符处切分，所以值里可以再包含分隔符
/// （URL、Base64 里的 `=`，header 值里的 `:` 都常见）。
fn parse_pairs(
    items: &[String],
    sep: &str,
    flag: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for item in items {
        let Some((k, v)) = item.split_once(sep) else {
            return Err(format!(
                "{flag} 格式应为 KEY{sep}VALUE，收到：{item}"
            ));
        };
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("{flag} 的键不能为空：{item}"));
        }
        out.push((k.to_string(), v.trim().to_string()));
    }
    Ok(out)
}

/// Claude Code 的首次引导开关。
///
/// 操作的是 `~/.claude.json` 根上的 `hasCompletedOnboarding`，
/// 和 provider 无关。
fn run_claude(cmd: crate::cli::ClaudeCmd) -> i32 {
    use crate::cli::ClaudeCmd;
    use crate::providers::claude_onboarding as ob;

    match cmd {
        ClaudeCmd::Status => {
            let status = match ob::status() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("读取状态失败: {e}");
                    return 1;
                }
            };
            println!("文件：{}", status.path);
            if !status.available {
                println!("Claude Code 未安装（{} 和 ~/.claude 都不存在）", status.path);
                return 0;
            }
            println!(
                "首次引导：{}",
                if status.skipped {
                    "已跳过（hasCompletedOnboarding = true）"
                } else {
                    "会显示（没有 hasCompletedOnboarding）"
                }
            );
            0
        }
        ClaudeCmd::Skip => match ob::set_skipped(true) {
            Ok((_, msg)) => {
                println!("{msg}");
                0
            }
            Err(e) => {
                eprintln!("写入失败: {e}");
                1
            }
        },
        ClaudeCmd::Restore => match ob::set_skipped(false) {
            Ok((_, msg)) => {
                println!("{msg}");
                0
            }
            Err(e) => {
                eprintln!("写入失败: {e}");
                1
            }
        },
    }
}

/// Cursor 自带密钥（BYOK）子命令。
///
/// Cursor 的配置在 SQLite 库里、密钥经 safeStorage 加密，不能像别的 agent
/// 那样走 `providers sync` 自动写。这一组命令是「半自动」的：
/// 读状态、算差异、然后在用户确认 Cursor 已退出之后才写。
fn run_cursor_cmd(cmd: crate::cli::CursorCmd) -> i32 {
    use crate::cli::CursorCmd;
    use crate::providers::cursor_db;

    match cmd {
        CursorCmd::Status => {
            let st = match cursor_db::read_state() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("读取 Cursor 配置失败: {e}");
                    return 1;
                }
            };

            if !st.installed {
                println!("没找到 Cursor 的配置库，可能没装过或没启动过。");
                println!("期望位置：{}", st.db_path);
                return 0;
            }

            println!("库文件：{}", st.db_path);
            println!();
            println!(
                "Use OpenAI API Key : {}",
                if st.use_openai_key {
                    "已打开"
                } else {
                    "未打开  ← 关着时自定义模型一定用不了"
                }
            );
            println!(
                "接口地址           : {}",
                st.base_url.as_deref().unwrap_or("（未设置，走 OpenAI 官方）")
            );
            println!(
                "密钥               : {}",
                if st.key_len == 0 {
                    "未存".to_string()
                } else {
                    format!("长度 {}，末尾 …{}", st.key_len, st.key_tail)
                }
            );
            println!(
                "自定义模型         : {}",
                if st.user_added_models.is_empty() {
                    "（无）".to_string()
                } else {
                    st.user_added_models.join(", ")
                }
            );
            println!(
                "\nCursor 进程        : {}",
                if cursor_db::is_running() {
                    "正在运行（写入前需要完全退出）"
                } else {
                    "未运行"
                }
            );
            0
        }

        CursorCmd::Plan { provider, models } => {
            let (base_url, key) = match load_provider_fields(&provider) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };

            let plan = match cursor_db::plan(&base_url, &key, &models) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("无法生成计划: {e}");
                    return 1;
                }
            };

            if plan.is_noop() {
                println!("Cursor 的配置已经和 {provider} 一致，不需要改动。");
                return 0;
            }

            println!("要让 {provider} 在 Cursor 里生效，需要做这些事：");
            for (i, line) in plan.summary.iter().enumerate() {
                println!("  {}. {line}", i + 1);
            }
            println!("\n这些改动会写进 Cursor 的 state.vscdb。执行前请完全退出 Cursor。");
            0
        }

        CursorCmd::Apply {
            provider,
            models,
            force,
        } => {
            let (base_url, key) = match load_provider_fields(&provider) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };

            let plan = match cursor_db::plan(&base_url, &key, &models) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("无法生成计划: {e}");
                    return 1;
                }
            };

            if plan.is_noop() {
                println!("Cursor 的配置已经和 {provider} 一致，无需改动。");
                return 0;
            }

            if cursor_db::is_running() && !force {
                eprintln!("Cursor 正在运行。它的内存状态会覆盖我们的写入，");
                eprintln!("并发写 4 GB 的 SQLite 还有损坏风险。");
                eprintln!();
                eprintln!("请先完全退出 Cursor（托盘图标也要退），然后重跑这条命令。");
                eprintln!("确实要强行写入（不推荐）：加 --force");
                return 2;
            }

            match cursor_db::apply(&plan, &key, force) {
                Ok(log) => {
                    println!("已写入 Cursor：");
                    for line in log {
                        println!("  - {line}");
                    }
                    println!("\n重启 Cursor 后生效。");
                    println!("在 Cursor 的模型选择器里选 {provider} 的模型发一句话即可验证。");
                    0
                }
                Err(e) => {
                    eprintln!("写入失败: {e}");
                    1
                }
            }
        }

        CursorCmd::RemoveModel { models, force } => {
            if models.is_empty() {
                eprintln!("要删哪个模型？例如：providers cursor remove-model deepseek/deepseek-v4.1-flash");
                return 2;
            }

            if cursor_db::is_running() && !force {
                eprintln!("Cursor 正在运行，写库会被它覆盖。请先完全退出 Cursor 再重试。");
                return 2;
            }

            // 只删模型，不动开关/地址/密钥 —— 删一个模型不该顺带重写接口配置
            let plan = cursor_db::plan_remove_models(&models);
            match cursor_db::apply(&plan, "", force) {
                Ok(log) => {
                    println!("已更新 Cursor 的模型选择器：");
                    for line in log {
                        println!("  - {line}");
                    }
                    println!("\n重启 Cursor 后生效。");
                    0
                }
                Err(e) => {
                    eprintln!("删除失败: {e}");
                    1
                }
            }
        }

        CursorCmd::Reset => {
            let plan = crate::providers::cursor_db::CursorPlan {
                set_use_openai_key: Some(false),
                set_base_url: Some(String::new()),
                write_key: false,
                add_models: Vec::new(),
                remove_models: Vec::new(),
                summary: vec!["关闭自带密钥，清空接口地址".into()],
            };

            if cursor_db::is_running() {
                eprintln!("Cursor 正在运行，请先完全退出再重置。");
                return 2;
            }

            match cursor_db::apply(&plan, "", false) {
                Ok(log) => {
                    println!("已重置 Cursor：");
                    for line in log {
                        println!("  - {line}");
                    }
                    println!("\n重启 Cursor 后回到官方模型。");
                    0
                }
                Err(e) => {
                    eprintln!("重置失败: {e}");
                    1
                }
            }
        }
    }
}

/// 从母版里取某个 provider 的接口地址和密钥，供 Cursor 命令用。
///
/// 单独抽出来是因为 `Status` / `Plan` / `Apply` 三个分支都要这两项，
/// 报错信息也只写一遍。
fn load_provider_fields(id: &str) -> anyhow::Result<(String, String)> {
    use crate::providers::store;

    let list = store::load_or_default()?;
    let Some(p) = list.iter().find(|p| p.id == id) else {
        anyhow::bail!("母版里没有 provider：{id}。用 `agentcenter providers list` 看现有的");
    };
    if p.base_url.trim().is_empty() {
        anyhow::bail!("provider {id} 没有 base_url，Cursor 需要它");
    }
    if p.api_key.trim().is_empty() {
        anyhow::bail!("provider {id} 没有 api_key，Cursor 需要它");
    }
    Ok((p.base_url.clone(), p.api_key.clone()))
}

/// provider 统一管理子命令。
fn run_providers(cmd: crate::cli::ProvidersCmd) -> i32 {
    use crate::cli::ProvidersCmd;
    use crate::providers::{diff, matrix, model, store};

    match cmd {
        ProvidersCmd::List => {
            let list = match store::load_or_default() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };
            if print_json(
                &list
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "id": p.id,
                            "name": p.name,
                            "baseUrl": p.base_url,
                            "kind": p.kind.as_str(),
                            "agentKinds": p
                                .agent_kinds
                                .iter()
                                .map(|(a, k)| (a.clone(), k.as_str().to_string()))
                                .collect::<std::collections::BTreeMap<_, _>>(),
                            "models": p.models,
                            "agents": p.agents,
                            "codexModel": p.codex_model,
                            "codexRequiresOpenaiAuth": p.codex_requires_openai_auth,
                            // 只给长度和末尾 4 位，和界面一致：密钥原文不出这个进程
                            "keyLen": p.api_key.len(),
                            "keyTail": p.api_key.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>(),
                            "note": p.note,
                        })
                    })
                    .collect::<Vec<_>>(),
            ) {
                return 0;
            }
            if list.is_empty() {
                println!("母版里还没有 provider。");
                println!("用 `agentcenter providers example` 看格式，或 `providers set` 添加。");
                return 0;
            }
            println!("母版：{}\n", store::path().display());
            for p in &list {
                println!("{}  ({})", p.id, p.name);
                println!("  base_url  {}", p.base_url);
                println!("  kind      {:?}", p.kind);
                let model_list = if p.models.is_empty() {
                    "-".to_string()
                } else {
                    p.models
                        .iter()
                        .map(|m| m.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                println!("  models    {model_list}");
                println!("  agents    {}", if p.agents.is_empty() { "-".into() } else { p.agents.join(", ") });
                if !p.note.is_empty() {
                    println!("  note      {}", p.note);
                }
                println!();
            }
            0
        }

        ProvidersCmd::Diff => {
            let report = match diff::run() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("对比失败: {e}");
                    return 1;
                }
            };

            if print_json(&report) {
                return 0;
            }

            if report.lines.is_empty() {
                println!("母版是空的。先定义 provider 再对比。");
                return 0;
            }

            println!("=== 差异报告（不写盘）===\n");
            let mut last_agent = String::new();
            for l in &report.lines {
                if l.agent != last_agent {
                    println!("[{}]", l.agent);
                    last_agent = l.agent.clone();
                }
                let mark = match l.action {
                    model::DiffAction::Add => "  + ",
                    model::DiffAction::Update => "  ~ ",
                    model::DiffAction::Remove => "  - ",
                    model::DiffAction::Unchanged => "  = ",
                    model::DiffAction::Conflict => "  ! ",
                };
                println!("{mark}{}  {}", l.target, l.detail);
            }

            if !report.conflicts.is_empty() {
                println!("\n=== 需要人工确认 ===\n");
                for c in &report.conflicts {
                    println!("  ! {c}");
                }
            }

            println!(
                "\n汇总：{} 个 agent 受影响，{} 处改动",
                report.agent_count, report.changed_files
            );
            0
        }

        ProvidersCmd::Sync { dry_run } => {
            let list = match store::load_or_default() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };

            if list.is_empty() {
                eprintln!("母版里没有 provider，无事可做。");
                return 1;
            }

            // 运行中的 agent 会在退出时用内存状态覆盖配置。只在真写时提醒
            if !dry_run {
                let running = crate::agents::running_agents();
                if !running.is_empty() {
                    eprintln!(
                        "[提醒] 这些 agent 正在运行，它们可能会用内存状态覆盖刚写入的配置：{}",
                        running.join(", ")
                    );
                    eprintln!("       建议先退出它们再同步。");
                }
            }

            match crate::providers::sync(&list, dry_run) {
                Ok(r) => {
                    if print_json(&serde_json::json!({
                        "dryRun": dry_run,
                        "ok": r.ok,
                        "written": r.written,
                        "skipped": r.skipped,
                        "backups": r.backups,
                        "log": r.log,
                    })) {
                        return if r.ok { 0 } else { 1 };
                    }
                    if dry_run {
                        println!("=== 预览（未写入）===\n");
                    }
                    for line in &r.log {
                        println!("{line}");
                    }
                    if !r.backups.is_empty() {
                        println!("\n备份：");
                        for b in &r.backups {
                            println!("  {b}");
                        }
                    }
                    if dry_run {
                        println!(
                            "\n将写入 {} 个文件，跳过 {} 个。去掉 --dry-run 才真正执行。",
                            r.written.len(),
                            r.skipped.len()
                        );
                    } else {
                        println!(
                            "\n完成：写入 {} 个文件，跳过 {} 个。",
                            r.written.len(),
                            r.skipped.len()
                        );
                    }
                    if r.ok {
                        0
                    } else {
                        1
                    }
                }
                Err(e) => {
                    eprintln!("同步失败: {e}");
                    1
                }
            }
        }

        ProvidersCmd::Set {
            id,
            name,
            base_url,
            api_key,
            kind,
            agent_kinds,
            models,
            agents,
            codex_model,
        } => {
            let mut list = match store::load_or_default() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };

            let is_new = !list.iter().any(|p| p.id == id);
            if let Some(existing) = list.iter_mut().find(|p| p.id == id) {
                // 更新：只覆盖显式提供的字段，没给的保持原值
                if let Some(v) = name {
                    existing.name = v;
                }
                if let Some(v) = base_url {
                    existing.base_url = v;
                }
                if let Some(v) = api_key {
                    existing.api_key = v;
                }
                if let Some(v) = kind {
                    existing.kind = model::ProviderKind::parse(&v);
                }
                // --agent-kind pi=openai-responses 这种，一次可以给多个
                for spec in &agent_kinds {
                    match spec.split_once('=') {
                        Some((agent, k)) => {
                            existing.agent_kinds.insert(
                                agent.trim().to_string(),
                                model::ProviderKind::parse(k.trim()),
                            );
                        }
                        None => {
                            eprintln!("--agent-kind 格式应为 <agent>=<协议>，收到：{spec}");
                            return 1;
                        }
                    }
                }
                if !models.is_empty() {
                    // CLI 只传 id 字符串，转成 ModelSpec（不带元数据）
                    existing.models = models
                        .iter()
                        .map(|m| model::ModelSpec::new(m.clone()))
                        .collect();
                }
                if !agents.is_empty() {
                    existing.agents = agents;
                }
                if let Some(m) = codex_model {
                    // 必须是这个 provider 自己的模型，否则写进去 Codex 会拿去
                    // 请求一个上游根本没有的名字
                    let m = m.trim().to_string();
                    if m.is_empty() {
                        existing.codex_model = None;
                    } else if existing.models.iter().any(|x| x.id == m) {
                        existing.codex_model = Some(m);
                    } else {
                        eprintln!("--codex-model {m} 不在这个 provider 的 --models 里");
                        return 1;
                    }
                }
            } else {
                // 新增：必填字段缺了就报错
                let (Some(n), Some(b), Some(k)) = (name, base_url, api_key) else {
                    eprintln!("新增 provider 必须提供 --name / --base-url / --api-key");
                    return 1;
                };
                // 新增时也支持 --agent-kind
                let mut kinds: std::collections::BTreeMap<
                    String,
                    model::ProviderKind,
                > = Default::default();
                for spec in &agent_kinds {
                    match spec.split_once('=') {
                        Some((agent, k)) => {
                            kinds.insert(
                                agent.trim().to_string(),
                                model::ProviderKind::parse(k.trim()),
                            );
                        }
                        None => {
                            eprintln!("--agent-kind 格式应为 <agent>=<协议>，收到：{spec}");
                            return 1;
                        }
                    }
                }

                let mut picked = None;
                if let Some(m) = codex_model {
                    let m = m.trim().to_string();
                    if !m.is_empty() {
                        if !models.iter().any(|x| *x == m) {
                            eprintln!("--codex-model {m} 不在这次传入的 --models 里");
                            return 1;
                        }
                        picked = Some(m);
                    }
                }

                list.push(model::Provider {
                    id: id.clone(),
                    name: n,
                    base_url: b,
                    kind: model::ProviderKind::parse(kind.as_deref().unwrap_or("anthropic")),
                    agent_kinds: kinds,
                    api_key: k,
                    models: models
                        .iter()
                        .map(|m| model::ModelSpec::new(m.clone()))
                        .collect(),
                    agents,
                    claude_model_map: model::ClaudeModelMap::default(),
                    // CLI 的 providers set 不碰这个开关；要用就写母版
                    // 的 codex_requires_openai_auth，或走界面上的 Codex 配置
                    codex_requires_openai_auth: None,
                    codex_model: picked,
                    note: String::new(),
                });
            }

            if let Err(e) = store::save(&list) {
                eprintln!("保存失败: {e}");
                return 1;
            }
            println!("{} provider {}", if is_new { "已新增" } else { "已更新" }, id);

            // 保存后检查 gitignore
            if let Some(warn) = store::check_gitignore() {
                println!("\n[警告] {warn}");
            }
            0
        }

        ProvidersCmd::Remove { id } => {
            let mut list = match store::load_or_default() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("读取母版失败: {e}");
                    return 1;
                }
            };
            let before = list.len();
            list.retain(|p| p.id != id);
            if list.len() == before {
                eprintln!("母版里没有 {id}");
                return 1;
            }
            if let Err(e) = store::save(&list) {
                eprintln!("保存失败: {e}");
                return 1;
            }
            println!("已从母版移除 {id}");
            0
        }

        ProvidersCmd::Matrix => {
            let m = match matrix::build() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("读取失败: {e}");
                    return 1;
                }
            };

            if print_json(&m) {
                return 0;
            }

            if m.providers.is_empty() {
                println!("母版里还没有 provider。");
                return 0;
            }

            // 表头：provider / 协议 / 每个 agent 一列
            let labels: Vec<&str> = m.columns.iter().map(|c| c.label.as_str()).collect();
            println!(
                "{:<20} {:<26} {}",
                "provider",
                "协议",
                labels.join("  ")
            );
            println!("{}", "-".repeat(20 + 1 + 26 + 1 + labels.len() * 8));

            for p in &m.providers {
                let marks: Vec<String> = m
                    .columns
                    .iter()
                    .map(|c| {
                        if !c.available {
                            "  - ".to_string()
                        } else if p.agents.iter().any(|a| a == &c.agent) {
                            "  ✓ ".to_string()
                        } else {
                            "    ".to_string()
                        }
                    })
                    .collect();
                println!("{:<20} {:<26} {}", p.id, p.kind_label, marks.join(""));
            }

            let available = m.columns.iter().filter(|c| c.available).count();
            println!("\n共 {} 个 provider，{} 个可用 agent。", m.providers.len(), available);
            0
        }

        ProvidersCmd::Assign { id, agents } => {
            match matrix::set_row(&id, &agents, None) {
                Ok(()) => {
                    println!(
                        "已设置 {id} 的分配：{}",
                        if agents.is_empty() {
                            "（无）".to_string()
                        } else {
                            agents.join(", ")
                        }
                    );
                    0
                }
                Err(e) => {
                    eprintln!("设置失败: {e}");
                    1
                }
            }
        }

        ProvidersCmd::Cursor { cmd } => run_cursor_cmd(cmd),

        ProvidersCmd::Example => {
            println!("{}", store::example_content());
            0
        }

        ProvidersCmd::Doctor => {
            let p = store::path();
            println!("母版路径  {}", p.display());
            println!("是否存在  {}", if p.exists() { "是" } else { "否" });
            match store::load_or_default() {
                Ok(l) => println!("provider  {}", l.len()),
                Err(e) => {
                    eprintln!("解析失败: {e}");
                    return 1;
                }
            }
            match store::check_gitignore() {
                Some(w) => {
                    println!("\n[警告] {w}");
                    2
                }
                None => {
                    println!("gitignore  已忽略");
                    0
                }
            }
        }
    }
}

/// API key 体检子命令。
fn run_creds(cmd: crate::cli::CredsCmd) -> i32 {
    use crate::cli::CredsCmd;

    match cmd {
        CredsCmd::Inspect => {
            let overview = crate::credentials::inspect();

            if print_json(&overview) {
                return 0;
            }

            println!(
                "共 {} 条凭证，{} 个不同 key\n",
                overview.total_entries, overview.unique_keys
            );

            for group in &overview.providers {
                if group.is_empty() {
                    continue;
                }
                let provider = &group[0].provider;

                // 同一 provider 下有几个不同指纹
                let mut fps: Vec<&str> = group.iter().map(|e| e.fingerprint.as_str()).collect();
                fps.dedup();
                let multi = fps.len() > 1;

                let tag = if multi { " [!] 多把 key" } else { "" };
                println!("{provider}{tag}");

                for e in group {
                    let dup = if e.duplicated { "  *重复" } else { "" };
                    println!(
                        "  {:<10} {:<8} {:<12} 长度{:<5} {}{}",
                        e.agent, e.kind, e.fingerprint, e.length, e.tail, dup
                    );
                    println!("             {}", e.location);
                    if !e.shared_with.is_empty() {
                        println!("             同一把 key 也在: {}", e.shared_with.join(", "));
                    }
                }
                println!();
            }

            if !overview.unsupported.is_empty() {
                println!("以下 agent 读不到明文 key：");
                for u in &overview.unsupported {
                    println!("  {:<12} {}", u.agent, u.reason);
                }
                println!();
            }

            println!(
                "汇总：{} 组重复 key，{} 个 provider 配了多把 key",
                overview.duplicated_keys, overview.providers_with_multiple_keys
            );
            0
        }
    }
}

/// `skills apply`：对一条散落 skill 执行选定处置。
///
/// 三种用法：
/// - `--dry-run`：只列每条散落 skill 可用的动作，不动任何东西
/// - `--path <路径> --action <动作>`：直接指定，最确定
/// - `<name> --action <动作>`：按名字找，重名时报错让人改用 `--path`
///
/// ## 为什么要重新扫描
///
/// 不信任调用方传进来的路径和先前 `scan` 的结果：`scan` 和 `apply` 之间
/// 用户完全可能在资源管理器里动过文件。所以每次都重新扫一遍，用最新结果
/// 校验目标还在不在、关系是什么。
///
/// ## 为什么校验关系和动作的搭配
///
/// 「对内容相同的副本选采用散落版」这类组合不会出错，但会白白备份一次，
/// 也说明调用方没看清扫描结果。报错比默默执行更有用。
fn run_skills_apply(
    tags: &crate::config::SkillTags,
    name: Option<String>,
    path: Option<String>,
    action: Option<String>,
    dry_run: bool,
) -> i32 {
    use crate::types::OrphanAction;

    // ---- 参数校验先行 ----
    //
    // 动作名拼错要在碰磁盘之前就报出来。放到扫描之后会导致「目标不存在」
    // 盖过「动作名不认识」这个更根本的问题，调用方会去查路径而不是改参数。

    let action = match action.as_deref() {
        Some(s) => match OrphanAction::parse(s) {
            Some(a) => Some(a),
            None => {
                eprintln!("不认识的动作：{s}");
                eprintln!(
                    "可用值：import / privatize / dedupe / take-orphan / take-shared / delete / remove-link"
                );
                return 1;
            }
        },
        None => None,
    };

    if !dry_run && action.is_none() {
        eprintln!("缺少 --action。");
        eprintln!(
            "可用值：import / privatize / dedupe / take-orphan / take-shared / delete / remove-link"
        );
        eprintln!("先跑 `agentcenter skills apply --dry-run` 看每条散落项各自能用哪个。");
        return 1;
    }

    if !dry_run && path.is_none() && name.is_none() {
        eprintln!(
            "要处置哪一条？用 --path <路径> 或 <name>。先跑 `agentcenter skills scan` 看清单。"
        );
        return 1;
    }

    let orphans = match crate::skills::scan_orphans(tags) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("扫描失败: {e}");
            return 1;
        }
    };

    // 干跑：列出全部散落项和各自可用的动作
    if dry_run {
        if orphans.is_empty() {
            println!("没有发现散落的 skill");
            return 0;
        }
        if print_json(&orphans) {
            return 0;
        }
        println!("散落 skill 及可用动作：\n");
        for o in &orphans {
            println!("{}  ({})", o.name, o.location);
            println!("  relation  {}", o.relation.as_str());
            println!("  path      {}", o.path);
            if !o.description.is_empty() {
                let desc: String = o.description.chars().take(80).collect();
                println!("  desc      {desc}");
            }
            let actions: Vec<&str> = available_actions(o.relation)
                .iter()
                .map(|a| a.as_str())
                .collect();
            println!("  可用动作  {}", actions.join(" / "));
            println!();
        }
        println!("执行示例：");
        println!("  agentcenter skills apply --path \"<上面的 path>\" --action import");
        return 0;
    }

    // 到这里 action 一定有值（前面已拦）
    let action = action.expect("action 已在参数校验阶段确认非空");

    // 定位目标：--path 优先，其次按 name
    let target = match (&path, &name) {
        (Some(p), _) => orphans.iter().find(|o| &o.path == p),
        (None, Some(n)) => {
            let hits: Vec<&crate::types::OrphanSkill> =
                orphans.iter().filter(|o| &o.name == n).collect();
            match hits.len() {
                0 => None,
                1 => Some(hits[0]),
                _ => {
                    // 同名散落在多个 agent 目录里，按名字分不清要处置哪一个
                    eprintln!("有 {} 条名为 {n} 的散落 skill，请用 --path 指定：", hits.len());
                    for h in hits {
                        eprintln!("  {}  ({})", h.path, h.location);
                    }
                    return 1;
                }
            }
        }
        (None, None) => unreachable!("已在参数校验阶段拦截"),
    };

    let Some(o) = target.cloned() else {
        eprintln!("扫描结果里没有这条。可能已经被处理过，或路径写错了。");
        eprintln!("跑 `agentcenter skills scan` 看当前清单。");
        return 1;
    };

    // 关系与动作的搭配校验
    let allowed = action.allowed_relations();
    if !allowed.is_empty() && !allowed.contains(&o.relation) {
        eprintln!(
            "{} 现在的关系是 {}，用不了 {} 这个动作。",
            o.name,
            o.relation.as_str(),
            action.as_str()
        );
        let actions: Vec<&str> = available_actions(o.relation)
            .iter()
            .map(|a| a.as_str())
            .collect();
        eprintln!("这个关系下可用：{}", actions.join(" / "));
        eprintln!("跑 `agentcenter skills apply --dry-run` 看全部散落项和各自可用的动作。");
        return 1;
    }

    let result = crate::skills::apply_action(&o.path, action);

    if print_json(&result) {
        return if result.ok { 0 } else { 1 };
    }

    if result.ok {
        println!("[{}] {}：{}", result.action.as_str(), result.name, result.message);
        0
    } else {
        eprintln!("[失败] {}：{}", result.name, result.message);
        1
    }
}

/// 某种关系下可用的动作。与界面上的按钮组合保持一致。
fn available_actions(relation: crate::types::OrphanRelation) -> Vec<crate::types::OrphanAction> {
    use crate::types::{OrphanAction::*, OrphanRelation::*};
    match relation {
        Unique => vec![Import, Privatize, Delete],
        Identical => vec![Dedupe, Delete],
        Conflict => vec![TakeOrphan, TakeShared, Delete],
        BrokenLink => vec![RemoveLink],
    }
}

/// skill 子命令。
fn run_skills(cmd: SkillsCmd) -> i32 {
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("读取配置失败: {e}");
            return 1;
        }
    };

    match cmd {
        SkillsCmd::List => {
            match skills::list_skills(&cfg.skills) {
                Ok(list) => {
                    if print_json(&list) {
                        return 0;
                    }
                    for s in list {
                        let managed = if s.managed { "managed" } else { "-" };
                        let tags = if s.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", s.tags.join(","))
                        };
                        println!(
                            "{:<28} {:<8} {:>4}f {:>8}B {}{}",
                            s.name, managed, s.file_count, s.size_bytes, s.hash, tags
                        );
                        // 描述独占一行。前面加四个空格做视觉缩进，
                        // 太长就截到 100 字符，完整内容在界面上看。
                        let desc: String = s.description.chars().take(100).collect();
                        let ellipsis = if s.description.chars().count() > 100 {
                            "…"
                        } else {
                            ""
                        };
                        println!("    {desc}{ellipsis}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("列出 skill 失败: {e}");
                    1
                }
            }
        }
        SkillsCmd::Scan => match skills::scan_orphans(&cfg.skills.tags) {
            Ok(orphans) => {
                if print_json(&orphans) {
                    return 0;
                }
                if orphans.is_empty() {
                    println!("没有发现散落的 skill");
                    return 0;
                }
                for o in orphans {
                    let rel = match o.relation {
                        crate::types::OrphanRelation::Unique => "unique",
                        crate::types::OrphanRelation::Identical => "identical",
                        crate::types::OrphanRelation::Conflict => "CONFLICT",
                        crate::types::OrphanRelation::BrokenLink => "broken",
                    };
                    println!("{:<12} {:<28} {}", rel, o.name, o.location);
                }
                0
            }
            Err(e) => {
                eprintln!("扫描失败: {e}");
                1
            }
        },
        SkillsCmd::Apply {
            name,
            path,
            action,
            dry_run,
        } => run_skills_apply(&cfg.skills.tags, name, path, action, dry_run),
        SkillsCmd::Collect { apply } => {
            let orphans = match skills::scan_orphans(&cfg.skills.tags) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("扫描失败: {e}");
                    return 1;
                }
            };
            let plan = skills::build_plan(orphans);

            if print_json(&plan) {
                // `--json` 只报告计划，不执行。要执行必须不带 --json 加 --apply，
                // 避免调用方以为「看到 JSON 就已经做了」
                return 0;
            }

            println!("移入 {} 个", plan.to_import.len());
            for o in &plan.to_import {
                println!("  + {} ({})", o.name, o.location);
            }
            println!("去重 {} 个", plan.to_dedupe.len());
            for o in &plan.to_dedupe {
                println!("  = {} ({})", o.name, o.location);
            }
            if !plan.conflicts.is_empty() {
                println!("冲突 {} 个（需人工决定）", plan.conflicts.len());
                for o in &plan.conflicts {
                    println!("  ! {} ({})", o.name, o.location);
                }
            }
            if !plan.broken.is_empty() {
                println!("坏链接 {} 个", plan.broken.len());
                for o in &plan.broken {
                    println!("  x {} ({})", o.name, o.location);
                }
            }

            if !apply {
                println!("\n这是预览。加 --apply 才会真正执行。");
                return 0;
            }

            match skills::apply_plan(&plan) {
                Ok(log) => {
                    for line in log {
                        println!("{line}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("执行失败: {e}");
                    1
                }
            }
        }
        SkillsCmd::Private { cmd } => run_skills_private(cmd),
        SkillsCmd::Tag { name, add } => {
            let mut cfg = Config::load().unwrap_or_default();
            if add.is_empty() {
                cfg.skills.tags.remove(&name);
                println!("已清空 {name} 的标签");
            } else {
                cfg.skills.tags.insert(name.clone(), add.clone());
                println!("{} 的标签：{}", name, add.join(", "));
            }
            match cfg.save() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    1
                }
            }
        }
        SkillsCmd::Check => {
            let r = skillscli::check_updates();
            println!("{}", r.output);
            // 退出码 1 表示有更新，原样传递
            r.exit_code
        }
        SkillsCmd::Update { names } => {
            let r = skillscli::update(&names);
            println!("{}", r.output);
            if r.ok {
                0
            } else {
                r.exit_code
            }
        }
    }
}

/// 专用 skill 子命令。
///
/// 和 `skills apply --action privatize` 的关系：那个从散落扫描结果里挑一条就地
/// 私有化，这个可以直接指定 skill 名和多个 agent。两者最终都调
/// `skills::privatize_one`，只是取本体的路径来源不同。
fn run_skills_private(cmd: crate::cli::SkillsPrivateCmd) -> i32 {
    use crate::cli::SkillsPrivateCmd;

    match cmd {
        SkillsPrivateCmd::List => {
            let cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("读取配置失败: {e}");
                    return 1;
                }
            };
            let views: Vec<crate::types::SkillMappingView> = cfg
                .skills
                .mappings
                .iter()
                .map(skills::inspect_private)
                .collect();

            if print_json(&views) {
                return 0;
            }
            if views.is_empty() {
                println!("没有专用 skill。");
                println!("用 `agentcenter skills private add <name> --agent <agent>` 标记一个。");
                return 0;
            }

            for v in &views {
                let state = match v.state {
                    crate::types::PrivateLinkState::Ok => "正常",
                    crate::types::PrivateLinkState::Broken => "断链",
                    crate::types::PrivateLinkState::Missing => "缺失",
                };
                println!("{}  [{}]  {}", v.skill, state, v.agents.join(","));
                println!("  本体  {}", v.store_path);
                for l in &v.links {
                    let ls = match l.state {
                        crate::types::PrivateLinkState::Ok => "ok",
                        crate::types::PrivateLinkState::Broken => "broken",
                        crate::types::PrivateLinkState::Missing => "missing",
                    };
                    let note = l
                        .note
                        .as_ref()
                        .map(|n| format!("  ({n})"))
                        .unwrap_or_default();
                    println!("  {:<10} {}  {}{note}", l.agent, l.path, ls);
                }
            }
            0
        }

        SkillsPrivateCmd::Add { skill, agents } => {
            // 校验 agent id：拼错的话目标位置算不出来，建完链接 agent 也读不到
            let mut bad = Vec::new();
            for a in &agents {
                if crate::agents::find(a).is_none() {
                    bad.push(a.clone());
                }
            }
            if !bad.is_empty() {
                eprintln!("不认识的 agent：{}", bad.join(", "));
                eprintln!("可用值：{}", crate::agents::ids().join(" / "));
                return 1;
            }

            // 找本体：优先 agent 自己的 skills 目录，其次公共库
            let mut found: Option<std::path::PathBuf> = None;
            for a in &agents {
                if let Some(agent) = crate::agents::find(a) {
                    let candidate = agent.skills_dir().join(&skill);
                    if candidate.join("SKILL.md").exists() && !link::is_link(&candidate) {
                        found = Some(candidate);
                        break;
                    }
                }
            }
            if found.is_none() {
                let candidate = skills::shared_skills_dir().join(&skill);
                if candidate.join("SKILL.md").exists() {
                    found = Some(candidate);
                }
            }
            let Some(src) = found else {
                eprintln!("找不到 {skill} 的目录。");
                eprintln!("在下列位置都没找到含 SKILL.md 的目录：");
                for a in &agents {
                    if let Some(agent) = crate::agents::find(a) {
                        eprintln!("  {}", agent.skills_dir().join(&skill).display());
                    }
                }
                eprintln!("  {}", skills::shared_skills_dir().join(&skill).display());
                eprintln!("\n先用 `agentcenter skills scan` 看它现在在哪。");
                return 1;
            };

            // 第一个 agent 决定本体落在哪个专用库
            let primary = &agents[0];
            let r = match skills::privatize_one(&src, primary) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("标记专用失败：{e}");
                    return 1;
                }
            };
            println!("{r}");

            // 其余 agent：写进同一条记录，然后跑一次 sync 把链接建全
            if agents.len() > 1 {
                let mut cfg = match Config::load() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("读取配置失败: {e}");
                        return 1;
                    }
                };
                if let Some(m) = cfg.skills.mappings.iter_mut().find(|m| m.skill == skill) {
                    m.agents = agents.clone();
                    m.agents.dedup();
                }
                if let Err(e) = cfg.save() {
                    eprintln!("写配置失败: {e}");
                    return 1;
                }
                match skills::sync_private_links(false) {
                    Ok(log) => {
                        for line in log {
                            if line.contains(&skill) {
                                println!("{line}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("建链接失败：{e}");
                        return 1;
                    }
                }
            }

            0
        }

        SkillsPrivateCmd::Sync { dry_run } => match skills::sync_private_links(dry_run) {
            Ok(log) => {
                if print_json(&log) {
                    return 0;
                }
                for line in log {
                    println!("{line}");
                }
                0
            }
            Err(e) => {
                eprintln!("同步失败: {e}");
                1
            }
        },

        SkillsPrivateCmd::Remove { skill, to_shared } => {
            match skills::remove_private(&skill, to_shared) {
                Ok(msg) => {
                    println!("{msg}");
                    0
                }
                Err(e) => {
                    eprintln!("取消失败：{e}");
                    1
                }
            }
        }
    }
}

/// token 用量统计子命令。
///
/// 输出保持 ASCII 安全（不用表格框线），避免 Windows 终端 GBK 代码页下花屏。
/// token 数按千分位分隔，几十亿的数字才看得清位数。
fn run_usage(cmd: crate::cli::UsageCmd) -> i32 {
    use crate::cli::UsageCmd;

    match cmd {
        UsageCmd::Status => {
            let report = crate::usage::collect_report(0, false);
            if print_json(&report.agents) {
                return 0;
            }
            println!("{:<10} {:<12} {:<8} {:<8} {}", "AGENT", "SUPPORTED", "ENABLED", "SOURCE?", "DATA");
            for a in &report.agents {
                // 不支持的 agent 也要列出来，否则用户会以为这里漏了
                if !a.supported {
                    println!("{:<10} {:<12} {:<8} {:<8} {}", a.agent, "no", "-", "-", "暂不支持");
                    continue;
                }
                let state = if !a.enabled {
                    "未启用（用 usage enable 打开）"
                } else if !a.error.is_empty() {
                    a.error.as_str()
                } else if !a.available {
                    "数据源不存在"
                } else {
                    "ok"
                };
                println!(
                    "{:<10} {:<12} {:<8} {:<8} {}",
                    a.agent,
                    "yes",
                    if a.enabled { "yes" } else { "no" },
                    if a.available { "yes" } else { "no" },
                    state
                );
                if a.enabled && !a.source.is_empty() {
                    println!("{:<10} {:<12} {:<8} {:<8} {}", "", "", "", "", a.source);
                    println!("{:<10} {:<12} {:<8} {:<8} {} 条记录", "", "", "", "", a.records);
                }
            }
            0
        }

        UsageCmd::Report { days, refresh } => {
            let report = crate::usage::collect_report(days, refresh);

            if print_json(&report) {
                return 0;
            }

            let enabled: Vec<_> = report.agents.iter().filter(|a| a.enabled).collect();

            if enabled.is_empty() {
                println!("还没有启用任何 agent 的用量统计。");
                println!("先跑 `agentcenter usage enable zcode`（pi / codex / claude 同理）。");
                return 0;
            }

            // 只读失败的 agent 单独提示，不混进统计表
            for a in &enabled {
                if !a.error.is_empty() {
                    println!("[失败] {}: {}", a.label, a.error);
                } else if !a.available {
                    println!("[跳过] {}: 数据源不存在（{}）", a.label, a.source);
                }
            }

            let scope = if days == 0 {
                "全部历史".to_string()
            } else {
                format!("最近 {days} 天")
            };

            if report.rows.is_empty() {
                println!("\n{scope}内没有用量记录。");
                return 0;
            }

            println!("\n=== token 用量（{scope}）===\n");
            println!(
                "{:<12} {:<10} {:<40} {:>16} {:>12} {:>8}",
                "DATE", "AGENT", "MODEL", "INPUT", "OUTPUT", "CALLS"
            );

            let (mut t_in, mut t_out, mut t_calls) = (0u64, 0u64, 0u64);
            for r in &report.rows {
                t_in += r.input;
                t_out += r.output;
                t_calls += r.calls;
                // 模型名可能很长，截到 40 字符保持列对齐
                let model: String = if r.model.chars().count() > 40 {
                    r.model.chars().take(39).collect::<String>() + "~"
                } else {
                    r.model.clone()
                };
                println!(
                    "{:<12} {:<10} {:<40} {:>16} {:>12} {:>8}",
                    r.date,
                    r.agent,
                    model,
                    thousands(r.input),
                    thousands(r.output),
                    r.calls
                );
            }

            println!(
                "\n合计：input {} / output {} / {} 次调用",
                thousands(t_in),
                thousands(t_out),
                t_calls
            );
            println!("口径：input 已折算为含缓存命中，output 不含推理细分。");
            0
        }

        UsageCmd::Enable { agent } => match crate::usage::set_enabled(&agent, true) {
            Ok(_) => {
                println!("{agent} 已纳入用量统计");
                0
            }
            Err(e) => {
                eprintln!("启用失败: {e}");
                1
            }
        },

        UsageCmd::Disable { agent } => match crate::usage::set_enabled(&agent, false) {
            Ok(_) => {
                println!("{agent} 已从用量统计中移除");
                0
            }
            Err(e) => {
                eprintln!("关闭失败: {e}");
                1
            }
        },
    }
}

/// 给数字加千分位。`6103448793` → `6,103,448,793`。
///
/// 手动实现而不引依赖：只需要这一处，加个库不值得。
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        // 除首位外，每 3 位插一个逗号
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// git 子命令。
fn run_git(cmd: GitCmd) -> i32 {
    match cmd {
        GitCmd::Status => {
            let s = git::status();
            if !s.is_repo {
                println!("~/.agents 不是 git 仓库");
                return 1;
            }
            println!("分支 {}  up {} / down {}", s.branch, s.ahead, s.behind);
            println!("远端 {}", if s.remote.is_empty() { "-" } else { &s.remote });
            if s.changed.is_empty() {
                println!("工作区干净");
            } else {
                println!("\n改动 {} 个文件：", s.changed.len());
                for c in &s.changed {
                    println!("  {:<3} {}", c.status, c.path);
                }
            }
            0
        }
        GitCmd::Commit { paths, message } => {
            let r = git::commit(&paths, &message);
            println!("{}", r.output);
            if r.ok {
                0
            } else {
                r.exit_code
            }
        }
        GitCmd::Push => {
            let r = git::push();
            println!("{}", r.output);
            if r.ok {
                0
            } else {
                r.exit_code
            }
        }
    }
}
