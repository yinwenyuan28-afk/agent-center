//! Cursor 的自带密钥（BYOK）配置：读状态、算计划、写回。
//!
//! ## 为什么单独一个模块，而不是走 [`super::Injector`]
//!
//! 其它 agent 的配置都是文本文件，注入器产出「写完之后整份文件长什么样」，
//! 由 [`super::safe_write`] 统一备份和原子写。Cursor 不适用：
//!
//! - 配置在 SQLite 库 `state.vscdb` 里，是 4 GB 级的二进制文件，不是文本；
//! - 密钥要先用 Electron safeStorage 的算法加密才能写进去；
//! - **Cursor 运行中写库会被它的内存状态覆盖**，必须要求用户先退出。
//!
//! 所以这里的流程是「探测 → 算差异 → 用户确认 → 退出 Cursor → 写 → 提示重启」，
//! 不是自动同步的一部分。`providers sync` 不会碰它。
//!
//! ## 涉及的三个键（都在 `ItemTable` 表）
//!
//! | 键 | 值 |
//! | --- | --- |
//! | `applicationUser` 这个 JSON 里的 `useOpenAIKey` | 总开关。false 时自定义模型名会被当自家目录模型校验，必失败 |
//! | 同一个 JSON 里的 `openAIBaseUrl` | 接口地址。null 表示走 OpenAI 官方 |
//! | `secret://cursorAuth/openAIKey` | 密钥，`v10` + 12 字节 nonce + AES-256-GCM 密文 |
//!
//! ## 加密链
//!
//! 1. `%APPDATA%\Cursor\Local State` 的 `os_crypt.encrypted_key`，base64 解码后
//!    去掉 `DPAPI` 前缀，交给 Windows DPAPI [`CryptUnprotectData`] 解开，得到
//!    32 字节 AES 主密钥；
//! 2. 密文格式是 `v10` + 12 字节 nonce + 密文 + 16 字节 tag，AES-256-GCM。
//!
//! 主密钥按用户绑定，同一台机器同一账户下 agentcenter 能解也能加。换机器或
//! 换账户就失效 —— 这是设计如此，不需要额外处理。
//!
//! [`CryptUnprotectData`]: https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};

/// `applicationUser` 在 `ItemTable` 里的完整键名。
///
/// 这是 Cursor 的 reactive storage 内部路径，不是一个稳定的公开接口。
/// Cursor 升级可能改它 —— 改了我们会在 [`read_state`] 里明确报「找不到该键」。
const APP_KEY: &str =
    "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";

/// 密钥在 `ItemTable` 里的键名。前缀 `secret://` 表示这是 safeStorage 加密值。
const SECRET_KEY: &str = "secret://cursorAuth/openAIKey";

/// Cursor 库相对于用户主目录的位置。
const DB_REL: &str = r"AppData\Roaming\Cursor\User\globalStorage\state.vscdb";

/// `Local State` 相对于用户主目录的位置，里面放着 safeStorage 主密钥。
const LOCAL_STATE_REL: &str = r"AppData\Roaming\Cursor\Local State";

/// Cursor 当前的自带密钥配置，供界面和 CLI 展示。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorState {
    /// 库文件是否存在
    pub installed: bool,
    /// `Use OpenAI API Key` 开关。false 时自定义模型一定用不了
    pub use_openai_key: bool,
    /// 当前接口地址。`None` 表示没覆盖，走 OpenAI 官方
    pub base_url: Option<String>,
    /// 库里存着的密钥长度。0 表示没存过。**不返回密钥本身**
    pub key_len: usize,
    /// 密钥末尾后 4 位，方便用户核对是不是同一把 key
    pub key_tail: String,
    /// 已添加的自定义模型名
    pub user_added_models: Vec<String>,
    /// 库文件路径，界面直接显示，方便用户自己去看
    pub db_path: String,
}

/// 一次写入要做的事。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPlan {
    /// 要不要改 `useOpenAIKey`
    pub set_use_openai_key: Option<bool>,
    /// 要不要改 `openAIBaseUrl`；`Some("")` 表示清空成 null
    pub set_base_url: Option<String>,
    /// 要不要写密钥
    pub write_key: bool,
    /// 要加进模型选择器的名字。Cursor 会自己补目录条目，不用我们造
    pub add_models: Vec<String>,
    /// 要从模型选择器里删掉的名字
    pub remove_models: Vec<String>,
    /// 面向人的改动说明，一行一条
    pub summary: Vec<String>,
}

impl CursorPlan {
    /// 什么都不用做，等于当前配置已经和母版一致。
    pub fn is_noop(&self) -> bool {
        self.set_use_openai_key.is_none()
            && self.set_base_url.is_none()
            && !self.write_key
            && self.add_models.is_empty()
            && self.remove_models.is_empty()
    }

    /// 这次写入涉不涉及 `applicationUser` 里的 JSON。
    ///
    /// 只写密钥时不用碰那一大坨 JSON（540 KB），能省一次整条重写。
    fn touches_application_user(&self) -> bool {
        self.set_use_openai_key.is_some()
            || self.set_base_url.is_some()
            || !self.add_models.is_empty()
            || !self.remove_models.is_empty()
    }
}

/// Cursor 库和 `Local State` 的绝对路径。
///
/// 跟着 [`super::store::home_dir`] 走，这样测试时把 `AGENTCENTER_HOME`
/// 指到临时目录就能避开真实配置。
pub fn db_path() -> PathBuf {
    super::store::home_dir().join(DB_REL)
}

/// safeStorage 主密钥所在文件。
pub fn local_state_path() -> PathBuf {
    super::store::home_dir().join(LOCAL_STATE_REL)
}

/// Cursor 进程有没有在跑。
///
/// 写库前必须为 false。Cursor 把 `applicationUser` 整份 JSON 缓存在内存里，
/// 我们写进去的值会在它下次保存时被覆盖；更糟的是并发写 4 GB 的 SQLite
/// 有损坏风险。
pub fn is_running() -> bool {
    // 用 tasklist 而不是逐个枚举进程：这里只需要一个布尔值，
    // 调一次系统命令比引一整套 Win32 进程 API 简单得多
    let Ok(out) = crate::proc::output("tasklist", &["/FI", "IMAGENAME eq Cursor.exe", "/NH"], None)
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .to_lowercase()
        .contains("cursor.exe")
}

/// 读当前配置。
///
/// 库不存在时返回 `installed: false` 的全空结构，不报错 —— 「没装 Cursor」
/// 是正常情况，不是失败。
pub fn read_state() -> Result<CursorState> {
    let db = db_path();
    if !db.exists() {
        return Ok(CursorState {
            installed: false,
            use_openai_key: false,
            base_url: None,
            key_len: 0,
            key_tail: String::new(),
            user_added_models: Vec::new(),
            db_path: db.display().to_string(),
        });
    }

    // 只读打开。immutable 不能加：Cursor 开着时数据可能还在 WAL 里，
    // 加了会读到旧值
    let con = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("打不开 Cursor 库：{}", db.display()))?;

    let raw: Option<String> = con
        .query_row("select value from ItemTable where key = ?1", [APP_KEY], |r| {
            r.get(0)
        })
        .optional()
        .context("读 applicationUser 失败")?;

    let Some(raw) = raw else {
        bail!(
            "Cursor 库里没有 applicationUser 这个键，可能是版本变化。文件：{}",
            db.display()
        );
    };

    let user: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("applicationUser 不是合法 JSON，长度 {}", raw.len()))?;

    let use_openai_key = user
        .get("useOpenAIKey")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let base_url = user
        .get("openAIBaseUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let user_added_models = user
        .get("aiSettings")
        .and_then(|a| a.get("userAddedModels"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let key = read_key(&con).unwrap_or_default();
    let key_len = key.chars().count();
    let key_tail = if key_len >= 4 {
        key.chars().skip(key_len - 4).collect::<String>()
    } else {
        String::new()
    };

    Ok(CursorState {
        installed: true,
        use_openai_key,
        base_url,
        key_len,
        key_tail,
        user_added_models,
        db_path: db.display().to_string(),
    })
}

/// 用真实配置算出「要让这条 provider 生效，还差哪些改动」。
///
/// 只读，不写盘。界面上先显示这个，用户确认后才走 [`apply`]。
///
/// `want_models` 是希望出现在 Cursor 模型选择器里的名字。传空数组表示
/// 「不管模型」—— 此时不会做任何增删。这不是「清空模型」，因为母版里
/// 一条 provider 往往有好几个模型，用户可能只想加其中两个；要不要删
/// 必须由界面明确指定，不能靠空列表猜。
pub fn plan(provider_base_url: &str, provider_key: &str, want_models: &[String]) -> Result<CursorPlan> {
    let state = read_state()?;
    let mut plan = CursorPlan::default();

    if !state.installed {
        bail!(
            "没找到 Cursor 的配置库：{}。确认 Cursor 装过并且启动过一次",
            state.db_path
        );
    }

    // 开关必须打开。这是最常见的坑：只填了 key 没开开关
    if !state.use_openai_key {
        plan.set_use_openai_key = Some(true);
        plan.summary.push(
            "打开「Use OpenAI API Key」开关（关着时自定义模型名会被拒）".into(),
        );
    }

    // 地址要比对，末位斜杠不算差异
    let want = provider_base_url.trim().trim_end_matches('/');
    let have = state.base_url.as_deref().unwrap_or("").trim_end_matches('/');
    if want != have {
        plan.set_base_url = Some(want.to_string());
        plan.summary.push(format!(
            "接口地址 {} → {want}",
            if have.is_empty() { "（未设置）" } else { have }
        ));
    }

    // 密钥比对：长度和后 4 位都一致就认为是同一把，不重写
    let key_len = provider_key.chars().count();
    let tail: String = if key_len >= 4 {
        provider_key.chars().skip(key_len - 4).collect()
    } else {
        String::new()
    };
    if state.key_len != key_len || state.key_tail != tail {
        plan.write_key = true;
        plan.summary
            .push(format!("写入密钥（长度 {key_len}，末尾 …{tail}）"));
    }

    // 模型：want_models 里缺的补上。删由调用方通过 plan.remove_models 显式指定
    for m in want_models {
        let m = m.trim();
        if !m.is_empty() && !state.user_added_models.iter().any(|x| x == m) {
            plan.add_models.push(m.to_string());
        }
    }
    if !plan.add_models.is_empty() {
        plan.summary
            .push(format!("加进模型选择器：{}", plan.add_models.join(", ")));
    }

    Ok(plan)
}

/// 把计划写进 Cursor 库。
///
/// `allow_running` 为 false 时会再查一次 Cursor 进程 —— 调用方可能在上次
/// 检查之后又把 Cursor 打开了，所以这里是最后一道闸。设为 true 只该发生在
/// 「写的是副本」这种明确场景。
///
/// 不开事务包整条 `applicationUser` 的读改写：单条 update 本身就是原子的，
/// 而 JSON 序列化在内存里完成，中途失败不会留下半个值。
pub fn apply(plan: &CursorPlan, provider_key: &str, allow_running: bool) -> Result<Vec<String>> {
    if plan.is_noop() {
        return Ok(vec!["当前配置已经和母版一致，无需改动".into()]);
    }

    if !allow_running && is_running() {
        bail!("Cursor 正在运行，写库会被它覆盖。请先完全退出 Cursor 再重试");
    }

    let db = db_path();
    let con = Connection::open(&db)
        .with_context(|| format!("打不开 Cursor 库：{}", db.display()))?;

    let mut log = Vec::new();

    // ---- 1. 改 applicationUser ----
    if plan.touches_application_user() {
        let raw: String = con
            .query_row("select value from ItemTable where key = ?1", [APP_KEY], |r| {
                r.get(0)
            })
            .context("读 applicationUser 失败")?;
        let mut user: serde_json::Value =
            serde_json::from_str(&raw).context("applicationUser 不是合法 JSON")?;

        let Some(obj) = user.as_object_mut() else {
            bail!("applicationUser 不是 JSON 对象，结构可能变了");
        };

        if let Some(on) = plan.set_use_openai_key {
            obj.insert("useOpenAIKey".into(), serde_json::Value::Bool(on));
            log.push(format!("已设置 useOpenAIKey = {on}"));
        }

        if let Some(url) = &plan.set_base_url {
            // 空串等价于「不覆盖」，Cursor 自己的迁移逻辑也是这么处理的
            let value = if url.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(url.clone())
            };
            obj.insert("openAIBaseUrl".into(), value);
            log.push(format!("已设置 openAIBaseUrl = {url}"));
        }

        // 模型列表：写在 aiSettings.userAddedModels。
        //
        // 不用自己造 availableDefaultModels2 的条目 —— Cursor 的
        // handleAvailableModelsChange 在启动时会遍历 userAddedModels，
        // 把不在可用列表里的名字补进去（源码：
        // `for(const a of i) n.includes(a)||n.push(a)`）。
        // 我们只维护这个字符串数组就够了。
        if !plan.add_models.is_empty() || !plan.remove_models.is_empty() {
            let ai = obj
                .entry("aiSettings")
                .or_insert_with(|| serde_json::json!({}));
            let Some(ai_obj) = ai.as_object_mut() else {
                bail!("aiSettings 不是 JSON 对象，结构可能变了");
            };

            let existing: Vec<String> = ai_obj
                .get("userAddedModels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // 先删后加：用户可能想改大小写或换写法，顺序反了会互相打架
            let mut next: Vec<String> = existing
                .into_iter()
                .filter(|m| !plan.remove_models.iter().any(|r| r == m))
                .collect();
            for m in &plan.add_models {
                if !next.iter().any(|x| x == m) {
                    next.push(m.clone());
                }
            }

            if !plan.remove_models.is_empty() {
                log.push(format!(
                    "已从模型选择器移除：{}",
                    plan.remove_models.join(", ")
                ));
            }
            if !plan.add_models.is_empty() {
                log.push(format!(
                    "已加进模型选择器：{}",
                    plan.add_models.join(", ")
                ));
            }

            ai_obj.insert(
                "userAddedModels".into(),
                serde_json::Value::Array(
                    next.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }

        // 用紧凑格式写回，和 Cursor 自己的序列化方式一致
        let text = serde_json::to_string(&user).context("序列化 applicationUser 失败")?;
        con.execute(
            "update ItemTable set value = ?1 where key = ?2",
            rusqlite::params![text, APP_KEY],
        )
        .context("写 applicationUser 失败")?;
    }

    // ---- 2. 写密钥 ----
    if plan.write_key {
        let master = load_master_key()?;
        let blob = encrypt_secret(&master, provider_key)?;
        let value = serde_json::json!({ "type": "Buffer", "data": blob });

        let exists: bool = con
            .query_row(
                "select count(*) from ItemTable where key = ?1",
                [SECRET_KEY],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);

        if exists {
            con.execute(
                "update ItemTable set value = ?1 where key = ?2",
                rusqlite::params![value.to_string(), SECRET_KEY],
            )
            .context("更新密钥失败")?;
        } else {
            con.execute(
                "insert into ItemTable(key, value) values(?1, ?2)",
                rusqlite::params![SECRET_KEY, value.to_string()],
            )
            .context("插入密钥失败")?;
        }
        log.push("已写入密钥（safeStorage 加密）".into());
    }

    Ok(log)
}

/// 读出库里存着的密钥明文。读不到时返回空串。
///
/// 只用于「比对是不是同一把 key」和界面显示长度，不外传。
pub fn read_key(con: &Connection) -> Result<String> {
    let raw: Option<String> = con
        .query_row("select value from ItemTable where key = ?1", [SECRET_KEY], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(raw) = raw else {
        return Ok(String::new());
    };

    // 值可能是 JSON 对象 {type,data}，也可能是老版本的裸 base64 字符串
    let blob: Vec<u8> = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => v
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|n| n.as_u64().map(|x| x as u8))
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    if blob.is_empty() {
        return Ok(String::new());
    }

    match decrypt_secret(&blob) {
        Ok(text) => Ok(text),
        // 解不开不是错误：可能是别的机器/账户写的，或格式变了。
        // 上层只拿它比对长度，解不开就当没存
        Err(_) => Ok(String::new()),
    }
}

// ---- 加解密 ----

/// 读出 safeStorage 的 32 字节 AES 主密钥。
fn load_master_key() -> Result<Vec<u8>> {
    let p = local_state_path();
    let text = std::fs::read_to_string(&p)
        .with_context(|| format!("读不到 Cursor 的 Local State：{}", p.display()))?;
    let state: serde_json::Value =
        serde_json::from_str(&text).context("Local State 不是合法 JSON")?;

    let b64 = state
        .get("os_crypt")
        .and_then(|v| v.get("encrypted_key"))
        .and_then(|v| v.as_str())
        .context("Local State 里没有 os_crypt.encrypted_key")?;

    let raw = base64_decode(b64).context("encrypted_key 不是合法 base64")?;
    // 这 5 个字节是 Electron 加的固定前缀，剥掉再交给 DPAPI
    if raw.len() < 6 || &raw[..5] != b"DPAPI" {
        bail!("encrypted_key 前缀不是 DPAPI，Cursor 的存储方式可能变了");
    }

    dpapi_unprotect(&raw[5..]).context("DPAPI 解不开主密钥（可能不是当前用户写的）")
}

/// 按 Electron safeStorage 的格式加密：`v10` + 12 字节 nonce + 密文。
fn encrypt_secret(master: &[u8], plain: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if master.len() != 32 {
        bail!("主密钥长度应为 32 字节，实际 {}", master.len());
    }

    let cipher = Aes256Gcm::new_from_slice(master).map_err(|e| anyhow!("初始化 AES 失败：{e}"))?;
    let nonce_bytes = random_nonce()?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| anyhow!("加密失败：{e}"))?;

    let mut out = Vec::with_capacity(3 + 12 + ct.len());
    out.extend_from_slice(b"v10");
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 解开库里存着的密钥。格式不对或主密钥不匹配时返回错误。
fn decrypt_secret(blob: &[u8]) -> Result<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if blob.len() < 3 + 12 + 16 || &blob[..3] != b"v10" {
        bail!("密钥不是 v10 格式");
    }
    let master = load_master_key()?;
    let cipher = Aes256Gcm::new_from_slice(&master).map_err(|e| anyhow!("初始化 AES 失败：{e}"))?;
    let nonce = Nonce::from_slice(&blob[3..15]);
    let plain = cipher
        .decrypt(nonce, &blob[15..])
        .map_err(|_| anyhow!("AES-GCM 解密失败（主密钥不匹配或数据损坏）"))?;
    String::from_utf8(plain).context("密钥不是合法 UTF-8")
}

/// 生成 12 字节随机 nonce。用系统的 CSPRNG，不用时间戳之类可预测的东西。
fn random_nonce() -> Result<[u8; 12]> {
    let mut buf = [0u8; 12];
    getrandom_bytes(&mut buf)?;
    Ok(buf)
}

// ---- 平台相关 ----

/// 用 Windows DPAPI 解当前用户范围的密文。
#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    if cipher.is_empty() {
        bail!("DPAPI 输入为空");
    }

    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    // SAFETY: 两个 blob 都指向本函数内有效的内存；out_blob 由系统分配，
    // 成功时用 LocalFree 释放，失败时不分配。
    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 {
        bail!("CryptUnprotectData 返回失败，错误码 {}", std::io::Error::last_os_error());
    }

    // SAFETY: 上一步成功后 out_blob 指向 cbData 字节的有效缓冲区
    let out = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) }
        .to_vec();
    // SAFETY: 这块内存是系统分配的，必须交回系统释放
    unsafe {
        LocalFree(out_blob.pbData as *mut core::ffi::c_void);
    }
    Ok(out)
}

/// 非 Windows 上没有 DPAPI，Cursor 的这套存储也只在 Windows 上这么实现。
#[cfg(not(windows))]
fn dpapi_unprotect(_cipher: &[u8]) -> Result<Vec<u8>> {
    bail!("Cursor 的密钥加密依赖 Windows DPAPI，当前平台不支持")
}

/// 用系统 CSPRNG 填满 buf。
#[cfg(windows)]
fn getrandom_bytes(buf: &mut [u8]) -> Result<()> {
    use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    // SAFETY: buf 是本函数内有效的可写切片
    let status = unsafe {
        BCryptGenRandom(
            ptr_null(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        bail!("BCryptGenRandom 失败，NTSTATUS = {status:#x}");
    }
    Ok(())
}

#[cfg(windows)]
fn ptr_null() -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

#[cfg(not(windows))]
fn getrandom_bytes(buf: &mut [u8]) -> Result<()> {
    // 非 Windows 平台上这个模块本来就用不了，给个占位实现避免编译失败
    for (i, b) in buf.iter_mut().enumerate() {
        *b = i as u8;
    }
    Ok(())
}

/// 解 base64。自己写十几行，免得为一个函数拉一个 crate。
fn base64_decode(s: &str) -> Result<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    let mut n = 0;

    for b in bytes {
        if b == b'=' {
            break;
        }
        let Some(v) = val(b) else {
            bail!("base64 里有非法字符");
        };
        chunk[n] = v;
        n += 1;
        if n == 4 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
            out.push((chunk[2] << 6) | chunk[3]);
            n = 0;
        }
    }

    // 剩下 2 或 3 个字符时补出 1 或 2 个字节
    match n {
        2 => out.push((chunk[0] << 2) | (chunk[1] >> 4)),
        3 => {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        1 => bail!("base64 长度非法"),
        _ => {}
    }
    Ok(out)
}

/// 判断某个路径是不是 Cursor 的配置库，给界面提示用。
pub fn is_target(p: &Path) -> bool {
    p == db_path()
}

// ---- 给界面用的包装 ----

/// 读状态，转成界面契约类型。
///
/// 比 [`read_state`] 多做两件事：附带「Cursor 是否在运行」（界面要据此
/// 灰掉写入按钮），以及把 `Plan` 的 noop 判断提前算好 —— 前端不该自己
/// 去拼「三个字段都为空」这种逻辑。
pub fn state_view() -> Result<crate::types::CursorByokState> {
    let st = read_state()?;
    Ok(crate::types::CursorByokState {
        installed: st.installed,
        use_openai_key: st.use_openai_key,
        base_url: st.base_url,
        key_len: st.key_len,
        key_tail: st.key_tail,
        user_added_models: st.user_added_models,
        db_path: st.db_path,
        running: is_running(),
    })
}

/// 算出计划并转成界面契约类型。
pub fn plan_view(
    provider_base_url: &str,
    provider_key: &str,
    want_models: &[String],
) -> Result<crate::types::CursorByokPlan> {
    let p = plan(provider_base_url, provider_key, want_models)?;
    // 先算 noop 再搬字段：is_noop 要读 self，字段被 move 出去之后就不能调了
    let noop = p.is_noop();
    Ok(crate::types::CursorByokPlan {
        set_use_openai_key: p.set_use_openai_key,
        set_base_url: p.set_base_url,
        write_key: p.write_key,
        add_models: p.add_models,
        remove_models: p.remove_models,
        noop,
        summary: p.summary,
    })
}

/// 只删模型，不动开关/地址/密钥。
///
/// 界面上的「×」按钮走这个 —— 用户删一个模型不该顺带重写接口配置。
pub fn plan_remove_models(models: &[String]) -> CursorPlan {
    CursorPlan {
        remove_models: models.to_vec(),
        summary: models
            .iter()
            .map(|m| format!("从模型选择器移除 {m}"))
            .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip_matches_known_values() {
        // 用几个固定样本对标准实现
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
        assert_eq!(base64_decode("AAEC").unwrap(), vec![0, 1, 2]);
        // 带换行的也应该能解
        assert_eq!(base64_decode("aGVs\nbG8=").unwrap(), b"hello");
    }

    #[test]
    fn base64_rejects_bad_input() {
        assert!(base64_decode("****").is_err());
        assert!(base64_decode("a").is_err());
    }

    #[test]
    fn plan_summary_is_human_readable() {
        // 只验证结构行为，不碰真实文件
        let p = CursorPlan {
            set_use_openai_key: Some(true),
            set_base_url: Some("https://example.com/v1".into()),
            write_key: true,
            add_models: vec!["m1".into()],
            remove_models: Vec::new(),
            summary: vec!["x".into()],
        };
        assert!(!p.is_noop());
        assert!(p.touches_application_user());

        // 只写密钥时不该碰那一大坨 JSON
        let key_only = CursorPlan {
            write_key: true,
            ..Default::default()
        };
        assert!(!key_only.is_noop());
        assert!(!key_only.touches_application_user());

        // 只加模型也要碰 JSON
        let model_only = CursorPlan {
            add_models: vec!["m1".into()],
            ..Default::default()
        };
        assert!(model_only.touches_application_user());

        let empty = CursorPlan::default();
        assert!(empty.is_noop());
    }

    #[test]
    fn secret_format_is_v10_with_12_byte_nonce() {
        // 固定主密钥，验证打包出来的结构能被同一个密钥解回来
        let master = [7u8; 32];
        let blob = encrypt_secret(&master, "user_abcdef123456").unwrap();
        assert_eq!(&blob[..3], b"v10");
        assert!(blob.len() > 3 + 12 + 16);

        // 用同样的算法解开
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        let cipher = Aes256Gcm::new_from_slice(&master).unwrap();
        let plain = cipher
            .decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..])
            .unwrap();
        assert_eq!(String::from_utf8(plain).unwrap(), "user_abcdef123456");
    }

    #[test]
    fn wrong_master_key_cannot_decrypt() {
        let blob = encrypt_secret(&[1u8; 32], "secret-value").unwrap();

        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        let other = Aes256Gcm::new_from_slice(&[2u8; 32]).unwrap();
        assert!(other
            .decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..])
            .is_err());
    }
}
