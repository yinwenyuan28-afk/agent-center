//! 从各 agent 现有配置回填模型元数据。
//!
//! ## 为什么需要这个
//!
//! 母版里可以只写模型 id（存量文件全是这样）。但注入时母版没填的字段
//! 会走内置默认值（1M 上下文 / 128K 输出），而真实配置里这些值往往不同 ——
//! Pi 里 wangxin 是 32768/8192，opencode-go 是 1048576/131072。
//!
//! 这个模块把各 agent 配置里**已有的真实元数据**读回来写进母版，
//! 让母版成为完整的能力记录，而不是一堆光秃秃的 id。
//!
//! ## 只填空字段，不覆盖已有值
//!
//! 用户手填的值优先级最高。回填只补母版里是 None / 空的那个字段，
//! 已有的一个都不动。这样反复跑也不会把用户的选择冲掉。

use crate::agents;
use crate::providers::model::{ModelSpec, Provider};
use crate::providers::store;
use crate::types::{BackfillLine, BackfillReport, KindMismatch};

/// 把母版里是空的模型字段，用各 agent 配置里的真实值补上。
///
/// `dry_run` 为 true 时只算报告，不写母版。
pub fn run(dry_run: bool) -> anyhow::Result<BackfillReport> {
    let mut providers = store::load_or_default()?;
    let scans = agents::all()
        .iter()
        .filter_map(|a| a.provider_scan().map(|s| (a.label().to_string(), s)))
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    let mut skipped_providers = Vec::new();
    let mut kind_mismatches = Vec::new();
    let mut filled_fields = 0usize;

    for scan in &scans {
        let (agent_label, agent_scan) = scan;
        for found in &agent_scan.providers {
            // 按 base_url 把扫描结果对回母版 —— provider id 在各家不统一，
            // 端点才是稳定的身份
            let Some(target) = providers
                .iter_mut()
                .find(|p| normalize_url(&p.base_url) == normalize_url(&found.base_url))
            else {
                if !found.base_url.is_empty() {
                    let tag = format!("{} (来自 {})", found.name, agent_label);
                    if !skipped_providers.contains(&tag) {
                        skipped_providers.push(tag);
                    }
                }
                continue;
            };

            for src in &found.models {
                let Some(dst) = target
                    .models
                    .iter_mut()
                    .find(|m| m.id == src.id)
                else {
                    // 母版里没这个模型：回填是「补元数据」不是「加模型」，
                    // 所以不新增，只报告
                    continue;
                };

                let filled = fill_from(dst, src);
                if !filled.is_empty() {
                    filled_fields += filled.len();
                    lines.push(BackfillLine {
                        provider: target.id.clone(),
                        model: src.id.clone(),
                        source: agent_label.clone(),
                        filled,
                    });
                }
            }

            // 协议：这里**不回填**，只报告不一致。
            //
            // 原因：同一个端点在不同 agent 里协议本来就可能不同，扫描到的
            // 那个值只对这个 agent 成立，不能拿来覆盖 provider 级默认值。
            // 但用户需要知道这件事 —— 母版写 anthropic 而 ZCode 里实际是
            // openai-compatible 时，注入会按母版改回去，可能把能用的配置搞坏。
            if let (Some(actual), agent_id) = (found.api_kind, &agent_scan.agent) {
                let expected = target.kind_for(agent_id);
                if actual != expected {
                    kind_mismatches.push(KindMismatch {
                        provider: target.id.clone(),
                        agent: agent_id.clone(),
                        master: expected.as_str().to_string(),
                        actual: actual.as_str().to_string(),
                    });
                }
            }
        }
    }

    if !dry_run && filled_fields > 0 {
        store::save(&providers)?;
    }

    Ok(BackfillReport {
        filled_fields,
        lines,
        skipped_providers,
        kind_mismatches,
        wrote: !dry_run && filled_fields > 0,
    })
}

/// 把 `src` 里有值、`dst` 里为空的字段补上。返回补了哪些字段。
///
/// 方向是单向的：只用 src 补 dst，绝不反过来覆盖 dst 已有的值。
fn fill_from(dst: &mut ModelSpec, src: &ModelSpec) -> Vec<String> {
    let mut filled = Vec::new();

    if dst.name.is_none() && src.name.is_some() {
        dst.name = src.name.clone();
        filled.push("name".to_string());
    }
    if dst.context_window.is_none() && src.context_window.is_some() {
        dst.context_window = src.context_window;
        filled.push(format!(
            "context_window={}",
            src.context_window.unwrap()
        ));
    }
    if dst.max_output_tokens.is_none() && src.max_output_tokens.is_some() {
        dst.max_output_tokens = src.max_output_tokens;
        filled.push(format!(
            "max_output_tokens={}",
            src.max_output_tokens.unwrap()
        ));
    }
    if dst.input_modalities.is_empty() && !src.input_modalities.is_empty() {
        dst.input_modalities = src.input_modalities.clone();
        filled.push(format!("input=[{}]", src.input_modalities.join(",")));
    }
    if dst.output_modalities.is_empty() && !src.output_modalities.is_empty() {
        dst.output_modalities = src.output_modalities.clone();
        filled.push(format!("output=[{}]", src.output_modalities.join(",")));
    }
    if dst.reasoning.is_none() && src.reasoning.is_some() {
        dst.reasoning = src.reasoning;
        filled.push(format!("reasoning={}", src.reasoning.unwrap()));
    }
    if dst.reasoning_efforts.is_empty() && !src.reasoning_efforts.is_empty() {
        dst.reasoning_efforts = src.reasoning_efforts.clone();
        filled.push(format!(
            "reasoning_efforts=[{}]",
            src.reasoning_efforts.join(",")
        ));
    }
    if dst.default_effort.is_none() && src.default_effort.is_some() {
        dst.default_effort = src.default_effort.clone();
        filled.push(format!(
            "default_effort={}",
            src.default_effort.as_deref().unwrap()
        ));
    }
    if dst.tool_call.is_none() && src.tool_call.is_some() {
        dst.tool_call = src.tool_call;
        filled.push(format!("tool_call={}", src.tool_call.unwrap()));
    }
    if dst.cost.is_none() && src.cost.is_some() {
        dst.cost = src.cost.clone();
        filled.push("cost".to_string());
    }

    filled
}

/// 归一化 URL 用于比对。忽略协议差异和尾部斜杠。
fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

/// 给测试用：直接对一份 provider 列表回填，不碰磁盘。
pub fn backfill_providers(
    providers: &mut Vec<Provider>,
    scans: &[(String, crate::providers::scan::AgentScan)],
) -> (usize, Vec<BackfillLine>) {
    let mut lines = Vec::new();
    let mut filled_fields = 0usize;

    for (agent_label, agent_scan) in scans {
        for found in &agent_scan.providers {
            let Some(target) = providers
                .iter_mut()
                .find(|p| normalize_url(&p.base_url) == normalize_url(&found.base_url))
            else {
                continue;
            };
            for src in &found.models {
                let Some(dst) = target.models.iter_mut().find(|m| m.id == src.id) else {
                    continue;
                };
                let filled = fill_from(dst, src);
                if !filled.is_empty() {
                    filled_fields += filled.len();
                    lines.push(BackfillLine {
                        provider: target.id.clone(),
                        model: src.id.clone(),
                        source: agent_label.clone(),
                        filled,
                    });
                }
            }
        }
    }

    (filled_fields, lines)
}
