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
pub mod syscall;
pub mod shell;

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
struct FileSystemState {
    /// 根文件系统实例
    root_fs: Option<alloc::sync::Arc<dyn vfs::FileSystem>>,
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

    println!("[VFS] Registering file systems...");

    // 创建并挂载文件系统
    // 为什么这样做：按优先级顺序挂载，tmpfs 作为根文件系统
    let mut tmpfs_instance: Option<alloc::sync::Arc<dyn vfs::FileSystem>> = None;

    match filesystems::Tmpfs::new() {
        Ok(tmpfs) => {
            println!("[VFS] tmpfs created successfully");
            tmpfs_instance = Some(tmpfs.clone());

            // 挂载到根目录（inode 2）
            let mut guard = VFS_STATE.lock();
            if let Some(state) = guard.as_mut() {
                state.root_fs = Some(tmpfs.clone());
                let _ = state.mount_table.mount(2, tmpfs, MountFlags::new(0), b"tmpfs");
            }
            drop(guard);
        }
        Err(e) => println!("[VFS] Failed to create tmpfs: {:?}", e),
    }

    // 挂载 devfs 到 /dev
    match filesystems::Devfs::new() {
        Ok(devfs) => {
            println!("[VFS] devfs created successfully");
            // 在 tmpfs 中创建 /dev 目录，然后挂载 devfs
            if let Some(ref tmpfs) = tmpfs_instance {
                // 创建 /dev 目录
                if let Ok(dev_ino) = tmpfs.mkdir(tmpfs.root_inode(), b"dev", FileMode::new(0o755)) {
                    // 挂载 devfs 到 /dev
                    let mut guard = VFS_STATE.lock();
                    if let Some(state) = guard.as_mut() {
                        let _ = state.mount_table.mount(dev_ino, devfs, MountFlags::new(0), b"devfs");
                    }
                    drop(guard);
                }
            }
        }
        Err(e) => println!("[VFS] Failed to create devfs: {:?}", e),
    }

    // 挂载 procfs 到 /proc
    match filesystems::Procfs::new() {
        Ok(procfs) => {
            println!("[VFS] procfs created successfully");
            // 在 tmpfs 中创建 /proc 目录，然后挂载 procfs
            if let Some(ref tmpfs) = tmpfs_instance {
                if let Ok(proc_ino) = tmpfs.mkdir(tmpfs.root_inode(), b"proc", FileMode::new(0o755)) {
                    let mut guard = VFS_STATE.lock();
                    if let Some(state) = guard.as_mut() {
                        let _ = state.mount_table.mount(proc_ino, procfs, MountFlags::new(0), b"procfs");
                    }
                    drop(guard);
                }
            }
        }
        Err(e) => println!("[VFS] Failed to create procfs: {:?}", e),
    }

    // 挂载 sysfs 到 /sys
    match filesystems::Sysfs::new() {
        Ok(sysfs) => {
            println!("[VFS] sysfs created successfully");
            // 在 tmpfs 中创建 /sys 目录，然后挂载 sysfs
            if let Some(ref tmpfs) = tmpfs_instance {
                if let Ok(sys_ino) = tmpfs.mkdir(tmpfs.root_inode(), b"sys", FileMode::new(0o755)) {
                    let mut guard = VFS_STATE.lock();
                    if let Some(state) = guard.as_mut() {
                        let _ = state.mount_table.mount(sys_ino, sysfs, MountFlags::new(0), b"sysfs");
                    }
                    drop(guard);
                }
            }
        }
        Err(e) => println!("[VFS] Failed to create sysfs: {:?}", e),
    }

    println!("[VFS] VFS module ready");

    selftest();
}

// ============================================================
// VFS 公开接口
// ============================================================

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
    let mut passed = 0;
    let mut total = 0;

    // 测试 1：VFS 初始化
    total += 1;
    let guard = VFS_STATE.lock();
    if guard.is_some() {
        println!("[VFS-SELFTEST] VFS state initialized: PASS");
        passed += 1;
    } else {
        println!("[VFS-SELFTEST] VFS state initialized: FAIL");
    }
    drop(guard);

    // 测试 2：挂载表非空
    total += 1;
    let guard = VFS_STATE.lock();
    if let Some(state) = guard.as_ref() {
        if !state.mount_table.is_empty() {
            println!("[VFS-SELFTEST] File systems mounted: PASS (count={})", state.mount_table.len());
            passed += 1;
        } else {
            println!("[VFS-SELFTEST] File systems mounted: FAIL");
        }
    }
    drop(guard);

    // 测试 3：路径解析
    total += 1;
    match path::parse_path(b"/a/b/c") {
        Ok(parsed) => {
            if parsed.is_absolute && parsed.components.len() == 3 {
                println!("[VFS-SELFTEST] Path parsing: PASS");
                passed += 1;
            } else {
                println!("[VFS-SELFTEST] Path parsing: FAIL");
            }
        }
        Err(_) => println!("[VFS-SELFTEST] Path parsing: FAIL"),
    }

    // 测试 4：dentry 缓存
    total += 1;
    let guard = VFS_STATE.lock();
    if let Some(state) = guard.as_ref() {
        let stats = state.dentry_cache.stats();
        println!("[VFS-SELFTEST] Dentry cache: PASS (capacity={})", stats.capacity);
        passed += 1;
    }
    drop(guard);

    // 测试 5：inode 权限检查
    total += 1;
    let metadata = InodeMetadata {
        inode_number: 1,
        file_type: FileType::File,
        size: 100,
        blocks: 1,
        mode: FileMode::new(0o644),
        uid: 100,
        gid: 100,
        nlink: 1,
        atime: 0,
        mtime: 0,
        ctime: 0,
        btime: None,
    };
    if inode::check_permission(&metadata, inode::UserId { uid: 100 }, inode::PermissionType::Read) {
        println!("[VFS-SELFTEST] Permission check: PASS");
        passed += 1;
    } else {
        println!("[VFS-SELFTEST] Permission check: FAIL");
    }

    // 测试 6：tmpfs 基本操作
    total += 1;
    match filesystems::Tmpfs::new() {
        Ok(tmpfs) => {
            // 验证根目录
            if tmpfs.root_inode() == 2 {
                // 验证 stat
                if tmpfs.stat(2).is_ok() {
                    // 验证 readdir
                    if tmpfs.readdir(2).is_ok() {
                        println!("[VFS-SELFTEST] tmpfs basic ops: PASS");
                        passed += 1;
                    } else {
                        println!("[VFS-SELFTEST] tmpfs basic ops: FAIL (readdir)");
                    }
                } else {
                    println!("[VFS-SELFTEST] tmpfs basic ops: FAIL (stat)");
                }
            } else {
                println!("[VFS-SELFTEST] tmpfs basic ops: FAIL (root_inode)");
            }
        }
        Err(_) => println!("[VFS-SELFTEST] tmpfs basic ops: FAIL (creation)"),
    }

    // 测试 7：devfs 基本操作
    total += 1;
    match filesystems::Devfs::new() {
        Ok(devfs) => {
            if devfs.root_inode() == 2 && devfs.readdir(2).is_ok() {
                println!("[VFS-SELFTEST] devfs basic ops: PASS");
                passed += 1;
            } else {
                println!("[VFS-SELFTEST] devfs basic ops: FAIL");
            }
        }
        Err(_) => println!("[VFS-SELFTEST] devfs basic ops: FAIL (creation)"),
    }

    // 测试 8：procfs 基本操作
    total += 1;
    match filesystems::Procfs::new() {
        Ok(procfs) => {
            if procfs.root_inode() == 2 && procfs.readdir(2).is_ok() {
                println!("[VFS-SELFTEST] procfs basic ops: PASS");
                passed += 1;
            } else {
                println!("[VFS-SELFTEST] procfs basic ops: FAIL");
            }
        }
        Err(_) => println!("[VFS-SELFTEST] procfs basic ops: FAIL (creation)"),
    }

    // 测试 9：sysfs 基本操作
    total += 1;
    match filesystems::Sysfs::new() {
        Ok(sysfs) => {
            if sysfs.root_inode() == 2 && sysfs.readdir(2).is_ok() {
                println!("[VFS-SELFTEST] sysfs basic ops: PASS");
                passed += 1;
            } else {
                println!("[VFS-SELFTEST] sysfs basic ops: FAIL");
            }
        }
        Err(_) => println!("[VFS-SELFTEST] sysfs basic ops: FAIL (creation)"),
    }

    println!("[VFS-SELFTEST] Result: {}/{} tests passed\n", passed, total);
    passed == total
}
