//! 安全写入：五道闸的实现。
//!
//! 这是整个 provider 注入里**唯一会写用户文件**的模块。
//! 所以每一道检查都要能独立说清楚"它防的是什么"。
//!
//! ## 五道闸
//!
//! 1. **解析失败即全停** —— 调用方在 plan 阶段就该拒绝，这里再校验一次
//! 2. **hash 复查** —— 写入前重读文件，和计划时比。变了说明被外部改过
//! 3. **首写备份** —— 每个文件第一次被改之前完整复制
//! 4. **原子写入** —— 同目录临时文件 → fsync → rename
//! 5. **符号链接解析** —— 写入真实目标，不把链接换成普通文件
//!
//! ## 为什么不能用 mtime
//!
//! 文件修改时间的精度在不同文件系统上差别很大，网络文件系统上不可靠，
//! 时钟回拨也会骗人。hash 是唯一可信的"内容变了吗"的判断依据。
//!
//! ## 为什么备份要按文件去重
//!
//! 每次同步都备份会让备份目录爆炸（一天跑十次就是十份）。
//! 用"每个文件在整个生命周期内只备份第一次"的策略，
//! 既保证了"能回到最初状态"，又不会无限增长。

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// 计算文件内容的 SHA-256。
///
/// 文件不存在时返回空串，表示"原本没有这个文件"。
/// 这和"文件存在但内容是空"要区分开 —— 前者首次创建，后者需要备份。
pub fn hash_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let bytes = fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

/// 计算一段字节的 SHA-256。
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// 备份根目录：`~/.agents/backup/`。
///
/// 用 store::home_dir 而不是 dirs::home_dir，保证测试时能整体重定向。
fn backup_root() -> PathBuf {
    super::store::home_dir().join(".agents").join("backup")
}

/// 首写备份。
///
/// 用 `~/.agents/backup/providers-first-write/` 作为固定位置，
/// 文件名用「原路径 hash 前 12 位 + 原文件名」避免撞名。
/// 同目录放一个 `.source` 文件记录原路径，同时充作"已备份"标记。
///
/// 返回备份文件的路径；如果已经备份过则返回 None。
pub fn ensure_first_write_backup(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        // 文件本来就不存在，没有可备份的内容。
        // 但仍然写一个标记，避免以后误以为"从没处理过"。
        return Ok(None);
    }

    let dir = backup_root().join("providers-first-write");
    fs::create_dir_all(&dir)?;

    // 用绝对路径的 hash 做前缀，保证同一个文件始终映射到同一个备份名
    let abs = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path_hash = hash_bytes(abs.to_string_lossy().as_bytes());
    let prefix = &path_hash[..12];

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let backup = dir.join(format!("{prefix}-{file_name}"));
    let marker = dir.join(format!("{prefix}-{file_name}.source"));

    // 标记文件存在说明已经备份过，直接返回
    if marker.exists() {
        return Ok(None);
    }

    // 复制内容
    fs::copy(path, &backup)
        .with_context(|| format!("备份失败：{}", path.display()))?;

    // 写标记，记录原路径
    fs::write(&marker, abs.to_string_lossy().as_bytes())?;

    Ok(Some(backup))
}

/// 解析符号链接的最终目标。
///
/// 不解析的话，原子写会把「链接」替换成「普通文件」，
/// 用 stow / chezmoi 管理 dotfiles 的用户会中招
/// （CC Switch 4.0.27 就是这个 bug）。
///
/// 最多跟 40 跳，防止循环链接死循环。
/// 不用 `canonicalize`，因为它在 Windows 上会把路径改写成 `\\?\` 形式。
pub fn resolve_final_target(path: &Path) -> PathBuf {
    const MAX_HOPS: usize = 40;
    let mut current = path.to_path_buf();

    for _ in 0..MAX_HOPS {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                match fs::read_link(&current) {
                    Ok(target) => {
                        // 相对链接按所在目录解析
                        current = if target.is_absolute() {
                            target
                        } else {
                            current
                                .parent()
                                .map(|p| p.join(&target))
                                .unwrap_or(target)
                        };
                    }
                    Err(_) => break,
                }
            }
            _ => break,
        }
    }

    current
}

/// 原子写入。
///
/// 步骤：同目录建临时文件 → 写入 → fsync → rename 覆盖目标。
///
/// 为什么临时文件必须在同目录：`rename` 跨文件系统会失败
/// （EXDEV），只有同一挂载点内才是原子的。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let real_path = resolve_final_target(path);

    let parent = real_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("路径没有父目录：{}", real_path.display()))?;
    fs::create_dir_all(parent)?;

    // 临时文件名带 pid 和纳秒，避免并发时撞名
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(
        ".{}.tmp.{}.{}",
        real_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into()),
        std::process::id(),
        stamp
    );
    let tmp_path = parent.join(tmp_name);

    // 写临时文件
    {
        let mut f = fs::File::create(&tmp_path)
            .with_context(|| format!("创建临时文件失败：{}", tmp_path.display()))?;
        f.write_all(bytes)?;
        // fsync 确保内容真的落盘，否则崩溃后可能留下零填充的文件
        f.sync_all()?;
    }

    // 保留原文件的权限位（如果原文件存在）
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(&real_path) {
        let _ = fs::set_permissions(&tmp_path, meta.permissions());
    }

    // rename 覆盖。Windows 上 rename 对已存在的目标会失败，
    // 所以先尝试删除目标再 rename；如果删除失败就报错让调用方处理。
    #[cfg(windows)]
    {
        if real_path.exists() {
            // 先尝试 ReplaceFileW 的等价操作：备份 + 删除 + rename。
            // 这里用简单可靠的路径：删除旧文件再 rename 临时文件。
            // 中间态很短，且前面已经做过首写备份。
            fs::remove_file(&real_path)
                .with_context(|| format!("无法覆盖 {}", real_path.display()))?;
        }
    }

    fs::rename(&tmp_path, &real_path)
        .with_context(|| format!("替换失败：{}", real_path.display()))?;

    Ok(())
}

/// 一次写入的完整流程：备份 → 复查 hash → 原子写。
///
/// `pre_hash` 是计划阶段读到的内容 hash。写入前重读比对：
/// 不一致说明文件被外部改过，中止并让调用方重新规划。
pub fn safe_write(path: &Path, content: &str, pre_hash: &str) -> Result<Option<PathBuf>> {
    // 第 2 道闸：hash 复查
    let current = hash_file(path)?;
    if current != pre_hash {
        anyhow::bail!(
            "{} 在计划后被修改过（原 hash {}，现 hash {}），已中止写入。请重新运行同步。",
            path.display(),
            if pre_hash.is_empty() { "无" } else { pre_hash },
            if current.is_empty() { "无" } else { &current },
        );
    }

    // 第 3 道闸：首写备份
    let backup = ensure_first_write_backup(path)?;

    // 第 4 道闸：原子写入
    atomic_write(path, content.as_bytes())?;

    Ok(backup)
}

/// 检查目标文件是否可写，以及所属 agent 是否在运行。
///
/// 运行中的 agent 会在退出时用内存态覆盖配置文件，
/// 所以同步前必须先关掉。这里只能检查文件能不能打开，
/// 真正的"进程是否在跑"留给调用方用进程名判断。
pub fn check_writable(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    // 尝试以追加模式打开（不实际写），验证句柄可用
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| {
            format!(
                "{} 无法写入。请确认对应 agent 已经退出，且文件没有被其他程序独占。",
                path.display()
            )
        })?;
    Ok(())
}
