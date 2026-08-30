// ============================================================
// 挂载点管理（Mount）
// ============================================================
// 管理文件系统的挂载表，支持多个文件系统同时挂载。
// 实现路径穿越挂载点的逻辑。

use crate::fs::vfs::{FileSystem, InodeNumber};
use crate::lib::result::KernelResult;
use alloc::sync::Arc;

// ============================================================
// 挂载点结构
// ============================================================

/// 单个挂载项
/// 为什么分离挂载信息：
/// - 支持同一个文件系统在多个位置挂载
/// - 跟踪挂载点的元数据（挂载时间、选项等）
#[derive(Clone)]
pub struct MountEntry {
    /// 此文件系统挂载的父 inode 号
    /// 为什么是 inode 号而非路径：inode 号更稳定，不随路径改变
    pub mount_point_ino: InodeNumber,
    /// 文件系统实例
    pub filesystem: Arc<dyn FileSystem>,
    /// 挂载标志（只读等）
    pub flags: MountFlags,
    /// 挂载点的名称（通常是最后一个路径组件）
    pub name: [u8; 256],
    pub name_len: usize,
}

impl MountEntry {
    /// 获取挂载点的名称切片
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 获取挂载点的名称字符串（假设 UTF-8）
    pub fn name_str(&self) -> Option<&str> {
        core::str::from_utf8(self.name()).ok()
    }
}

/// 挂载标志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountFlags(pub u32);

impl MountFlags {
    /// 只读挂载
    pub const MS_RDONLY: u32 = 1;
    /// 禁用执行
    pub const MS_NOEXEC: u32 = 8;
    /// 禁用 setuid/setgid
    pub const MS_NOSUID: u32 = 2;
    /// 禁用设备访问
    pub const MS_NODEV: u32 = 4;

    pub fn new(flags: u32) -> Self {
        MountFlags(flags)
    }

    pub fn is_readonly(&self) -> bool {
        (self.0 & Self::MS_RDONLY) != 0
    }

    pub fn is_noexec(&self) -> bool {
        (self.0 & Self::MS_NOEXEC) != 0
    }

    pub fn is_nosuid(&self) -> bool {
        (self.0 & Self::MS_NOSUID) != 0
    }

    pub fn is_nodev(&self) -> bool {
        (self.0 & Self::MS_NODEV) != 0
    }
}

// ============================================================
// 挂载表
// ============================================================

/// 全局挂载表
/// 为什么使用 Vec：
/// - 挂载点数量通常较少（几十个以内）
/// - 简单易维护
/// - 不需要频繁查询（大多数查询通过 dentry cache）
pub struct MountTable {
    /// 挂载项列表
    entries: alloc::vec::Vec<MountEntry>,
}

impl MountTable {
    /// 创建新的挂载表
    pub fn new() -> Self {
        MountTable {
            entries: alloc::vec::Vec::new(),
        }
    }

    /// 挂载文件系统
    /// 为什么检查重复挂载：防止在同一位置挂载多个文件系统
    pub fn mount(
        &mut self,
        mount_point_ino: InodeNumber,
        filesystem: Arc<dyn FileSystem>,
        flags: MountFlags,
        name: &[u8],
    ) -> KernelResult<()> {
        // 为什么检查名称长度：255 是 POSIX 标准限制
        if name.len() > 255 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 为什么检查重复：同一位置不应该有多个挂载
        if self.find_mount(mount_point_ino).is_some() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut name_arr = [0u8; 256];
        name_arr[..name.len()].copy_from_slice(name);

        self.entries.push(MountEntry {
            mount_point_ino,
            filesystem,
            flags,
            name: name_arr,
            name_len: name.len(),
        });

        Ok(())
    }

    /// 卸载文件系统
    pub fn unmount(&mut self, mount_point_ino: InodeNumber) -> KernelResult<()> {
        if let Some(idx) = self.entries.iter().position(|e| e.mount_point_ino == mount_point_ino) {
            self.entries.remove(idx);
            Ok(())
        } else {
            Err(crate::lib::result::KernelError::InvalidArgument)
        }
    }

    /// 查找在指定 inode 号处挂载的文件系统
    /// 为什么需要这个函数：
    /// - 路径解析时需要检查是否穿越了挂载点
    pub fn find_mount(&self, mount_point_ino: InodeNumber) -> Option<Arc<dyn FileSystem>> {
        for entry in &self.entries {
            if entry.mount_point_ino == mount_point_ino {
                return Some(entry.filesystem.clone());
            }
        }
        None
    }

    /// 获取所有挂载项
    pub fn entries(&self) -> &[MountEntry] {
        &self.entries
    }

    /// 获取挂载项数量
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 检查是否有挂载点
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空挂载表
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// 获取根文件系统
    /// 为什么需要这个函数：
    /// - 路径解析通常从根文件系统开始
    /// - 根文件系统通常首先挂载
    pub fn get_root_filesystem(&self) -> Option<Arc<dyn FileSystem>> {
        // 为什么假设 inode 2 是根目录：这是 Unix/Linux 的惯例
        for entry in &self.entries {
            if entry.mount_point_ino == 2 {
                return Some(entry.filesystem.clone());
            }
        }

        // 如果没有明确的根挂载点，返回第一个挂载项
        if !self.entries.is_empty() {
            Some(self.entries[0].filesystem.clone())
        } else {
            None
        }
    }
}

// ============================================================
// 挂载点穿越逻辑
// ============================================================

/// 检查在给定的 inode 号处是否有挂载点
/// 如果有，返回新的文件系统和该文件系统的根 inode 号
pub fn traverse_mount(
    mount_table: &MountTable,
    current_ino: InodeNumber,
) -> Option<(Arc<dyn FileSystem>, InodeNumber)> {
    if let Some(fs) = mount_table.find_mount(current_ino) {
        // 为什么获取根 inode：穿越挂载点后需要使用新文件系统的根
        let root_ino = fs.root_inode();
        Some((fs, root_ino))
    } else {
        None
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // 为了测试，我们需要一个模拟的 FileSystem 实现
    // 这里仅做基本的挂载表测试

    #[test]
    fn test_mount_table_creation() {
        let table = MountTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn test_mount_flags() {
        let flags = MountFlags::new(MountFlags::MS_RDONLY | MountFlags::MS_NOEXEC);
        assert!(flags.is_readonly());
        assert!(flags.is_noexec());
        assert!(!flags.is_nosuid());
    }

    #[test]
    fn test_mount_entry_name() {
        let mut name_arr = [0u8; 256];
        name_arr[..5].copy_from_slice(b"tmpfs");

        let entry = MountEntry {
            mount_point_ino: 1,
            filesystem: unsafe {
                // 这是不安全的，仅用于测试
                // 实际中需要真实的 FileSystem 实现
                core::mem::zeroed()
            },
            flags: MountFlags::new(0),
            name: name_arr,
            name_len: 5,
        };

        assert_eq!(entry.name(), b"tmpfs");
        assert_eq!(entry.name_str(), Some("tmpfs"));
    }
}
