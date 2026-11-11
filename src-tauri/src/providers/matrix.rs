//! provider 分配矩阵：provider 池 × agent 的勾选关系。
//!
//! 和 MCP 的矩阵同形，语义也一致：母版里的 `agents` 字段就是分配关系。
//! 界面上默认只列 provider，点「分配」才展开 agent 卡片。
//!
//! **列来自 `agents::all()` 里支持 provider 注入的那些**，
//! 这个模块不再自己维护 agent 清单。
//!
//! ## 单槽位互斥
//!
//! 某些 agent 同一时刻只能有一个 provider 生效（Claude Code 只有一组
//! `ANTHROPIC_*` 环境变量）。这类 agent 由 [`Agent::single_slot`] 声明，
//! 勾选某个 provider 时自动把其他 provider 上的它摘掉。

use super::model::Provider;
use super::store;
use crate::agents;
// 列结构定义在 types.rs，和前端契约一一对应
pub use crate::types::{AgentColumn, ProviderMatrix, ProviderView};

/// 把一条 provider 转成界面展示结构。key 只给长度和末尾 4 位。
fn to_view(p: &Provider) -> ProviderView {
    let tail: String = p
        .api_key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    ProviderView {
        id: p.id.clone(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        kind: p.kind.as_str().to_string(),
        kind_label: p.kind.label().to_string(),
        agent_kinds: p
            .agent_kinds
            .iter()
            .map(|(a, k)| (a.clone(), k.as_str().to_string()))
            .collect(),
        models: p.models.clone(),
        agents: p.agents.clone(),
        note: p.note.clone(),
        key_len: p.api_key.len(),
        key_tail: if p.api_key.len() >= 4 {
            tail
        } else {
            String::new()
        },
    }
}

/// 构建 provider 分配矩阵。
pub fn build() -> anyhow::Result<ProviderMatrix> {
    let providers = store::load_or_default()?;

    let columns: Vec<AgentColumn> = agents::provider_capable()
        .iter()
        .map(|a| {
            let available = a.provider_ready();
            AgentColumn {
                agent: a.id().to_string(),
                label: a.label().to_string(),
                available,
                note: if available {
                    String::new()
                } else {
                    "配置文件不存在".to_string()
                },
                single_slot: a.single_slot(),
                supported_kinds: a
                    .supported_kinds()
                    .iter()
                    .map(|k| k.as_str().to_string())
                    .collect(),
            }
        })
        .collect();

    Ok(ProviderMatrix {
        columns,
        empty: providers.is_empty(),
        providers: providers.iter().map(to_view).collect(),
    })
}

/// 批量设置某个 provider 的分配列表。
///
/// 一次写整行，避免逐格写入留下中间状态。
/// 声明了单槽位的 agent 会从其他 provider 上摘掉，保证只有一个生效。
pub fn set_row(provider_id: &str, agents_in: &[String]) -> anyhow::Result<()> {
    let mut providers = store::load_or_default()?;

    // 只接受已知的、且支持 provider 注入的 agent 名
    let known: Vec<&'static str> = agents::provider_capable()
        .iter()
        .map(|a| a.id())
        .collect();

    let mut clean: Vec<String> = Vec::new();
    for a in agents_in {
        if known.contains(&a.as_str()) && !clean.contains(a) {
            clean.push(a.clone());
        }
    }

    if !providers.iter().any(|p| p.id == provider_id) {
        anyhow::bail!("母版里没有 provider：{provider_id}");
    }

    // 单槽位互斥：这个 provider 勾了某个单槽位 agent，就把别人的摘掉
    let exclusive: Vec<&'static str> = agents::provider_capable()
        .iter()
        .filter(|a| a.single_slot())
        .map(|a| a.id())
        .collect();

    for slot in exclusive {
        if clean.iter().any(|a| a == slot) {
            for other in providers.iter_mut().filter(|p| p.id != provider_id) {
                other.agents.retain(|a| a != slot);
            }
        }
    }

    let Some(target) = providers.iter_mut().find(|p| p.id == provider_id) else {
        anyhow::bail!("母版里没有 provider：{provider_id}");
    };
    target.agents = clean;

    store::save(&providers)
}

/// 切换某个 provider 在某个 agent 的分配状态。
///
/// 内部走 [`set_row`]，单槽位互斥那套逻辑只写一遍。
pub fn set_assignment(provider_id: &str, agent: &str, on: bool) -> anyhow::Result<()> {
    let providers = store::load_or_default()?;
    let Some(p) = providers.iter().find(|p| p.id == provider_id) else {
        anyhow::bail!("母版里没有 provider：{provider_id}");
    };

    let mut next = p.agents.clone();
    if on {
        if !next.iter().any(|a| a == agent) {
            next.push(agent.to_string());
        }
    } else {
        next.retain(|a| a != agent);
    }

    set_row(provider_id, &next)
}
