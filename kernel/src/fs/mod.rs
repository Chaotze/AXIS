// ============================================================
// 虚拟文件系统（VFS）根模块
// ============================================================
// 聚合 VFS 层的所有子模块，并作为与其他内核模块的接口。
//
// 模块结构：
//   - 纯逻辑层：vfs / path / inode / dentry
//   - 缓存管理：dcache / pagecache
//   - 装配层：本模块（全局状态、初始化、自测）
//   - 具体实现：filesystems/ 下各文件系统

pub mod vfs;
pub mod path;
pub mod inode;
pub mod dentry;

// 后续模块
// pub mod dcache;
// pub mod pagecache;
// pub mod mount;
// pub mod file;
// pub mod filesystems;

// 系统调用接口
// pub mod syscall;

// 重新导出常用类型
pub use vfs::{
    DirectoryEntry, FileMode, FileType, FileOffset, FileSize, InodeNumber,
    InodeMetadata, OpenFlags, UnixTime, VfsResult, FsError, FileSystem,
};

pub use path::ParsedPath;
pub use dentry::Dentry;
pub use inode::UserId;

/// 文件系统初始化入口
pub fn init() {
    println!("[VFS] VFS module initializing...");

    // 后续：
    // 1. 初始化缓存管理（dcache、pagecache）
    // 2. 注册具体文件系统（tmpfs、devfs、procfs、sysfs）
    // 3. 创建全局挂载表
    // 4. 挂载根文件系统
    // 5. 运行自测

    println!("[VFS] VFS module ready");
}

// ============================================================
// 内核自测
// ============================================================

fn selftest() -> bool {
    println!("\n[VFS-SELFTEST] VFS Subsystem Selftest");
    let all = true;

    // 后续添加测试项：
    // - VFS trait 编译验证
    // - tmpfs 挂载和文件操作
    // - 路径解析
    // - dentry 缓存
    // - 页缓存
    // - 权限检查

    println!("[VFS-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}
