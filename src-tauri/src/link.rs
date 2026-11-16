//! 链接引擎：把母版文件映射到各 agent 目录，并检测健康状态。
//!
//! 这是整个工具里唯一有破坏性的模块，所以三条规则贯穿始终：
//!
//! 1. **不删除任何用户数据**。已存在的真实文件先移进
//!    `~/.agents/backup/<时间戳>/`，而不是删除或直接覆盖。
//! 2. **降级链每一级都明确报告**。符号链接失败不是静默换成副本，
//!    而是记录实际用了哪种方式并在界面上显示。
//! 3. **幂等**。连续跑两次，第二次必须零改动。
//!
//! Windows 权限背景：建符号链接需要管理员权限或开启开发者模式；
//! 建 junction（目录连接）和硬链接不需要任何权限。所以降级顺序是
//! 符号链接 → junction/硬链接 → 副本，前两级就能覆盖绝大多数情况。

use crate::config::ResolvedMapping;
use crate::types::{HealthState, LinkKind, LinkMode, Mapping};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// 计算文件的 SHA-256，返回十六进制字符串。
///
/// 用于比对母版与副本内容是否一致，以及记录 copy 模式下的指纹。
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

/// 计算一段字节的 SHA-256。
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 计算一个目录的内容指纹。
///
/// 算法：把目录下所有文件按相对路径排序，逐个把「相对路径 + 文件内容」
/// 拼进哈希。这样目录内任何一个文件改名或改内容都会导致指纹变化。
///
/// 跳过 `.git` 目录 —— 版本控制元数据和内容无关，且体量大。
pub fn sha256_dir(path: &Path) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    let mut entries = Vec::new();

    for entry in walkdir::WalkDir::new(path).into_iter().filter_entry(|e| {
        // 跳过 .git，但不要跳过根目录本身
        e.depth() == 0 || e.file_name() != ".git"
    }) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(path)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            entries.push((rel, entry.path().to_path_buf()));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, full) in entries {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        if let Ok(bytes) = fs::read(&full) {
            hasher.update(&bytes);
        }
        hasher.update(b"\0");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// 备份目录：`~/.agents/backup/<时间戳>/`。
fn backup_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agents")
        .join("backup")
}

/// 把已存在的路径移进备份目录，返回备份后的位置。
///
/// 用时间戳分子目录，避免同一秒内多次备份互相覆盖；
/// 如果时间戳目录已存在就加序号后缀。
fn move_to_backup(path: &Path) -> anyhow::Result<PathBuf> {
    let dest = new_backup_slot(path)?;
    fs::rename(path, &dest)?;
    Ok(dest)
}

/// 把一个目录复制进备份目录，返回备份后的位置。**原目录保留**。
///
/// 和 [`move_to_backup`] 的区别是跨卷安全：目录被 rename 到另一个卷
/// 会直接失败，而用户放进备份的目录常常和 `~/.agents` 不在一个盘上。
/// 复制成功后由调用方决定删不删原目录。
pub fn backup_dir_copy(path: &Path) -> anyhow::Result<PathBuf> {
    let dest = new_backup_slot(path)?;
    copy_dir_recursive(path, &dest)?;
    Ok(dest)
}

/// 在备份目录里给一个路径腾出位置，返回备份目标路径。
///
/// 只建目录、算路径，不搬东西 —— 文件和目录的搬运方式不同
/// （目录跨卷 rename 会失败），所以留给调用方决定怎么搬。
fn new_backup_slot(path: &Path) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut dir = backup_root().join(&stamp);
    let mut n = 1;
    while dir.exists() {
        dir = backup_root().join(format!("{stamp}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&dir)?;

    let file_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "unknown".into());
    Ok(dir.join(file_name))
}

/// 判断一个路径是不是链接（符号链接或 junction）。
///
/// 用 `symlink_metadata` 看文件类型：链接本身是个"特殊文件"，
/// 它和 `metadata` 指向的目标是两回事。
///
/// **注意硬链接不在此列**。硬链接在文件系统层面就是同一个 inode 的
/// 另一个名字，没有任何重解析点标记，`is_symlink()` 对它是 false。
/// 所以检测硬链接必须走 [`is_same_inode`] 或按内容比对。
pub fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// 读取链接指向的目标。不是链接时返回 None。
pub fn read_link_target(path: &Path) -> Option<PathBuf> {
    fs::read_link(path).ok()
}

/// 检测一个映射当前的健康状态和实际链接方式。
///
/// 这个函数只读，不改动任何文件，可以安全地在界面上频繁调用。
pub fn inspect(m: &ResolvedMapping) -> Mapping {
    let installed = m.config_dir.exists();
    let source_exists = m.source.exists();

    let mut mapping = Mapping {
        agent: m.agent.clone(),
        label: m.label.clone(),
        source: crate::config::contract_tilde(&m.source),
        target: crate::config::contract_tilde(&m.target),
        mode: m.mode,
        actual: None,
        health: HealthState::Missing,
        installed,
        disabled: m.disabled,
        note: None,
    };

    if !source_exists {
        mapping.note = Some("母版文件不存在".to_string());
        mapping.health = HealthState::Broken;
        return mapping;
    }

    if m.disabled {
        // 已取消的映射：不再诊断链接健康度，只报告目标是否已清掉。
        // 用户主动关掉的东西不该在界面上一直报红。
        mapping.actual = if is_link(&m.target) {
            Some(detect_link_kind(&m.target))
        } else {
            None
        };
        mapping.note = Some(if m.target.exists() || is_link(&m.target) {
            "已取消映射，但目标位置的链接还在，可点「取消映射」清掉".to_string()
        } else {
            "已取消，同步时跳过".to_string()
        });
        return mapping;
    }

    if !m.target.exists() && !is_link(&m.target) {
        // 目标完全没有
        return mapping;
    }

    // 目标是链接：检查它指向哪里、目标还在不在
    if is_link(&m.target) {
        let actual = detect_link_kind(&m.target);
        mapping.actual = Some(actual);

        match read_link_target(&m.target) {
            Some(dest) => {
                if !dest.exists() {
                    mapping.health = HealthState::Broken;
                    mapping.note = Some("链接指向的目标不存在".to_string());
                } else if same_file(&dest, &m.source) {
                    mapping.health = HealthState::Ok;
                } else {
                    // 链接指向别处，也算需要修复
                    mapping.health = HealthState::Drifted;
                    mapping.note = Some(format!("链接指向 {}", dest.display()));
                }
            }
            None => {
                mapping.health = HealthState::Broken;
                mapping.note = Some("无法读取链接目标".to_string());
            }
        }
        return mapping;
    }

    // 目标是真实文件（不是符号链接/junction）。这里还要再分两种情况：
    // 是不是与母版同一个 inode 的硬链接，还是独立副本。
    // 只看内容是否一致是不够的 —— 硬链接和"内容相同的副本"在
    // 行为上完全不同：前者改一处两处都变，后者改完就漂移。
    match (sha256_file(&m.target), sha256_file(&m.source)) {
        (Ok(t), Ok(s)) => {
            let hardlinked = is_same_inode(&m.target, &m.source);
            if hardlinked {
                mapping.health = HealthState::Ok;
                mapping.actual = Some(LinkKind::Hardlink);
                mapping.note = Some("与母版是同一份文件（硬链接）".to_string());
            } else if t == s {
                mapping.health = HealthState::Ok;
                mapping.actual = Some(LinkKind::Copy);
                mapping.note = Some("独立副本，内容与母版一致".to_string());
            } else {
                mapping.health = HealthState::Drifted;
                mapping.actual = Some(LinkKind::Copy);
                mapping.note = Some("独立副本，内容与母版不同".to_string());
            }
        }
        _ => {
            mapping.health = HealthState::Broken;
            mapping.note = Some("无法读取文件内容".to_string());
        }
    }

    mapping
}

/// 判断两个路径是否指向同一个文件。
///
/// 分三种情况，顺序很重要：
/// 1. 规范化后是同一路径 —— 同一个文件
/// 2. 同一个 inode —— 互为硬链接，内容必然相同
/// 3. 内容哈希相同 —— 两份独立副本，内容恰好一致
///
/// 第 2 步是必需的：硬链接不是 reparse point，
/// `is_symlink()` 对它是 false，只靠路径和内容判断会把
/// 硬链接误认成「内容相同的两份副本」。
fn same_file(a: &Path, b: &Path) -> bool {
    if let (Ok(ca), Ok(cb)) = (fs::canonicalize(a), fs::canonicalize(b)) {
        if ca == cb {
            return true;
        }
    }
    if is_same_inode(a, b) {
        return true;
    }
    match (sha256_file(a), sha256_file(b)) {
        (Ok(ha), Ok(hb)) => ha == hb,
        _ => false,
    }
}

/// 判断两个路径是否指向同一个 inode（即互为硬链接）。
///
/// Windows 上 std 的 `MetadataExt::file_index` 还是 nightly 特性，
/// 所以直接调 `GetFileInformationByHandle` 拿 BY_HANDLE_FILE_INFORMATION
/// 里的 nFileIndexHigh/Low + dwVolumeSerialNumber。
/// 用已有的 windows-sys 依赖，不额外引库，也不起子进程。
///
/// Unix 上用 dev + ino，零成本。
#[cfg(windows)]
fn is_same_inode(a: &Path, b: &Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    /// 拿一个路径的 (卷序列号, 文件索引)。失败返回 None。
    fn identity(path: &Path) -> Option<(u32, u64)> {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        // 以"只读 + 允许共享"打开。0 表示只读访问。
        // FILE_FLAG_BACKUP_SEMANTICS 让目录也能用这种方式打开。
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;

        let handle = file.as_raw_handle();
        // SAFETY: handle 由上面的 File 持有，在本函数作用域内有效；
        // info 是栈上零初始化的结构体，长度由类型保证。
        // File 离开作用域时会自己关闭句柄，这里不需要 CloseHandle。
        unsafe {
            let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            let ok = GetFileInformationByHandle(handle as _, &mut info);
            if ok == 0 {
                return None;
            }
            let idx = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
            Some((info.dwVolumeSerialNumber, idx))
        }
    }

    match (identity(a), identity(b)) {
        (Some(ia), Some(ib)) => ia == ib,
        _ => false,
    }
}

#[cfg(not(windows))]
fn is_same_inode(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

/// 判断一个链接的实际类型。
///
/// Windows 上 junction 和符号链接在 `symlink_metadata` 里都表现为
/// 重解析点，需要用目标是否可写、以及路径语义来区分。这里用
/// `read_link` 的返回值形态做近似判断：junction 的目标总是绝对路径，
/// 且 junction 只能指向目录。
fn detect_link_kind(path: &Path) -> LinkKind {
    #[cfg(windows)]
    {
        // 目录链接在 Windows 上通常是 junction
        if path.is_dir() {
            return LinkKind::Junction;
        }
        return LinkKind::Symlink;
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        LinkKind::Symlink
    }
}

/// 创建一个链接，按 mode 与降级链决定实际方式。
///
/// 返回实际使用的链接方式。失败时把每一级的错误都记进 message 便于诊断。
pub fn create_link(
    target: &Path,
    source: &Path,
    mode: LinkMode,
) -> anyhow::Result<(LinkKind, String)> {
    let is_dir = source.is_dir();

    // 用户显式指定了某一种，就不降级，直接试那一种
    if mode != LinkMode::Auto {
        let kind = match mode {
            LinkMode::Symlink => LinkKind::Symlink,
            LinkMode::Junction => LinkKind::Junction,
            LinkMode::Hardlink => LinkKind::Hardlink,
            LinkMode::Copy => LinkKind::Copy,
            LinkMode::Auto => unreachable!(),
        };
        let note = apply(kind, target, source, is_dir)?;
        return Ok((kind, note));
    }

    // auto：按降级链依次尝试
    let mut errors = Vec::new();

    // 第一级：目录用符号链接，文件也用符号链接
    match apply(LinkKind::Symlink, target, source, is_dir) {
        Ok(note) => return Ok((LinkKind::Symlink, note)),
        Err(e) => errors.push(format!("符号链接：{e}")),
    }

    // 第二级：目录用 junction，文件用硬链接。两者都不需要权限
    let fallback = if is_dir {
        LinkKind::Junction
    } else {
        LinkKind::Hardlink
    };
    match apply(fallback, target, source, is_dir) {
        Ok(note) => return Ok((fallback, note)),
        Err(e) => errors.push(format!("{fallback:?}：{e}")),
    }

    // 第三级：副本。这是最后手段，靠 manifest 指纹检测漂移
    match apply(LinkKind::Copy, target, source, is_dir) {
        Ok(note) => return Ok((LinkKind::Copy, note)),
        Err(e) => errors.push(format!("副本：{e}")),
    }

    anyhow::bail!("所有链接方式都失败：\n  {}", errors.join("\n  "))
}

/// 按指定方式创建链接或副本。
fn apply(kind: LinkKind, target: &Path, source: &Path, is_dir: bool) -> anyhow::Result<String> {
    // 目标已存在的情况由调用方负责清理，这里只做创建
    match kind {
        LinkKind::Symlink => {
            #[cfg(windows)]
            {
                if is_dir {
                    std::os::windows::fs::symlink_dir(source, target)?;
                } else {
                    std::os::windows::fs::symlink_file(source, target)?;
                }
            }
            #[cfg(not(windows))]
            {
                let _ = is_dir;
                std::os::unix::fs::symlink(source, target)?;
            }
            Ok("已创建符号链接".to_string())
        }
        LinkKind::Junction => {
            #[cfg(windows)]
            {
                if !is_dir {
                    anyhow::bail!("junction 只能用于目录");
                }
                // 用 mklink /J 而不是引第三方 crate。
                // 理由：junction 的创建就是一条稳定的系统命令，
                // 引一个只为这一件事的依赖不值得，而且路径含空格时
                // 由 std 的参数转义处理比手拼命令行更安全。
                //
                // 走 crate::proc 而不是裸 Command：主程序没有控制台，
                // 直接起 cmd 会让系统给这个子进程新建一个黑窗口。
                let mut cmd = crate::proc::new("cmd");
                let status = cmd
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(target)
                    .arg(source)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("mklink /J 返回失败状态 {:?}", status.code());
                }
                Ok("已创建目录连接（junction，无需权限）".to_string())
            }
            #[cfg(not(windows))]
            {
                let _ = (target, source);
                anyhow::bail!("junction 只在 Windows 上可用");
            }
        }
        LinkKind::Hardlink => {
            if is_dir {
                anyhow::bail!("硬链接只能用于文件");
            }
            // std::fs::hard_link 跨平台可用，且不需要特权
            fs::hard_link(source, target)?;
            Ok("已创建硬链接（无需权限）".to_string())
        }
        LinkKind::Copy => {
            if is_dir {
                copy_dir_recursive(source, target)?;
                Ok("已复制目录（最后手段，需靠指纹检测漂移）".to_string())
            } else {
                fs::copy(source, target)?;
                Ok("已复制文件（最后手段，需靠指纹检测漂移）".to_string())
            }
        }
    }
}

/// 递归复制目录。
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 让一个映射生效：清理旧目标，建新链接。
///
/// 返回 `(实际链接方式, 说明)`。旧目标若是真实文件会被移进备份目录。
pub fn sync_one(m: &ResolvedMapping) -> anyhow::Result<(LinkKind, String)> {
    if !m.source.exists() {
        anyhow::bail!("母版文件不存在：{}", m.source.display());
    }
    if !m.config_dir.exists() {
        anyhow::bail!("agent 配置目录不存在：{}", m.config_dir.display());
    }

    // 目标目录必须存在，但不要替用户创建 agent 的配置根目录
    if let Some(parent) = m.target.parent() {
        if !parent.exists() {
            anyhow::bail!("目标目录不存在：{}", parent.display());
        }
    }

    let mut backup_note = String::new();

    // 清理已存在的目标。分四种情况，顺序有讲究：
    // 1. 符号链接/junction → 看指向哪
    // 2. 与母版同一 inode 的硬链接 → 已经是最好的状态，直接跳过
    // 3. 真实文件且内容与母版相同 → 删掉重建链接（为了拿到更"活"的形式）
    // 4. 真实文件但内容不同 → 备份后重建
    if is_link(&m.target) {
        // 已是指向母版的链接，且健康 —— 直接跳过，保证幂等
        if let Some(dest) = read_link_target(&m.target) {
            if dest.exists() && same_file(&dest, &m.source) {
                let kind = detect_link_kind(&m.target);
                return Ok((kind, "已是最新，跳过".to_string()));
            }
        }
        // 断链或指向别处：删掉链接本身，不碰目标
        fs::remove_file(&m.target).or_else(|_| fs::remove_dir(&m.target))?;
    } else if m.target.exists() {
        // 已经是硬链接就无需再动。删掉重建反而会打断现有关系。
        if is_same_inode(&m.target, &m.source) {
            return Ok((LinkKind::Hardlink, "已是最新，跳过".to_string()));
        }

        // 真实文件：内容与母版相同就直接替换成链接，不同则先备份
        let same = match (sha256_file(&m.target), sha256_file(&m.source)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if same {
            fs::remove_file(&m.target)?;
        } else {
            let dest = move_to_backup(&m.target)?;
            backup_note = format!("；原文件已备份到 {}", dest.display());
        }
    }

    let (kind, note) = create_link(&m.target, &m.source, m.mode)?;
    Ok((kind, format!("{note}{backup_note}")))
}

/// 取消一个映射：删掉目标位置上的链接或副本。
///
/// 边界与 [`sync_one`] 对称 —— 只动「映射建出来的东西」，母版一律不碰：
///
/// - 符号链接 / junction：只删链接本身，指向的内容原地不动
/// - 与母版同一 inode 的硬链接：删掉这个目录项，母版那一份仍在
/// - 内容与母版一致的副本：删掉，母版有同一份
/// - 内容与母版不一致的真实文件：**不直接删**，移进备份目录后报告位置
/// - 目标本来就不存在：算成功，保证重复点两次不报错
///
/// 只删文件不动配置。`disabled` 标记由 commands 层写进配置文件，
/// 否则下一次同步会立刻把它建回来。
pub fn remove_one(m: &ResolvedMapping) -> anyhow::Result<String> {
    // 目标不存在，也没有悬空的链接 → 已经是取消状态
    if !m.target.exists() && !is_link(&m.target) {
        return Ok("目标本来就不存在，无需处理".to_string());
    }

    // 符号链接 / junction：只删链接，不碰目标内容
    if is_link(&m.target) {
        let dest = read_link_target(&m.target);
        fs::remove_file(&m.target).or_else(|_| fs::remove_dir(&m.target))?;
        return Ok(match dest {
            Some(d) => format!("已删除链接，指向的内容保留在 {}", d.display()),
            None => "已删除链接".to_string(),
        });
    }

    // 与母版同一 inode 的硬链接：删目录项即可，母版那一份不受影响
    if is_same_inode(&m.target, &m.source) {
        fs::remove_file(&m.target)?;
        return Ok("已删除硬链接，母版文件不受影响".to_string());
    }

    // 剩下的都是真实文件或目录。内容与母版一致才敢直接删。
    let same = if m.target.is_dir() {
        match (sha256_dir(&m.target), sha256_dir(&m.source)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    } else {
        match (sha256_file(&m.target), sha256_file(&m.source)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    };

    if same {
        if m.target.is_dir() {
            fs::remove_dir_all(&m.target)?;
        } else {
            fs::remove_file(&m.target)?;
        }
        return Ok("副本内容与母版一致，已删除".to_string());
    }

    // 内容不同或被用户改过：不删，移进备份目录
    if m.target.is_dir() {
        let dest = backup_dir_copy(&m.target)?;
        fs::remove_dir_all(&m.target)?;
        return Ok(format!(
            "目标内容与母版不同，已移到备份 {}",
            dest.display()
        ));
    }

    let dest = move_to_backup(&m.target)?;
    Ok(format!("目标内容与母版不同，已移到备份 {}", dest.display()))
}
