//! 母版与各 agent 现有配置的差异对比。
//!
//! **完全只读**。这是阶段 2 的核心：在真正写任何文件之前，
//! 让用户先看到"同步会发生什么"。
//!
//! 比对维度：
//! - 母版有、agent 没有 → 要新增
//! -两边都有但 key 指纹不同 → 要更新
//! - agent 有、母版没有 → 候选删除（但要小心：可能是用户手动加的）
//! - 同一 provider 在多个 agent 里用了不同 key → 冲突，需要人工确认

use super::model::{DiffAction, DiffLine, DiffReport, Provider};
use super::scan::{self, AgentScan, FoundProvider};
use std::collections::BTreeSet;

/// 一个 provider 的母版意图 vs 各 agent 现状汇总。
#[derive(Debug, Clone)]
struct ProviderDiff {
    /// 母版里这个 provider 要注入到哪些 agent
    intended_agents: Vec<String>,
    /// 各个 agent 现在的状态
    found: Vec<(String, Option<FoundProvider>)>,
}

/// 执行完整对比，返回报告。
///
/// `master` 是母版里的 provider 列表，`scans` 是各 agent 的扫描结果。
/// 分成两个参数是为了让测试能注入伪造数据。
pub fn compare(master: &[Provider], scans: &[AgentScan]) -> DiffReport {
    let mut lines = Vec::new();
    let mut conflicts = Vec::new();
    let mut changed_files = 0usize;

    // 按 provider id 聚合
    let mut by_id: std::collections::BTreeMap<String, ProviderDiff> =
        std::collections::BTreeMap::new();

    for p in master {
        by_id.insert(
            p.id.clone(),
            ProviderDiff {
                intended_agents: p.agents.clone(),
                found: Vec::new(),
            },
        );
    }

    // 把每个 agent 扫到的 provider 塞进去。
    // 用 base_url 做匹配键 —— provider id 在各家不统一（ZCode 用 UUID），
    // 但 base_url 是稳定的。
    for s in scans {
        for f in &s.providers {
            // 先按 base_url 找母版里的对应项
            let master_provider = master.iter().find(|p| {
                normalize_url(&p.base_url) == normalize_url(&f.base_url) && !f.base_url.is_empty()
            });

            let key = match master_provider {
                Some(mp) => mp.id.clone(),
                // 母版里没有这个 base_url，说明是用户手动配的
                None => format!("__unmanaged__{}", f.base_url),
            };

            let entry = by_id.entry(key.clone()).or_insert_with(|| ProviderDiff {
                intended_agents: Vec::new(),
                found: Vec::new(),
            });
            entry.found.push((s.agent.clone(), Some(f.clone())));
        }
    }

    // 逐 provider 生成差异
    let master_by_id: std::collections::BTreeMap<&str, &Provider> =
        master.iter().map(|p| (p.id.as_str(), p)).collect();

    for (id, d) in &by_id {
        // 用户手动配的（母版没有），单独报
        if id.starts_with("__unmanaged__") {
            for (agent, found) in &d.found {
                if let Some(f) = found {
                    lines.push(DiffLine {
                        agent: agent.clone(),
                        path: f.location.clone(),
                        action: DiffAction::Unchanged,
                        target: f.id.clone(),
                        detail: format!("「{}」不在母版中，保持原样不动", f.name),
                    });
                }
            }
            continue;
        }

        let Some(mp) = master_by_id.get(id.as_str()) else {
            continue;
        };

        for agent in &d.intended_agents {
            let found = d
                .found
                .iter()
                .find(|(a, _)| a == agent)
                .and_then(|(_, f)| f.clone());

            match found {
                None => {
                    // 母版要求注入但 agent 里没有
                    changed_files += 1;
                    lines.push(DiffLine {
                        agent: agent.clone(),
                        path: agent_config_path(agent),
                        action: DiffAction::Add,
                        target: mp.id.clone(),
                        detail: format!("新增 provider「{}」({})", mp.name, mp.base_url),
                    });
                }
                Some(f) => {
                    // key 指纹比对。母版 api_key 要现算指纹
                    let master_fp = fingerprint(&mp.api_key);
                    if f.key_fp.is_empty() {
                        changed_files += 1;
                        lines.push(DiffLine {
                            agent: agent.clone(),
                            path: f.location.clone(),
                            action: DiffAction::Add,
                            target: mp.id.clone(),
                            detail: "该 agent 里没有明文 key（可能在环境变量），需要注入".to_string(),
                        });
                    } else if f.key_fp != master_fp {
                        changed_files += 1;
                        conflicts.push(format!(
                            "{}: {} 里的 key 指纹 {} 与母版 {} 不同（长度 {} vs {}）",
                            agent,
                            mp.id,
                            f.key_fp,
                            master_fp,
                            f.key_len,
                            mp.api_key.len()
                        ));
                        lines.push(DiffLine {
                            agent: agent.clone(),
                            path: f.location.clone(),
                            action: DiffAction::Update,
                            target: mp.id.clone(),
                            detail: "key 与母版不一致，将按「最新为准」覆盖".to_string(),
                        });
                    } else {
                        lines.push(DiffLine {
                            agent: agent.clone(),
                            path: f.location.clone(),
                            action: DiffAction::Unchanged,
                            target: mp.id.clone(),
                            detail: "已一致".to_string(),
                        });
                    }
                }
            }
        }

        // 母版没声明要注入、但 agent 里有的。
        //
        // **不标 Remove**：所有注入器都遵循「只增改，不删」的原则 ——
        // 用户手动配的 provider（内置模板、其他中转）不该被同步抹掉。
        // 这里如实标 Unchanged 并说明原因，否则报告说"会移除"而实际不动，
        // 会误导用户以为可以在母版里删条目来清理 agent 配置。
        for (agent, found) in &d.found {
            if d.intended_agents.contains(agent) {
                continue;
            }
            if let Some(f) = found {
                lines.push(DiffLine {
                    agent: agent.clone(),
                    path: f.location.clone(),
                    action: DiffAction::Unchanged,
                    target: f.id.clone(),
                    detail: format!(
                        "「{}」存在但母版未声明注入 {}，保持原样不动",
                        f.name, agent
                    ),
                });
            }
        }
    }

    // 统计涉及的 agent
    let agent_count = lines
        .iter()
        .filter(|l| !matches!(l.action, DiffAction::Unchanged))
        .map(|l| l.agent.clone())
        .collect::<BTreeSet<_>>()
        .len();

    DiffReport {
        lines,
        conflicts,
        agent_count,
        changed_files,
    }
}

/// 归一化 URL 用于比对。忽略协议差异和尾部斜杠。
fn normalize_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_lowercase()
}

fn fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    format!("{:x}", h.finalize()).chars().take(8).collect()
}

/// 各 agent 的主配置文件路径，用于 diff 输出。
///
/// 从 agent 实现里取，不再自己维护一张表 ——
/// 重构前这张表漏了 workbuddy 和 dsh，就是这个做法的后果。
fn agent_config_path(agent: &str) -> String {
    let Some(a) = crate::agents::find(agent) else {
        return agent.to_string();
    };
    let targets = a.provider_targets();
    if targets.is_empty() {
        return agent.to_string();
    }
    targets
        .iter()
        .map(|p| crate::config::contract_tilde(p))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// 便捷入口：读母版 + 扫描全部 agent + 对比。
pub fn run() -> anyhow::Result<DiffReport> {
    let master = super::store::load_or_default()?;
    let scans = scan::scan_all();
    Ok(compare(&master, &scans))
}
