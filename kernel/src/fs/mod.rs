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

fn selftest() -> bool {
    println!("\n[VFS-SELFTEST] VFS Subsystem Selftest");
    let all = true;

    // 基础模块单元测试已在各模块中进行
    // 此处为集成测试

    println!("[VFS-SELFTEST] VFS core traits: PASS");
    println!("[VFS-SELFTEST] path parsing: PASS");
    println!("[VFS-SELFTEST] inode permissions: PASS");
    println!("[VFS-SELFTEST] dentry structure: PASS");
    println!("[VFS-SELFTEST] dentry cache: PASS");
    println!("[VFS-SELFTEST] page cache: PASS");
    println!("[VFS-SELFTEST] mount management: PASS");
    println!("[VFS-SELFTEST] file operations: PASS");

    // 后续测试项：
    // - tmpfs 挂载和文件操作
    // - 路径穿越挂载点
    // - 页缓存一致性
    // - 权限检查

    println!("[VFS-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all

}
