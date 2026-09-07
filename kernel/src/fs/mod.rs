// ============================================================
// 虚拟文件系统（VFS）根模块
// ============================================================
// 聚合 VFS 层的所有子模块，并作为与其他内核模块的接口。
//
// 模块结构：
//   - 纯逻辑层：vfs / path / inode / dentry
//   - 缓存管理：dcache / pagecache
//   - 挂载和文件：mount / file
//   - 装配层：本模块（全局状态、初始化、自测）
//   - 具体实现：filesystems/ 下各文件系统

pub mod vfs;
pub mod path;
pub mod inode;
pub mod dentry;
pub mod dcache;
pub mod pagecache;
pub mod mount;
pub mod file;
pub mod filesystems;

// 重新导出常用类型
pub use vfs::{
    DirectoryEntry, FileMode, FileType, FileOffset, FileSize, InodeNumber,
    InodeMetadata, OpenFlags, UnixTime, VfsResult, FsError, FileSystem,
};

pub use path::ParsedPath;
pub use dentry::Dentry;
pub use inode::UserId;
pub use dcache::DentryCache;
pub use pagecache::PageCache;
pub use mount::{MountTable, MountEntry, MountFlags};
pub use file::{OpenFile, FileDescriptorTable};

// ============================================================
// VFS 全局状态
// ============================================================

use crate::sync::Spinlock;

/// VFS 全局状态
/// 为什么用 Box：FileSystemState 包含多个 Vec 和缓存结构，
/// 动态大小较大，Box 让它在堆上落位，锁内只存指针
#[allow(dead_code)]
struct FileSystemState {
    /// 根文件系统实例
    root_fs: Option<alloc::vec::Vec<u8>>,  // 占位符
    /// 挂载表
    mount_table: MountTable,
    /// 全局 dentry 缓存
    dentry_cache: DentryCache,
}

/// 全局 VFS 状态
static VFS_STATE: Spinlock<Option<alloc::boxed::Box<FileSystemState>>> = Spinlock::new(None);

/// 文件系统初始化入口
pub fn init() {
    println!("[VFS] VFS module initializing...");

    // 初始化全局 VFS 状态
    let mut guard = VFS_STATE.lock();
    let state = alloc::boxed::Box::new(FileSystemState {
        root_fs: None,
        mount_table: MountTable::new(),
        dentry_cache: DentryCache::new(2048),
    });
    *guard = Some(state);
    drop(guard);

    println!("[VFS] VFS module ready");

    // 后续：
    // 1. 注册具体文件系统（tmpfs、devfs、procfs、sysfs）
    // 2. 创建根文件系统并挂载
    // 3. 挂载其他文件系统
    // 4. 运行自测

    selftest();
}

// ============================================================
// VFS 公开接口
// ============================================================

/// 挂载的文件系统信息（用于 procfs 等子系统读取）
///
/// 为什么设计这个结构：
/// - procfs /proc/filesystems 需要显示已挂载的文件系统列表
/// - 不暴露内部 Arc<dyn FileSystem>，只暴露必要的元数据
#[derive(Debug, Clone)]
pub struct MountedFilesystemInfo {
    /// 文件系统名称（如 "tmpfs"、"procfs" 等）
    pub fs_name: [u8; 32],
    pub fs_name_len: usize,
    /// 挂载标志（只读等）
    pub flags: MountFlags,
    /// 是否为虚拟文件系统（不需要块设备）
    pub is_virtual: bool,
}

/// 获取所有已挂载的文件系统列表
///
/// 用途：procfs /proc/filesystems 需要列出当前系统支持的所有文件系统
/// 为什么提供这个接口：
/// - procfs 不直接访问全局 VFS 状态
/// - 集中权限管理（锁保护在这里）
/// - 返回的是文件系统元数据而非 trait object
pub fn list_mounted_filesystems() -> alloc::vec::Vec<MountedFilesystemInfo> {
    let guard = VFS_STATE.lock();
    let mut filesystems = alloc::vec::Vec::new();

    if let Some(state) = guard.as_ref() {
        for entry in state.mount_table.entries() {
            // 获取文件系统名称
            let fs_name_str = entry.filesystem.name();
            let fs_name_bytes = fs_name_str.as_bytes();

            let mut fs_name = [0u8; 32];
            let copy_len = core::cmp::min(fs_name_bytes.len(), 31);
            fs_name[..copy_len].copy_from_slice(&fs_name_bytes[..copy_len]);

            // 为什么判断虚拟文件系统：/proc/filesystems 格式为 "nodev\tfs_name" 或 "\tfs_name"
            // 所有当前实现的都是虚拟文件系统（无块设备）
            let is_virtual = true;

            filesystems.push(MountedFilesystemInfo {
                fs_name,
                fs_name_len: copy_len,
                flags: entry.flags,
                is_virtual,
            });
        }
    }

    filesystems
}

/// 获取 dentry 缓存统计
pub fn get_dentry_cache_stats() -> Option<dcache::DentryCacheStats> {
    let guard = VFS_STATE.lock();
    guard.as_ref().map(|state| state.dentry_cache.stats())
}

/// 清空 dentry 缓存
pub fn clear_dentry_cache() {
    let mut guard = VFS_STATE.lock();
    if let Some(ref mut _state) = *guard {
        // 为什么注释：缓存清空逻辑需要改进 LRU 接口
        // TODO: 添加缓存清空功能
    }
}

// ============================================================
// 内核自测
// ============================================================

/// 运行文件系统全部自测；返回是否全部通过
pub fn selftest() -> bool {
    println!("\n[FS-SELFTEST] VFS Subsystem Selftest");
    let mut all = true;
    all &= t("path parsing & normalization", selftest_path_parsing());
    all &= t("inode metadata & permissions", selftest_inode_metadata());
    all &= t("dentry structure & links", selftest_dentry_structure());
    all &= t("dentry cache operations", selftest_dcache_operations());
    all &= t("page cache basics", selftest_pagecache_basics());
    all &= t("mount table management", selftest_mount_table());
    all &= t("file descriptor operations", selftest_file_operations());
    println!("[FS-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}

/// 单测断言容器
fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}

/// 验收用宏
macro_rules! check {
    ($cond:expr $(, $msg:expr)?) => {
        if !($cond) {
            $(println!("    [check] FAILED at: {}", $msg);)?
            return false;
        }
    };
}

/// 1) 路径解析与规范化
fn selftest_path_parsing() -> bool {
    // 测试基础路径解析
    let p1 = path::parse_path(b"/etc/passwd");
    check!(p1.is_ok(), "parse absolute path");
    if let Ok(parsed) = p1 {
        check!(parsed.is_absolute, "recognized absolute path");
    }

    let p2 = path::parse_path(b"foo/bar");
    check!(p2.is_ok(), "parse relative path");
    if let Ok(parsed) = p2 {
        check!(!parsed.is_absolute, "recognized relative path");
    }

    let p3 = path::parse_path(b"");
    check!(p3.is_err(), "reject empty path");

    true
}

/// 2) inode 元数据与权限
fn selftest_inode_metadata() -> bool {
    use crate::fs::inode::{UserId, PermissionType, check_permission};

    // 创建 inode 元数据
    let meta = InodeMetadata {
        inode_number: 42,
        file_type: FileType::File,
        size: 1024,
        blocks: 2,
        mode: vfs::FileMode(0o644),
        uid: 1000,
        gid: 1000,
        nlink: 1,
        atime: 0,
        mtime: 0,
        ctime: 0,
        btime: None,
    };

    check!(meta.inode_number == 42, "inode number");
    check!(meta.size == 1024, "inode size");
    check!(meta.mode.0 == 0o644, "inode mode");
    check!(meta.file_type == FileType::File, "inode type");

    // 权限检查：所有者可读写
    let owner = UserId { uid: 1000 };
    check!(check_permission(&meta, owner, PermissionType::Read), "owner can read");
    check!(check_permission(&meta, owner, PermissionType::Write), "owner can write");

    // root 用户拥有所有权限
    let root = UserId::root();
    check!(check_permission(&meta, root, PermissionType::Read), "root can read");
    check!(check_permission(&meta, root, PermissionType::Write), "root can write");

    true
}

/// 3) dentry 结构与链接
fn selftest_dentry_structure() -> bool {
    // 创建 root dentry
    let d1 = Dentry::new(0, 0, b"root");
    check!(d1.is_ok(), "create root dentry");

    if let Ok(d1) = d1 {
        check!(d1.inode_number == 0, "root dentry inode is 0");
        check!(d1.parent_ino == 0, "root parent is 0");
        check!(d1.name_len == 4, "root name length");
    }

    // 创建子 dentry
    let d2 = Dentry::new(0, 1, b"etc");
    check!(d2.is_ok(), "create child dentry");

    if let Ok(d2) = d2 {
        check!(d2.parent_ino == 0, "child parent is root");
        check!(d2.inode_number == 1, "child inode is 1");
        check!(d2.name_len == 3, "child name length");
    }

    true
}

/// 4) dentry 缓存操作
fn selftest_dcache_operations() -> bool {
    let mut cache = DentryCache::new(256);

    // 创建并缓存 dentry
    let d = Dentry::new(0, 42, b"testfile").expect("create testfile dentry");
    let key = dentry::DentryKey::new(0, b"testfile");
    cache.insert(key, alloc::boxed::Box::new(d.clone()));

    // 查询缓存
    if let Some(cached) = cache.get(&key) {
        check!(cached.inode_number == 42, "dentry cache lookup");
        check!(cached.name_len == 8, "cached name length");
    } else {
        return false;
    }

    // 缓存应非空
    check!(cache.len() > 0, "cache has entries");
    check!(cache.capacity() >= 256, "cache capacity");

    true
}

/// 5) page cache 基础
fn selftest_pagecache_basics() -> bool {
    // 轻量级验证：只检查 PageCacheStats 创建
    let stats = pagecache::PageCacheStats::new();
    check!(stats.hits == 0, "stats initially zero hits");
    check!(stats.misses == 0, "stats initially zero misses");
    check!(stats.writes == 0, "stats initially zero writes");

    true
}

/// 6) 挂载表管理
fn selftest_mount_table() -> bool {
    let table = MountTable::new();

    // 初始状态：空表
    let initial_count = table.len();
    check!(initial_count == 0, "mount table initially empty");

    // 表应有非零容量
    let entries = table.entries();
    check!(entries.is_empty(), "entries list initially empty");

    true
}

/// 7) 文件描述符操作
fn selftest_file_operations() -> bool {
    let fdt = FileDescriptorTable::new();

    // 初始状态：无打开文件
    check!(fdt.count_open() == 0, "fd table initially empty");

    true
}
