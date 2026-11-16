//! token 用量统计。
//!
//! ## 为什么要归一
//!
//! 四个 agent 都把用量写在各自磁盘上，但**口径不一致**，直接相加会算错：
//!
//! | agent | 原始语义 | 缓存字段 |
//! |---|---|---|
//! | pi | `input` 不含缓存，`total = input + output + cacheRead + cacheWrite` | 独立字段 |
//! | zcode | `input` **已含**缓存读取，缓存是子集 | 子集 |
//! | codex | `input` **已含**缓存读取，缓存是子集 | 子集 |
//! | claude | `input` 不含缓存，`cache_read` / `cache_creation` 独立 | 独立字段 |
//!
//! 本模块统一折算成「输入含缓存」再交给界面，四家的数字才可比。
//!
//! ## 为什么统计是增量的
//!
//! 会话数据加起来上 GB（本机 pi 595 MB、codex 191 MB、Cursor 的库 4.2 GB）。
//! 每次都全量重读的话，打开一次统计页要等几秒到半分钟。
//!
//! 所以每个 agent 的采集结果存在 `~/.agents/usage-cache/<agent>.json`，
//! 下次只处理新增的部分：JSONL 文件按字节偏移往后读，Cursor 的库按 rowid 水位往前查。
//! 具体机制见 `usage/cache.rs` 和 `agents/cursor.rs`。
//!
//! `--refresh` 或「重算」按钮会丢掉全部缓存从头算，用于用户怀疑数字不对的时候。
//!
//! ## 开关是显式的
//!
//! 默认**全部不统计**。用户必须在界面上勾选某个 agent，它才进统计。
//! 理由：这些库动辄上 GB，读它们是用户自己的决定，不该由工具默认替用户做。

pub mod cache;

use crate::types::{UsageAgentStatus, UsageRow, UsageReport};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 一条原始用量记录，尚未按天归并。
///
/// 各家采集器只负责把磁盘上的数据翻成这个结构，不关心口径——
/// 口径的归一在 [`RawUsage::normalize`] 里统一做。
pub struct RawUsage {
    /// Unix 毫秒时间戳
    pub at_ms: i64,
    /// 模型标识
    pub model: String,
    /// 原始 input
    pub input: u64,
    /// 原始 output
    pub output: u64,
    /// 缓存读取
    pub cache_read: u64,
    /// 缓存写入
    pub cache_write: u64,
    /// 原始口径：input 是否已包含缓存读取
    pub input_includes_cache: bool,
}

/// 归一后的用量。只保留界面要展示的两个量。
#[derive(Debug, Clone, Copy, Default)]
pub struct NormalizedUsage {
    /// 输入 token，含缓存命中
    pub input: u64,
    /// 输出 token
    pub output: u64,
}

impl RawUsage {
    /// 折算成「输入含缓存」口径。
    ///
    /// 两种原始口径：
    /// - `input_includes_cache = true`（zcode/codex/cursor）：原样取 input
    /// - `input_includes_cache = false`（pi/claude）：input + cache_read + cache_write
    ///
    /// cache_write 也算进输入——它确实是被送进模型前的 token，
    /// 只是计费上打折。漏掉它会让 pi 的数字偏小。
    pub fn normalize(&self) -> NormalizedUsage {
        let input = if self.input_includes_cache {
            self.input
        } else {
            self.input
                .saturating_add(self.cache_read)
                .saturating_add(self.cache_write)
        };
        NormalizedUsage {
            input,
            output: self.output,
        }
    }

    /// 这条记录是否一个 token 都没消耗。
    ///
    /// 这类记录真实存在：pi 里占 3.4%（本机 36163 条中 1241 条），
    /// 是请求失败、被中断、或指向本地模型时的空回包。
    /// 保留它们会让「调用次数」虚高、并在界面上占满整屏 0/0 的行，
    /// 所以统计时直接丢掉。
    ///
    /// 判断用归一前的原始值：只要任何一个字段非 0 就算有消耗。
    /// 只有 output 而没有 input 的情况（如 Claude 的本地提示消息）
    /// 是真实的，不能一起丢掉。
    pub fn is_empty(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_write == 0
    }
}

/// 按 (日期, agent, 模型) 累加。
///
/// 用一个 BTreeMap 而不是 Vec，省掉最后再排序；键自带顺序，
/// 输出的日期天然有序，界面不用自己排。
pub struct Aggregator {
    /// key = (日期, agent, 模型)
    buckets: BTreeMap<(String, String, String), (u64, u64, u64)>,
    /// 归一后总数，用于界面上的汇总条
    pub total_input: u64,
    pub total_output: u64,
    pub total_calls: u64,
}

impl Aggregator {
    pub fn new() -> Self {
        Self {
            buckets: BTreeMap::new(),
            total_input: 0,
            total_output: 0,
            total_calls: 0,
        }
    }

    /// 收一条记录。`date` 由调用方算好，避免这里重复做时区换算。
    pub fn add(&mut self, date: &str, agent: &str, raw: &RawUsage) {
        let n = raw.normalize();
        let key = (date.to_string(), agent.to_string(), raw.model.clone());
        let slot = self.buckets.entry(key).or_insert((0, 0, 0));
        slot.0 = slot.0.saturating_add(n.input);
        slot.1 = slot.1.saturating_add(n.output);
        slot.2 += 1;

        self.total_input = self.total_input.saturating_add(n.input);
        self.total_output = self.total_output.saturating_add(n.output);
        self.total_calls += 1;
    }

    /// 转成界面用的行，日期倒序、同日内按 agent 名和模型名排。
    ///
    /// 倒序是因为用户最关心「最近花了多少」，最上面就该是今天。
    pub fn into_rows(self) -> Vec<UsageRow> {
        let mut rows: Vec<UsageRow> = self
            .buckets
            .into_iter()
            .map(|((date, agent, model), (input, output, calls))| UsageRow {
                date,
                agent,
                model,
                input,
                output,
                calls,
            })
            .collect();
        rows.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then(a.agent.cmp(&b.agent))
                .then(a.model.cmp(&b.model))
        });
        rows
    }
}

impl Default for Aggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Unix 毫秒时间戳转本地日期 `YYYY-MM-DD`。
///
/// 用本地时区而不是 UTC：用户看「今天用了多少」，指的是自己日历上的今天。
/// 用 UTC 的话东八区早上 8 点前的用量会被算到前一天。
pub fn local_date(at_ms: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(at_ms).single() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        // 时间戳离谱时退化成空串，宁可少一行也不要 panic
        None => String::new(),
    }
}

/// 本地日期转当天 00:00 的 Unix 毫秒。
///
/// 用于给窗口算下界 —— 本地午夜对齐，界面按天聚合才对得上。
pub fn local_day_start_ms(days_ago: i64) -> i64 {
    use chrono::{Local, TimeZone};
    let today = Local::now().date_naive();
    let target = today - chrono::Duration::days(days_ago);
    let midnight = target.and_hms_opt(0, 0, 0).unwrap_or_default();
    Local
        .from_local_datetime(&midnight)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// 一个 agent 的采集结果：记录 + 元信息。
pub struct AgentUsage {
    /// 原始记录
    pub records: Vec<RawUsage>,
    /// 数据源是否可用（库/目录在不在）
    pub available: bool,
    /// 数据源位置，给界面显示
    pub source: String,
    /// 口径说明或已知限制
    pub note: String,
    /// 读取出错时的原因。空串表示正常
    pub error: String,
}

impl AgentUsage {
    /// 数据源不存在时的结果。
    pub fn unavailable(source: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            records: Vec::new(),
            available: false,
            source: source.into(),
            note: note.into(),
            error: String::new(),
        }
    }

    /// 读取失败时的结果。
    pub fn failed(source: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            records: Vec::new(),
            available: true,
            source: source.into(),
            note: String::new(),
            error: error.into(),
        }
    }
}

/// 统计窗口。
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// 天数。0 = 不限
    pub days: u32,
}

impl Window {
    /// 窗口下界的时间戳。不限时返回 0。
    pub fn start_ms(&self) -> i64 {
        if self.days == 0 {
            0
        } else {
            // days=7 表示「含今天在内的 7 天」，所以往前推 6 天
            local_day_start_ms(self.days.saturating_sub(1) as i64)
        }
    }

    /// 这条记录是否落在窗口内。
    ///
    /// 只判断下界，不判断上界：未来时间戳只可能是系统时钟错乱，
    /// 丢掉反而会让用户觉得数据无故消失。
    pub fn contains(&self, at_ms: i64) -> bool {
        at_ms >= self.start_ms()
    }
}

/// 组装最终报告。
///
/// 入参是各 agent 的「状态 + 采集结果」，出参是界面直接渲染的结构。
/// 采集失败的 agent 照样出现在 `agents` 里并带上 `error`，
/// 让用户看见「这家读了但失败了」，而不是让它静默消失。
pub fn build_report(
    days: u32,
    statuses: Vec<crate::types::UsageAgentStatus>,
    collected: Vec<(String, AgentUsage)>,
) -> UsageReport {
    let window = Window { days };
    let mut agg = Aggregator::new();

    // 用 agent id 找采集结果，避免依赖两个 Vec 的顺序一致
    let by_agent: BTreeMap<&str, &AgentUsage> =
        collected.iter().map(|(id, u)| (id.as_str(), u)).collect();

    for st in &statuses {
        if !st.enabled || !st.supported {
            continue;
        }
        let Some(usage) = by_agent.get(st.agent.as_str()) else {
            continue;
        };
        for r in &usage.records {
            if !window.contains(r.at_ms) {
                continue;
            }
            // 空记录在聚合层统一丢掉，各采集器不必各自判断
            if r.is_empty() {
                continue;
            }
            let date = local_date(r.at_ms);
            if date.is_empty() {
                continue;
            }
            agg.add(&date, &st.agent, r);
        }
    }

    let rows = agg.into_rows();
    let charts = build_charts(&rows);

    UsageReport {
        days,
        agents: statuses,
        rows,
        charts,
    }
}

/// 递归列出目录下匹配后缀的文件，按路径排序保证输出稳定。
pub fn walk_files(root: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let walker = walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok());
    for entry in walker {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        // ends_with 而不是后缀分割：`.jsonl.zst`、`.jsonl` 都能匹配到
        if name.ends_with(suffix) {
            out.push(entry.path().to_path_buf());
        }
    }
    out.sort();
    out
}

/// 从 ISO8601 时间串取 Unix 毫秒。解析失败返回 None。
///
/// 各 agent 的时间字段有两种：数字时间戳（zcode、cursor 的 ms）
/// 和 ISO 字符串（pi、codex、claude）。这里统一收口。
pub fn iso_to_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 从按天明细里算出图表数据。
///
/// 折线图按「模型」分组而不是按 agent —— 用户想看的通常是
/// 「哪个模型吃 token 最多」，agent 维度在下面的明细表里已经有了。
///
/// 饼图只显示占比，不区分 agent：同一个模型在多个 agent 上用，
/// 合起来算才是它在总消耗里的真实份量。
///
/// 最多保留 8 个模型。超出的合并成一条「其他」，否则饼图会碎成
/// 一堆看不出比例的细条，图例也排不下。
pub fn build_charts(rows: &[UsageRow]) -> crate::types::UsageCharts {
    use crate::types::{ModelShare, TrendPoint};

    if rows.is_empty() {
        return crate::types::UsageCharts {
            models: Vec::new(),
            trend: Vec::new(),
            shares: Vec::new(),
            dates: Vec::new(),
        };
    }

    // 模型 → 总量，用于排序和决定哪些进「其他」
    let mut per_model: BTreeMap<&str, u64> = BTreeMap::new();
    for r in rows {
        *per_model.entry(r.model.as_str()).or_insert(0) += r.input + r.output;
    }
    let mut ranked: Vec<(&str, u64)> = per_model.into_iter().collect();
    // 量大的排前面；同量按名字排，保证输出稳定
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    let keep = 8;
    let top: Vec<&str> = ranked.iter().take(keep).map(|(m, _)| *m).collect();
    let has_other = ranked.len() > keep;

    // 日期升序（折线图从左到右是时间推进）
    let mut dates: Vec<String> = rows.iter().map(|r| r.date.clone()).collect();
    dates.sort();
    dates.dedup();

    // (模型, 日期) → tokens。用 BTreeMap 让输出顺序确定
    let mut points: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut shares: BTreeMap<String, (u64, u64)> = BTreeMap::new();

    for r in rows {
        let tokens = r.input + r.output;
        let bucket = if top.contains(&r.model.as_str()) {
            r.model.clone()
        } else if has_other {
            "其他".to_string()
        } else {
            r.model.clone()
        };
        *points.entry((bucket.clone(), r.date.clone())).or_insert(0) += tokens;
        let e = shares.entry(bucket).or_insert((0, 0));
        e.0 += tokens;
        e.1 += r.calls;
    }

    let total: u64 = shares.values().map(|(t, _)| *t).sum();

    let mut models: Vec<String> = shares.keys().cloned().collect();
    models.sort_by(|a, b| {
        let ta = shares.get(a).map(|x| x.0).unwrap_or(0);
        let tb = shares.get(b).map(|x| x.0).unwrap_or(0);
        tb.cmp(&ta).then(a.cmp(b))
    });

    let share_list: Vec<ModelShare> = models
        .iter()
        .map(|m| {
            let (tokens, calls) = shares[m];
            ModelShare {
                model: m.clone(),
                percent: if total > 0 {
                    (tokens as f64) * 100.0 / (total as f64)
                } else {
                    0.0
                },
                tokens,
                calls,
            }
        })
        .collect();

    let trend: Vec<TrendPoint> = points
        .into_iter()
        .map(|((model, date), tokens)| TrendPoint {
            date,
            model,
            tokens,
        })
        .collect();

    crate::types::UsageCharts {
        models,
        trend,
        shares: share_list,
        dates,
    }
}

/// 组装一个 agent 的状态行，省掉各采集器各自重复拼装。
pub fn status(
    agent: &str,
    label: &str,
    supported: bool,
    enabled: bool,
    usage: &AgentUsage,
    records: u64,
) -> UsageAgentStatus {
    UsageAgentStatus {
        agent: agent.to_string(),
        label: label.to_string(),
        supported,
        enabled,
        available: usage.available,
        source: usage.source.clone(),
        note: usage.note.clone(),
        records,
        error: usage.error.clone(),
    }
}

/// 跑一次完整统计。
///
/// 流程：
/// 1. 遍历全部 agent，只对「支持用量采集」的生成状态行 ——
///    不支持的也放进行里（`supported = false`），界面才能解释为什么没有它
/// 2. **只对有启用的 agent 真正读盘**。没启用的连数据源都不碰，
///    这是「默认不统计」的落地方式：不勾选就不读
/// 3. 交给 [`build_report`] 按天聚合
///
/// `refresh = true` 时先清掉全部磁盘缓存，退化成全量重算。
/// 这是用户怀疑数字不对时的最后手段，也是唯一会重读全部历史数据的路径。
///
/// 单个 agent 读失败不影响其他 agent：错误记在它自己的状态行上。
pub fn collect_report(days: u32, refresh: bool) -> crate::types::UsageReport {
    if refresh {
        cache::clear_disk_cache();
    }

    let cfg = crate::config::Config::load().unwrap_or_default();
    let mut statuses = Vec::new();
    let mut collected = Vec::new();

    for agent in crate::agents::all() {
        let supported = agent.supports_usage();
        let enabled = supported && cfg.usage.is_enabled(agent.id());

        if !supported {
            // 不支持的 agent 也给一行，界面上能明确告诉用户「这家还没做」，
            // 而不是让它凭空消失、让用户怀疑是不是漏了
            statuses.push(UsageAgentStatus {
                agent: agent.id().to_string(),
                label: agent.label().to_string(),
                supported: false,
                enabled: false,
                available: false,
                source: String::new(),
                note: String::new(),
                records: 0,
                error: String::new(),
            });
            continue;
        }

        // 没启用就不读盘。数据源位置仍然要给，用户才知道打开后会读哪里
        if !enabled {
            let source = agent
                .usage_sources()
                .first()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            statuses.push(UsageAgentStatus {
                agent: agent.id().to_string(),
                label: agent.label().to_string(),
                supported: true,
                enabled: false,
                available: agent.usage_sources().iter().any(|p| p.exists()),
                source,
                note: agent.usage_note().to_string(),
                records: 0,
                error: String::new(),
            });
            continue;
        }

        let Some(usage) = agent.usage_collect() else {
            statuses.push(UsageAgentStatus {
                agent: agent.id().to_string(),
                label: agent.label().to_string(),
                supported: true,
                enabled,
                available: false,
                source: String::new(),
                note: agent.usage_note().to_string(),
                records: 0,
                error: "采集器没有返回结果".to_string(),
            });
            continue;
        };

        let n = usage.records.len() as u64;
        statuses.push(status(
            agent.id(),
            agent.label(),
            true,
            enabled,
            &usage,
            n,
        ));
        collected.push((agent.id().to_string(), usage));
    }

    // 采集期间改过的缓存统一落盘。放在这里而不是逐文件写：
    // pi 有几千个会话文件，逐个写盘会把增量省下的时间又花回去
    cache::flush_jsonl_cache();

    build_report(days, statuses, collected)
}

/// 打开或关闭某个 agent 的统计，写回 `~/.agents/agentcenter.toml`。
///
/// 返回写入后的状态。拒绝为不支持的 agent 开启统计 ——
/// 让配置里出现一个「已启用但永远不会产出数据」的条目只会误导人。
pub fn set_enabled(agent_id: &str, on: bool) -> anyhow::Result<bool> {
    if on {
        let supported = crate::agents::find(agent_id)
            .map(|a| a.supports_usage())
            .unwrap_or(false);
        if !supported {
            anyhow::bail!("{agent_id} 暂不支持用量统计");
        }
    }

    let mut cfg = crate::config::Config::load()?;
    let now = cfg.usage.set_enabled(agent_id, on);
    cfg.save()?;
    Ok(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pi 口径：input 不含缓存，要加回去。
    #[test]
    fn pi_style_input_adds_cache() {
        let raw = RawUsage {
            at_ms: 0,
            model: "m".into(),
            input: 100,
            output: 10,
            cache_read: 900,
            cache_write: 5,
            input_includes_cache: false,
        };
        let n = raw.normalize();
        assert_eq!(n.input, 1005);
        assert_eq!(n.output, 10);
    }

    /// zcode/codex 口径：input 已含缓存，不能重复加。
    #[test]
    fn zcode_style_input_already_includes_cache() {
        let raw = RawUsage {
            at_ms: 0,
            model: "m".into(),
            input: 1000,
            output: 10,
            cache_read: 900,
            cache_write: 0,
            input_includes_cache: true,
        };
        let n = raw.normalize();
        assert_eq!(n.input, 1000);
    }

    /// 同一天同一模型的多条要累加，不是覆盖。
    #[test]
    fn same_bucket_accumulates() {
        let mut agg = Aggregator::new();
        let mk = |i| RawUsage {
            at_ms: 0,
            model: "m".into(),
            input: i,
            output: 1,
            cache_read: 0,
            cache_write: 0,
            input_includes_cache: true,
        };
        agg.add("2026-11-12", "zcode", &mk(100));
        agg.add("2026-11-12", "zcode", &mk(200));
        assert_eq!(agg.total_input, 300);
        assert_eq!(agg.total_calls, 2);

        let rows = agg.into_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input, 300);
        assert_eq!(rows[0].calls, 2);
    }

    /// 窗口下界对齐本地午夜，且 days=1 只含今天。
    #[test]
    fn window_covers_today_only_when_one_day() {
        let w = Window { days: 1 };
        let today_start = local_day_start_ms(0);
        assert_eq!(w.start_ms(), today_start);
        assert!(w.contains(today_start));
        assert!(!w.contains(today_start - 1));
    }

    /// days=0 表示不限，任何时间戳都收。
    #[test]
    fn window_zero_means_unlimited() {
        let w = Window { days: 0 };
        assert_eq!(w.start_ms(), 0);
        assert!(w.contains(0));
    }
}
