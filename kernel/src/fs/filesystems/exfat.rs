// ============================================================
// exFAT - exFAT 文件系统驱动
// ============================================================
// 提供对 exFAT 格式磁盘的读写支持
//
// 架构设计：
// - 支持大于 4GB 的文件和分区
// - 支持长文件名（通过 Unicode 编码）
// - 支持集群链管理
// - 目前实现基本的导航和读取功能
//
// 为什么采用这样的设计：
// - exFAT 是 USB 移动设备的标准文件系统
// - 与 FAT32 相比，支持更大的文件和分区
// - 集群概念与 VFS 的 inode 需要适配
// - 分层实现纯算法部分（bitmap、FAT 表解析）与内核胶水（VFS 接口）

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use crate::lib::time::current_timestamp_secs;
use alloc::vec::Vec;

// ============================================================
// exFAT 引导扇区结构（简化模型）
// ============================================================

/// exFAT 引导扇区信息
///
/// 为什么使用结构体而非直接偏移量访问：
/// - 类型安全，编译期错误检测
/// - 便于单元测试和验证
/// - 跨平台字节序处理
#[derive(Debug, Clone, Copy)]
pub struct ExfatBootSector {
    /// 每簇字节数的 log2（例如 12 表示 4096 字节）
    pub cluster_size_bits: u32,
    /// FAT 表偏移（扇区数）
    pub fat_offset: u32,
    /// FAT 表大小（扇区数）
    pub fat_size: u32,
    /// 簇堆偏移（扇区数）
    pub cluster_heap_offset: u32,
    /// 总簇数
    pub total_clusters: u32,
}

// ============================================================
// exFAT 目录项定义
// ============================================================

/// exFAT 目录项类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExfatDirEntryType {
    /// 文件或目录
    Regular,
    /// 长文件名条目
    LongName,
    /// 分配位图
    Bitmap,
}

/// exFAT 目录项
#[derive(Debug, Clone)]
pub struct ExfatDirEntry {
    /// 项类型
    pub entry_type: ExfatDirEntryType,
    /// 簇起始位置
    pub start_cluster: u32,
    /// 文件大小（字节）
    pub size: u64,
    /// 创建时间（Unix 时间戳）
    pub created: i64,
    /// 修改时间（Unix 时间戳）
    pub modified: i64,
    /// 文件名（UTF-16 编码后的字节）
    pub name: Vec<u8>,
}

// ============================================================
// exFAT 文件系统核心
// ============================================================

/// exFAT 文件系统
pub struct Exfat {
    /// 引导扇区信息
    boot_sector: ExfatBootSector,
    /// 所有 inode 节点
    nodes: Vec<Option<ExfatInode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

/// exFAT inode 节点（映射 VFS inode）
struct ExfatInode {
    /// inode 号
    inode_number: InodeNumber,
    /// 对应的 exFAT 目录项
    dir_entry: ExfatDirEntry,
    /// 元数据
    metadata: InodeMetadata,
}

impl Exfat {
    /// 创建新的 exFAT 实例
    ///
    /// 为什么返回 KernelResult：
    /// - 磁盘可能损坏或不是有效的 exFAT 格式
    /// - 初始化可能失败（内存不足等）
    /// - 将来可以集成块设备驱动来读取真实磁盘
    pub fn new() -> KernelResult<Self> {
        // TODO: 从块设备读取引导扇区并验证签名
        // 临时使用默认配置
        let boot_sector = ExfatBootSector {
            cluster_size_bits: 12,      // 4096 字节/簇
            fat_offset: 1,
            fat_size: 100,
            cluster_heap_offset: 101,
            total_clusters: 10000,
        };

        let mut exfat = Exfat {
            boot_sector,
            nodes: Vec::new(),
            next_ino: 2,
        };

        // 预留 inode 0/1
        exfat.nodes.push(None);
        exfat.nodes.push(None);

        // 创建根目录 inode（暂为空）
        // TODO: 从磁盘读取根目录内容
        let now = current_timestamp_secs();
        let root_dir_entry = ExfatDirEntry {
            entry_type: ExfatDirEntryType::Regular,
            start_cluster: 2,
            size: 0,
            created: now,
            modified: now,
            name: b"/".to_vec(),
        };

        exfat.nodes.push(Some(ExfatInode {
            inode_number: 2,
            dir_entry: root_dir_entry,
            metadata: InodeMetadata {
                inode_number: 2,
                file_type: FileType::Directory,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o755),
                uid: 0,
                gid: 0,
                nlink: 2,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }));

        Ok(exfat)
    }

    /// 获取 inode 节点
    fn get_inode(&self, ino: InodeNumber) -> Option<&ExfatInode> {
        self.nodes.get(ino as usize)?.as_ref()
    }

    /// 将簇号转换为字节偏移
    fn cluster_to_offset(&self, cluster: u32) -> u64 {
        let cluster_size = 1u64 << self.boot_sector.cluster_size_bits;
        let offset_sectors = self.boot_sector.cluster_heap_offset as u64
            + (cluster as u64 - 2) * (cluster_size / 512);
        offset_sectors * 512
    }
}

impl FileSystem for Exfat {
    fn name(&self) -> &'static str {
        "exfat"
    }

    fn mount(&self) -> KernelResult<InodeNumber> {
        Ok(2)  // 根目录 inode
    }

    fn unmount(&self) -> KernelResult<()> {
        // TODO: 刷新 FAT 表并关闭
        Ok(())
    }

    fn lookup(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<InodeNumber> {
        // TODO: 在目录中查找文件名
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn root_inode(&self) -> InodeNumber {
        2
    }

    fn stat(&self, ino: InodeNumber) -> KernelResult<InodeMetadata> {
        self.get_inode(ino)
            .map(|inode| inode.metadata)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)
    }

    fn read(&self, _ino: InodeNumber, _offset: u64, _buf: &mut [u8]) -> KernelResult<usize> {
        // TODO: 从簇链读���文件内容
        // 需要集成块设备驱动
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn write(&self, _ino: InodeNumber, _offset: u64, _data: &[u8]) -> KernelResult<usize> {
        // TODO: 写入文件内容
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<Vec<DirectoryEntry>> {
        let _inode = self.get_inode(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        // TODO: 读取目录内容
        // 临时返回空列表
        let mut entries = Vec::new();
        entries.push(DirectoryEntry::new(b".", ino, FileType::Directory)?);
        entries.push(DirectoryEntry::new(b"..", ino, FileType::Directory)?);
        Ok(entries)
    }

    fn create(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        // TODO: 创建新文件
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn mkdir(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        // TODO: 创建目录
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn unlink(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        // TODO: 删除文件
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn rmdir(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        // TODO: 删除目录
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn symlink(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _target: &[u8],
    ) -> KernelResult<InodeNumber> {
        // exFAT 本质上不支持符号链接
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readlink(&self, _ino: InodeNumber, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exfat_creation() {
        let exfat = Exfat::new().unwrap();
        assert_eq!(exfat.root_inode(), 2);
    }

    #[test]
    fn test_cluster_to_offset() {
        let exfat = Exfat::new().unwrap();
        // 簇 2 应该在 cluster_heap_offset 处
        let offset = exfat.cluster_to_offset(2);
        assert_eq!(offset, exfat.boot_sector.cluster_heap_offset as u64 * 512);
    }

    #[test]
    fn test_exfat_stat_root() {
        let exfat = Exfat::new().unwrap();
        let stat = exfat.stat(2).unwrap();
        assert_eq!(stat.file_type, FileType::Directory);
        assert_eq!(stat.mode.0, 0o755);
    }
}
