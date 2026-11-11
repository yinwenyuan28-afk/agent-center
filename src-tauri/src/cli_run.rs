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
                if !matches!(m.health, crate::types::HealthState::Ok) && m.installed {
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
        Command::Skills(sub) => run_skills(sub),
        Command::Creds(sub) => run_creds(sub),
        Command::Providers(sub) => run_providers(sub),
        Command::Mcp(sub) => run_mcp(sub),
        Command::Agents => run_agents(),
        Command::Models(sub) => run_models(sub),
        Command::Git(sub) => run_git(sub),
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
            match matrix::set_row(&id, &agents) {
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
