//! provider 统一管理与注入。
//!
//! 模块划分：
//!
//! - [`model`] —— 数据结构（Provider / FilePlan / Diff）
//! - [`store`] —— 母版 `providers.toml` 的读写
//! - 各 agent 的注入器（后续阶段逐个加入）
//!
//! 核心抽象是 [`Injector`]：每个 agent 实现它，把统一的 provider 列表
//! 翻译成该 agent 的配置文件形态。**plan 阶段只读**，产出 [`FilePlan`]，
//! 真正的写入由 [`safe_write`] 统一负责。

pub mod backfill;
pub mod claude;
pub mod claude_bridge;
pub mod claude_onboarding;
pub mod codex;
pub mod cursor_db;
pub mod diff;
pub mod dsh;
pub mod gemini;
pub mod grok;
pub mod hermes;
pub mod kimi;
pub mod matrix;
pub mod minimax;
pub mod model;
pub mod openclaw;
pub mod opencode;
pub mod pi;
pub mod safe_write;
pub mod scan;
pub mod store;
pub mod workbuddy;
pub mod zcode;

use model::{DiffLine, FilePlan, Provider};
use std::path::PathBuf;

use crate::config;

/// 一个 agent 的配置注入器。
///
/// 实现者只需要关心"我的配置应该长什么样"，不需要关心备份、
/// hash 复查、原子写这些安全机制 —— 那些在 safe_write 里统一处理。
pub trait Injector {
    /// agent 标识，与母版 `agents` 字段里的值对应
    fn agent(&self) -> &'static str;

    /// 这个 agent 涉及的所有文件。
    ///
    /// 有些 agent 要改两个文件（如 Pi 的 auth.json + models.json），
    /// 所以返回列表而不是单值。
    fn targets(&self) -> Vec<PathBuf>;

    /// 计算这个 agent 的目标文件应该变成什么。
    ///
    /// 只读，不写盘。返回的 [`FilePlan`] 里带着原始内容的 hash，
    /// 供执行阶段做复查。
    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>>;

    /// 生成面向人的差异描述。默认实现从 plan 结果推导。
    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        for p in plans {
            if p.changed {
                out.push(DiffLine {
                    agent: p.agent.clone(),
                    path: p.path.clone(),
                    action: model::DiffAction::Update,
                    target: p.agent.clone(),
                    detail: p.summary.join("；"),
                });
            }
        }
        Ok(out)
    }
}

/// 返回当前阶段已实现的所有注入器。
///
/// 随着阶段推进逐个加入。还没实现的 agent 不在这里，
/// 界面上会显示为"暂不支持注入"。
pub fn all_injectors() -> Vec<Box<dyn Injector>> {
    vec![
        Box::new(zcode::ZCodeInjector),
        Box::new(pi::PiInjector),
        Box::new(opencode::OpencodeInjector),
        Box::new(codex::CodexInjector),
        Box::new(workbuddy::WorkBuddyInjector),
        Box::new(dsh::DshInjector),
        Box::new(claude::ClaudeInjector),
        Box::new(hermes::HermesInjector),
        Box::new(gemini::GeminiInjector),
        Box::new(grok::GrokInjector),
        Box::new(openclaw::OpenClawInjector),
        Box::new(kimi::KimiInjector),
        Box::new(minimax::MinimaxInjector),
    ]
}

/// 执行同步：先全部 plan，任何一个失败就中止，全部成功才写。
///
/// 遍历 `agents::all()` 里声明了 provider 能力的那些。
/// 这个「先全算后全写」的顺序很重要 —— 如果边算边写，
/// 第三个文件解析失败时前两个已经被改了，会留下半同步的状态。
pub fn sync(providers: &[Provider], dry_run: bool) -> anyhow::Result<model::SyncReport> {
    let mut report = model::SyncReport {
        ok: true,
        written: Vec::new(),
        skipped: Vec::new(),
        backups: Vec::new(),
        log: Vec::new(),
    };

    // 第一遍：所有 agent 都 plan 一遍，收集结果
    let mut all_plans: Vec<(String, Vec<model::FilePlan>)> = Vec::new();
    for agent in crate::agents::provider_capable() {
        let id = agent.id().to_string();
        match agent.provider_plan(providers) {
            Ok(plans) => {
                if !plans.is_empty() {
                    all_plans.push((id, plans));
                }
            }
            Err(e) => {
                // 任何一个 agent 规划失败，整次同步中止
                anyhow::bail!("{} 规划失败，已中止（未写入任何文件）：{e}", id);
            }
        }
    }

    // 第二遍：全部写
    for (agent, plans) in &all_plans {
        for plan in plans {
            if !plan.changed {
                report.skipped.push(plan.path.clone());
                report
                    .log
                    .push(format!("[跳过] {agent}: {} 无变化", plan.path));
                continue;
            }

            if dry_run {
                report.written.push(plan.path.clone());
                report
                    .log
                    .push(format!("[预览] {agent}: 将写入 {}", plan.path));
                for s in &plan.summary {
                    report.log.push(format!("         {s}"));
                }
                continue;
            }

            // 真实写入走 safe_write（含 hash 复查、备份、原子写）
            let real = config::expand_tilde(&plan.path);
            match safe_write::safe_write(&real, &plan.content, &plan.pre_hash) {
                Ok(backup) => {
                    report.written.push(plan.path.clone());
                    if let Some(b) = backup {
                        report.backups.push(crate::config::contract_tilde(&b));
                    }
                    report.log.push(format!("[写入] {agent}: {}", plan.path));
                    for s in &plan.summary {
                        report.log.push(format!("       {s}"));
                    }
                }
                Err(e) => {
                    report.ok = false;
                    report.log.push(format!("[失败] {agent}: {e}"));
                    // 一个失败就停，避免继续改更多文件
                    return Ok(report);
                }
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod matrix_ser_test;

#[cfg(test)]
mod injector_test;

