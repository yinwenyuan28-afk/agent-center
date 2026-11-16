//! CLI 子命令的实际执行，输出面向终端的纯文本。
//!
//! 与 IPC 命令层的区别：这里不返回结构化数据，直接把结果打印出来，
//! 便于脚本调用和人工排查。所有输出保持 ASCII 安全，避免 Windows
//! 终端在 GBK 代码页下花屏。

use crate::cli::{Command, GitCmd, SkillsCmd};
use crate::config::Config;
use crate::{git, link, skills, skillscli};

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
            for spec in cfg.resolved_mappings() {
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
            for spec in cfg.resolved_mappings() {
                let m = link::inspect(&spec);
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
                if !matches!(m.health, crate::types::HealthState::Ok) && m.installed && !m.disabled {
                    problems += 1;
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
            let mut failed = 0;
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
        Command::Skills(sub) => run_skills(sub),
        Command::Creds(sub) => run_creds(sub),
        Command::Providers(sub) => run_providers(sub),
        Command::Claude(sub) => run_claude(sub),
        Command::Mcp(sub) => run_mcp(sub),
        Command::Usage(sub) => run_usage(sub),
        Command::Agents => run_agents(),
        Command::Models(sub) => run_models(sub),
        Command::Git(sub) => run_git(sub),
    }
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

            match crate::mcp::sync_run(&list, dry_run) {
                Ok(r) => {
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

        McpCmd::Example => {
            println!("{}", store::example_content());
            0
        }
    }
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

            match crate::providers::sync(&list, dry_run) {
                Ok(r) => {
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
                            "\n将写入 {} 个文件，跳过 {} 个。加 --no-dry-run 才真正执行。",
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
            match skills::list_skills(&cfg.skills.tags) {
                Ok(list) => {
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
        SkillsCmd::Collect { apply } => {
            let orphans = match skills::scan_orphans(&cfg.skills.tags) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("扫描失败: {e}");
                    return 1;
                }
            };
            let plan = skills::build_plan(orphans);

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

/// token 用量统计子命令。
///
/// 输出保持 ASCII 安全（不用表格框线），避免 Windows 终端 GBK 代码页下花屏。
/// token 数按千分位分隔，几十亿的数字才看得清位数。
fn run_usage(cmd: crate::cli::UsageCmd) -> i32 {
    use crate::cli::UsageCmd;

    match cmd {
        UsageCmd::Status => {
            let report = crate::usage::collect_report(0, false);
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
