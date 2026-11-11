//! 各 agent 的 API key 体检。
//!
//! **这个模块是纯只读的**。它不写任何凭证文件，也不尝试解密
//! 系统凭据库里的内容。设计目标是回答三个问题：
//!
//! 1. 我到底有几把 key，分别配在哪个 agent 上？
//! 2. 哪些 key 是重复的（同一个 key 散落在多个 agent）？
//! 3. 哪些 provider 配了多把不同的 key（容易搞混用哪把）？
//!
//! 密钥原文只在内存里停留于计算指纹的那一瞬间，绝不进入
//! 返回给前端的数据结构。界面拿到的只有指纹前 8 位和长度。
//!
//! ## agent 清单只有一处
//!
//! 各 agent 的扫描逻辑在 `agents/<id>.rs` 的
//! [`crate::agents::Agent::credential_entries`] 里。这个模块只负责
//! 遍历、聚合和统计 —— 不再自己维护一份 agent 清单。
//! 重构前 `unsupported_agents()` 里写着一个 `gemini`，而 registry
//! 里根本没有它，就是清单分裂的结果。

use crate::agents;
use crate::agents::model::{collect, fingerprint, tail, RawEntry};
use crate::types::{CredentialEntry, CredentialOverview, UnsupportedAgent};
use std::collections::BTreeMap;

/// 环境变量里的 key。不算任何 agent，单独归到「环境变量」下。
///
/// 只查已知的几个名字，不做模糊匹配 —— 模糊匹配会把
/// `ZAI_BUSINESS_BASE_URL` 这种非密钥变量也算进来。
fn scan_env(out: &mut Vec<RawEntry>) {
    const NAMES: &[&str] = &[
        "DEEPSEEK_API_KEY",
        "COMMAND_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "ZCODE_OFFICIAL_ENDPOINT_TOKEN",
    ];
    for name in NAMES {
        if let Ok(v) = std::env::var(name) {
            collect(
                out,
                "环境变量",
                name,
                "api_key",
                &format!("${name}"),
                Some(&v),
            );
        }
    }
}

/// 返回那些「读不到明文、需要用户自己登录」的 agent，附带原因。
///
/// 数据来自各 agent 自己的 `credential_unsupported()`，
/// 只有在它已安装时才报 —— 没装的完全不出现在界面上。
fn unsupported_agents() -> Vec<UnsupportedAgent> {
    agents::all()
        .iter()
        .filter_map(|a| {
            let reason = a.credential_unsupported()?;
            if a.installed() {
                Some(UnsupportedAgent {
                    agent: a.id().to_string(),
                    reason: reason.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// 执行完整体检，返回分组后的结果。
///
/// 分组维度是 provider：同一个 provider 下的所有 key 放在一起，
/// 方便看出「这个服务我配了几把钥匙、分别给了谁」。
pub fn inspect() -> CredentialOverview {
    let mut raw: Vec<RawEntry> = Vec::new();

    // 各 agent 各扫自己的，顺序由 agents::all() 的 order 决定
    for agent in agents::all() {
        raw.extend(agent.credential_entries());
    }
    // 环境变量是全局的，放在最后
    scan_env(&mut raw);

    // 按 provider 聚合
    let mut by_provider: BTreeMap<String, Vec<CredentialEntry>> = BTreeMap::new();
    for e in &raw {
        by_provider
            .entry(e.provider.clone())
            .or_default()
            .push(CredentialEntry {
                agent: e.agent.clone(),
                provider: e.provider.clone(),
                kind: e.kind.clone(),
                location: e.location.clone(),
                fingerprint: e.fingerprint.clone(),
                length: e.length,
                tail: e.tail.clone(),
                // 下面两个字段在分组后统一回填
                shared_with: Vec::new(),
                duplicated: false,
            });
    }

    // 回填「这个指纹还出现在哪些别的 agent 上」
    // 先算每个指纹出现在哪些 agent
    let mut fp_agents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &raw {
        let list = fp_agents.entry(e.fingerprint.clone()).or_default();
        if !list.contains(&e.agent) {
            list.push(e.agent.clone());
        }
    }

    let mut providers: Vec<Vec<CredentialEntry>> = Vec::new();
    for (_, mut entries) in by_provider {
        for e in &mut entries {
            let agents_of_fp = fp_agents.get(&e.fingerprint).cloned().unwrap_or_default();
            e.duplicated = agents_of_fp.len() > 1;
            e.shared_with = agents_of_fp
                .into_iter()
                .filter(|a| a != &e.agent)
                .collect();
        }
        // 同一 provider 内按 agent 名排序，输出稳定
        entries.sort_by(|a, b| a.agent.cmp(&b.agent).then(a.location.cmp(&b.location)));
        providers.push(entries);
    }

    // 统计
    let total_entries = raw.len();
    let unique_keys = fp_agents.len();
    let duplicated_keys = fp_agents.values().filter(|v| v.len() > 1).count();

    // 同一 provider 下有几把不同的 key。
    // 注意不能用 dedup() —— 它只去掉「相邻」重复，
    // 而这里的条目按 agent 名排序，同一 provider 的指纹可能不相邻。
    // 必须用集合去重再数个数。
    let providers_with_multiple_keys = providers
        .iter()
        .filter(|p| {
            let fps: std::collections::BTreeSet<&str> =
                p.iter().map(|e| e.fingerprint.as_str()).collect();
            fps.len() > 1
        })
        .count();

    CredentialOverview {
        providers,
        unsupported: unsupported_agents(),
        total_entries,
        unique_keys,
        duplicated_keys,
        providers_with_multiple_keys,
    }
}

/// 暴露给测试：算指纹。
pub fn fp(secret: &str) -> String {
    fingerprint(secret)
}

/// 暴露给测试：取末尾 4 位。
pub fn tail_of(secret: &str) -> String {
    tail(secret)
}

/// 暴露给测试：服务名归一化。
pub fn normalize(raw: &str) -> String {
    crate::agents::model::normalize_provider(raw)
}
