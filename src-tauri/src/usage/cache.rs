//! 用量统计的磁盘缓存。
//!
//! 为什么需要这一层：会话数据加起来上 GB（本机 pi 595 MB、codex 191 MB、
//! Cursor 的 state.vscdb 4.2 GB）。每次打开统计页都全量重读，等几秒到半分钟，
//! 这个页面就没人愿意点开了。
//!
//! 所以缓存放在磁盘上（`~/.agents/usage-cache/<agent>.json`），并且是**增量**的：
//! 不是「指纹变了就整份重读」，而是记住上次读到文件的第几个字节，
//! 下次只读新追加的那一段。
//!
//! ## 两类数据源，两种增量方式
//!
//! | 数据源 | 增量依据 | 为什么能这么算 |
//! |---|---|---|
//! | JSONL 会话文件（pi / codex / claude） | 文件字节偏移 | 会话文件是纯追加写的，旧字节永远不变 |
//! | Cursor 的 state.vscdb | `cursorDiskKV` 的 rowid 水位 | rowid 随插入单调递增；见 `agents/cursor.rs` |
//!
//! ZCode 的 `model_usage` 表不做增量：全量查询只要 0.05 秒，而它的行是
//! 「先插入 running、跑完再改成 completed」，按 rowid 设水位会漏掉这些行。
//! 用 0.05 秒换一个不会算错的实现，划算。
//!
//! ## 什么情况会让增量失效（自动退回全量）
//!
//! - 文件变短了：说明被截断或重写，偏移没有意义，从头读
//! - 文件头部 4 KB 的哈希对不上：说明不是追加，是整体改写
//! - 缓存版本号变了（解析逻辑升级）：整份作废
//! - 用户显式跑 `usage report --refresh`
//!
//! ## 半行怎么处理
//!
//! 进程正在写的时候，文件尾部可能只有半条 JSON。这里只在「读到换行」时才推进偏移；
//! 最后那段没有换行的内容，只有能完整解析成 JSON 才当作已读，否则留给下次。
//! 不这么做的话，一次半行写入会把后面所有记录都读成错位的垃圾。

use crate::usage::RawUsage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 缓存格式版本。解析逻辑有不兼容变化时加一，旧缓存会被丢弃。
const JSONL_VERSION: u32 = 1;

/// 缓存目录：`~/.agents/usage-cache/`。
///
/// 放在 `~/.agents` 下而不是系统临时目录：那份目录是用户自己的仓库，
/// 缓存跟着它走，用户能看见、能手动删，也知道「这些数字哪来的」。
pub fn cache_dir() -> PathBuf {
    crate::providers::store::home_dir()
        .join(".agents")
        .join("usage-cache")
}

/// 读某个 agent 的缓存文件。没有、读不了、格式不对都返回 None。
///
/// 所有失败都当成「没有缓存」处理 —— 缓存是加速手段，不是正确性依赖，
/// 它坏掉时最坏的结果应该只是慢一点，不能是统计失败。
pub fn cache_load<T: serde::de::DeserializeOwned>(agent: &str) -> Option<T> {
    let file = cache_dir().join(format!("{agent}.json"));
    let text = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&text).ok()
}

/// 写某个 agent 的缓存文件。
///
/// 先写 `.json.tmp` 再改名：进程写到一半被杀时，磁盘上要么是旧的完整缓存，
/// 要么是新的完整缓存，不会留下半份 JSON 让下次读崩。
pub fn cache_store<T: Serialize>(agent: &str, value: &T) {
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    if std::fs::create_dir_all(cache_dir()).is_err() {
        return;
    }
    let file = cache_dir().join(format!("{agent}.json"));
    let tmp = cache_dir().join(format!("{agent}.json.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        // Windows 上 rename 会覆盖已存在的目标文件，不需要先删
        let _ = std::fs::rename(&tmp, &file);
    }
}

/// 清掉所有磁盘缓存，并顺手清掉进程内的。
///
/// 给 `usage report --refresh` 和「重算」按钮用：用户怀疑数字不对时的
/// 最后手段，直接丢掉一切从头算。
pub fn clear_disk_cache() {
    reset_jsonl_cache();
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if matches!(p.extension().and_then(|s| s.to_str()), Some("json") | Some("tmp")) {
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---- 文件指纹 ----

/// 文件的一瞬间状态：大小 + 修改时间。
///
/// 两个都要：只看大小会漏掉「原地改写但长度不变」，只看修改时间
/// 在粗粒度时间戳上会漏掉同一时刻的多次追加。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    /// 修改时间，纳秒。秒级精度太粗
    mtime_ns: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or(0);
        Some(Self {
            size: meta.len(),
            mtime_ns,
        })
    }
}

/// FNV-1a 64 位哈希。
///
/// 用自己实现而不是 `DefaultHasher`：后者的输出**在不同 Rust 版本间不保证稳定**，
/// 拿它算出的哈希存到磁盘，换个编译器版本就会全部对不上、缓存永久失效。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 算文件前 4 KB 的哈希。用来发现「文件被整体改写」。
///
/// 只读 4 KB 是因为：完整比对旧内容的开销等于重读一遍，那增量就白做了；
/// 而会话文件被整体改写时，开头几乎必然跟着变。
fn head_hash(path: &Path, offset: u64) -> Option<u64> {
    let n = offset.min(4096) as usize;
    let mut buf = vec![0u8; n];
    let mut f = std::fs::File::open(path).ok()?;
    f.read_exact(&mut buf).ok()?;
    Some(fnv1a(&buf))
}

// ---- 缓存里存的一条记录 ----

/// 缓存里存的一条用量记录。
///
/// 与 [`RawUsage`] 字段完全一致，区别只是可以 Clone + 序列化。
/// 手写 serde 实现是为了让磁盘格式稳定：字段名固定成下划线风格，
/// 以后给 `RawUsage` 加字段也不会静默改变缓存格式。
#[derive(Debug, Clone)]
pub struct CachedRecord {
    pub at_ms: i64,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub input_includes_cache: bool,
}

impl From<&RawUsage> for CachedRecord {
    fn from(r: &RawUsage) -> Self {
        Self {
            at_ms: r.at_ms,
            model: r.model.clone(),
            input: r.input,
            output: r.output,
            cache_read: r.cache_read,
            cache_write: r.cache_write,
            input_includes_cache: r.input_includes_cache,
        }
    }
}

impl From<&CachedRecord> for RawUsage {
    fn from(c: &CachedRecord) -> Self {
        Self {
            at_ms: c.at_ms,
            model: c.model.clone(),
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
            input_includes_cache: c.input_includes_cache,
        }
    }
}

impl Serialize for CachedRecord {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CachedRecord", 7)?;
        st.serialize_field("at_ms", &self.at_ms)?;
        st.serialize_field("model", &self.model)?;
        st.serialize_field("input", &self.input)?;
        st.serialize_field("output", &self.output)?;
        st.serialize_field("cache_read", &self.cache_read)?;
        st.serialize_field("cache_write", &self.cache_write)?;
        st.serialize_field("input_includes_cache", &self.input_includes_cache)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for CachedRecord {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Helper {
            at_ms: i64,
            model: String,
            input: u64,
            output: u64,
            cache_read: u64,
            cache_write: u64,
            input_includes_cache: bool,
        }
        let h = Helper::deserialize(d)?;
        Ok(CachedRecord {
            at_ms: h.at_ms,
            model: h.model,
            input: h.input,
            output: h.output,
            cache_read: h.cache_read,
            cache_write: h.cache_write,
            input_includes_cache: h.input_includes_cache,
        })
    }
}

// ---- JSONL 增量缓存 ----

/// 单个会话文件在缓存里的状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    /// 已经消费到的字节位置，永远停在某个换行之后
    offset: u64,
    /// 上次读完时的文件修改时间。和当前对不上说明文件动过，要读尾巴
    mtime_ns: u64,
    /// 已消费部分的前 4 KB 哈希。对不上说明不是追加，是整体改写
    head_hash: u64,
    /// 已解析出的记录
    records: Vec<CachedRecord>,
    /// 解析器的跨行状态。codex 需要「turn_id → 模型」的映射，
    /// 不存下来的话，从新追加的字节里读到 usage 记录时会找不到模型
    state: serde_json::Value,
}

/// 一个 agent 的全部 JSONL 缓存。
#[derive(Debug, Serialize, Deserialize)]
struct JsonlCache {
    version: u32,
    files: BTreeMap<String, FileEntry>,
}

impl JsonlCache {
    /// 从磁盘读缓存。版本不符或读失败都返回空缓存（等于下次全量重算）。
    fn load(agent: &str) -> Self {
        match cache_load::<JsonlCache>(agent) {
            Some(c) if c.version == JSONL_VERSION => c,
            _ => Self {
                version: JSONL_VERSION,
                files: BTreeMap::new(),
            },
        }
    }
}

/// 进程内的缓存表：agent → (缓存内容, 是否需要写回磁盘)。
struct Slot {
    cache: JsonlCache,
    dirty: bool,
}

static JSONL_CACHES: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();

fn jsonl_caches() -> &'static Mutex<HashMap<String, Slot>> {
    JSONL_CACHES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取缓存表的锁。中毒（别的线程 panic 过）时照样取数据：
/// 这里的缓存只是派生数据，不会因为一次 panic 就变成有害状态。
fn lock_caches() -> std::sync::MutexGuard<'static, HashMap<String, Slot>> {
    match jsonl_caches().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

/// 清掉进程内缓存。下一个 agent 采集会重新从磁盘读。
pub fn reset_jsonl_cache() {
    lock_caches().clear();
}

/// 把脏掉的缓存写回磁盘。
///
/// 由 [`crate::usage::collect_report`] 在结束时统一调用，而不是每次解析完一个文件就写：
/// pi 有几千个会话文件，逐文件写盘会把增量省下的时间又花回去。
pub fn flush_jsonl_cache() {
    let mut map = lock_caches();
    for (agent, slot) in map.iter_mut() {
        if slot.dirty {
            cache_store(agent, &slot.cache);
            slot.dirty = false;
        }
    }
}

/// 解析一个没有跨行状态的 JSONL 文件（pi、claude）。
pub fn cached_jsonl<F>(agent: &str, path: &Path, mut parse: F) -> Vec<RawUsage>
where
    F: FnMut(&serde_json::Value, &mut Vec<RawUsage>),
{
    cached_jsonl_stateful::<(), _>(agent, path, |v, _state: &mut (), out| parse(v, out))
}

/// 解析一个带跨行状态的 JSONL 文件（codex），只读新增的字节。
///
/// ## 参数
///
/// - `agent`：缓存文件名，也是「同一份缓存」的分组键
/// - `path`：会话文件
/// - `parse`：处理一条已解析的 JSON 行。`state` 跨行、跨调用存活，
///   由调用方定义类型 `S`，这里负责序列化存盘
///
/// ## 返回值
///
/// 这个文件的**全部**记录（上次的 + 这次新读的）。调用方不用关心哪些是增量。
///
/// ## 为什么状态要跟着文件走
///
/// codex 的 `token_usage_record` 里没有模型名，模型在同一个文件前面的
/// `turn_context` 行里。只读文件尾巴时那些行早就读过了，不把
/// 「turn_id → 模型」传下来，新记录会全部因为找不到模型被丢掉。
pub fn cached_jsonl_stateful<S, F>(agent: &str, path: &Path, mut parse: F) -> Vec<RawUsage>
where
    S: Serialize + serde::de::DeserializeOwned + Default,
    F: FnMut(&serde_json::Value, &mut S, &mut Vec<RawUsage>),
{
    let key = path.to_string_lossy().into_owned();
    let Some(stamp) = Stamp::of(path) else {
        // 文件没了：顺手把条目清掉，否则删掉的会话会一直占着缓存
        let mut map = lock_caches();
        if let Some(slot) = map.get_mut(agent) {
            if slot.cache.files.remove(&key).is_some() {
                slot.dirty = true;
            }
        }
        return Vec::new();
    };

    let mut map = lock_caches();
    let slot = map.entry(agent.to_string()).or_insert_with(|| Slot {
        cache: JsonlCache::load(agent),
        dirty: false,
    });

    // 文件没动过：直接返回上次的结果，一次磁盘读都没有
    if let Some(e) = slot.cache.files.get(&key) {
        if e.offset == stamp.size && e.mtime_ns == stamp.mtime_ns {
            return e.records.iter().map(RawUsage::from).collect();
        }
    }

    // 取出旧条目，判断能不能从中间接着读
    let prev = slot.cache.files.remove(&key);
    let can_resume = prev.as_ref().is_some_and(|e| {
        // offset == 0 说明上次读到的是空文件，从头读也就一行的事
        e.offset > 0 && e.offset <= stamp.size && head_hash(path, e.offset) == Some(e.head_hash)
    });

    let (start, mut records, mut state) = match (&prev, can_resume) {
        (Some(e), true) => (
            e.offset,
            e.records.iter().map(RawUsage::from).collect::<Vec<_>>(),
            serde_json::from_value::<S>(e.state.clone()).unwrap_or_default(),
        ),
        // 全新文件、被截断、被改写：一律从头解析
        _ => (0u64, Vec::new(), S::default()),
    };

    let mut fresh = Vec::new();
    let Some(consumed) = read_lines_from(path, start, &mut state, &mut parse, &mut fresh) else {
        // 读失败（权限、被独占锁住）：把旧条目放回去，返回上次的结果。
        // 统计少算一点可以接受，把有效缓存清掉不行
        if let Some(e) = prev {
            slot.cache.files.insert(key, e);
        }
        return records;
    };
    records.extend(fresh);

    let new_offset = start + consumed;
    let entry = FileEntry {
        offset: new_offset,
        // 存读之前的 mtime：如果读的过程中文件又长了，下次 mtime 不同，
        // 会再读一次尾巴，不会漏
        mtime_ns: stamp.mtime_ns,
        head_hash: head_hash(path, new_offset).unwrap_or(0),
        records: records.iter().map(CachedRecord::from).collect(),
        state: serde_json::to_value(&state).unwrap_or(serde_json::Value::Null),
    };
    slot.cache.files.insert(key, entry);
    slot.dirty = true;

    records
}

/// 从 `start` 字节开始逐行读，喂给 `parse`，返回消费了多少字节。
///
/// 返回的字节数**永远停在换行之后**，除非最后一行能完整解析成 JSON
/// （有些写入方最后一行不带换行）。这样下次从这里接着读不会错位。
///
/// 打开/定位失败时返回 None，调用方负责保住旧缓存。
fn read_lines_from<S, F>(
    path: &Path,
    start: u64,
    state: &mut S,
    parse: &mut F,
    out: &mut Vec<RawUsage>,
) -> Option<u64>
where
    F: FnMut(&serde_json::Value, &mut S, &mut Vec<RawUsage>),
{
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);

    let mut line: Vec<u8> = Vec::new();
    let mut consumed: u64 = 0;

    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line).ok()?;
        if n == 0 {
            break;
        }
        let complete = line.last() == Some(&b'\n');
        let text = String::from_utf8_lossy(&line);
        let trimmed = text.trim();

        if trimmed.is_empty() {
            // 空行也要吃掉，否则下次还会读到同一行
            if !complete {
                break;
            }
            consumed += n as u64;
            continue;
        }

        // 单行 JSON 坏了就跳过。会话文件可能因为进程被杀而断在半行，
        // 一条坏记录不该让整份统计失败
        let parsed = serde_json::from_str::<serde_json::Value>(trimmed);
        if let Ok(v) = &parsed {
            parse(v, state, out);
        }

        if !complete {
            // 没有结尾换行的最后一行：能解析才当作已读，否则留到下次。
            // 这是「正在写」和「没有结尾换行的完整文件」之间的唯一区分手段
            if parsed.is_ok() {
                consumed += n as u64;
            }
            break;
        }
        consumed += n as u64;
    }

    Some(consumed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时文件路径。同一个测试里用不同后缀，避免并行测试互相覆盖。
    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agentcenter-usage-test-{tag}-{}.jsonl",
            std::process::id()
        ));
        p
    }

    /// 追加内容后，第二次调用只读新增的字节，结果与一次性全量解析相同。
    #[test]
    fn append_only_reads_increment() {
        let path = temp_path("append");
        let _ = std::fs::remove_file(&path);

        // 造两行 pi 风格的记录
        let mk = |ts: &str, input: u64| {
            format!(
                r#"{{"type":"message","timestamp":"{ts}","message":{{"role":"assistant","model":"m","usage":{{"input":{input},"output":1,"cacheRead":0,"cacheWrite":0}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{}\n", mk("2026-11-12T00:00:00Z", 10))).unwrap();

        // 用一个独立的 agent 名，避免和真实缓存撞
        let agent = format!("test-append-{}", std::process::id());
        let first = cached_jsonl(&agent, &path, |v, out| {
            if let Some(u) = v.pointer("/message/usage/input").and_then(|x| x.as_u64()) {
                out.push(RawUsage {
                    at_ms: 0,
                    model: "m".into(),
                    input: u,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    input_includes_cache: false,
                });
            }
        });
        assert_eq!(first.len(), 1);

        // 追一行，模拟会话继续写
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write as _;
        writeln!(f, "{}", mk("2026-11-12T00:00:01Z", 20)).unwrap();
        drop(f);
        // 文件系统时间戳粒度可能把两次写入压到同一纳秒，主动拨一下
        std::thread::sleep(std::time::Duration::from_millis(10));

        let second = cached_jsonl(&agent, &path, |v, out| {
            if let Some(u) = v.pointer("/message/usage/input").and_then(|x| x.as_u64()) {
                out.push(RawUsage {
                    at_ms: 0,
                    model: "m".into(),
                    input: u,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    input_includes_cache: false,
                });
            }
        });
        assert_eq!(second.len(), 2, "追加后应看到两条，而不是重复或丢失");
        assert_eq!(second.iter().map(|r| r.input).sum::<u64>(), 30);

        let _ = std::fs::remove_file(&path);
    }

    /// 文件被整体改写（变短）时，不能拿旧偏移接着读，必须从头解析。
    #[test]
    fn truncation_falls_back_to_full_parse() {
        let path = temp_path("truncate");
        let agent = format!("test-truncate-{}", std::process::id());
        std::fs::write(&path, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();

        let count = |window: &str| {
            cached_jsonl(&agent, &path, |_v, out| {
                out.push(RawUsage {
                    at_ms: 0,
                    model: window.into(),
                    input: 1,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    input_includes_cache: true,
                });
            })
            .len()
        };
        assert_eq!(count("a"), 3);

        // 重写成更短的内容
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, "{\"n\":9}\n").unwrap();
        assert_eq!(count("b"), 1, "变短后应重新解析，得到新文件的 1 条");

        let _ = std::fs::remove_file(&path);
    }

    /// 没有换行的半行不该被当成记录吃掉，补全后要能读到。
    #[test]
    fn partial_tail_is_not_consumed() {
        let path = temp_path("partial");
        let agent = format!("test-partial-{}", std::process::id());
        let _ = std::fs::remove_file(&path);

        std::fs::write(&path, "{\"n\":1}\n{\"n\":2").unwrap();
        let n = |window: &str| {
            cached_jsonl(&agent, &path, |_v, out| {
                out.push(RawUsage {
                    at_ms: 0,
                    model: window.into(),
                    input: 1,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    input_includes_cache: true,
                });
            })
            .len()
        };
        assert_eq!(n("a"), 1, "半行解析不出来，只能算前面那一条");

        // 补全半行
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, "{\"n\":1}\n{\"n\":2}\n").unwrap();
        assert_eq!(n("b"), 2, "半行补全后应能读到");

        let _ = std::fs::remove_file(&path);
    }
}
