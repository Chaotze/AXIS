// ============================================================
// exFAT - exFAT 文件系统驱动
// ============================================================
// 提供对 exFAT 格式磁盘的读写支持
//
// 架构设计：
// - 支持大于 4GB 的文件和分区
// - 支持长文件名（通过 Unicode 编码）
// - 支持集群链管理
//
// 实现状态：
// ✅ 完成：
//   - 引导扇区解析框架
//   - 目录项数据结构
//   - inode 节点映射
//   - VFS 接口实现
//   - 单元测试框架
//
// ⏳ 待完成（需要块设备驱动层）：
//   - 磁盘 I/O 操作（等待块设备驱动集成）
//   - FAT 表解析和遍历（需要读取磁盘 FAT 表）
//   - 文件内容读写（需要块设备层的 read/write）
//   - 目录条目搜索（需要磁盘 I/O）
//   - 文件创建和删除（需要 FAT 表更新和磁盘写入）

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use crate::lib::time::current_timestamp_secs;
use alloc::vec::Vec;

// ============================================================
// exFAT 引导扇区结构
// ============================================================

/// exFAT 引导扇区信息
#[derive(Debug, Clone, Copy)]
pub struct ExfatBootSector {
    pub cluster_size_bits: u32,
    pub fat_offset: u32,
    pub fat_size: u32,
    pub cluster_heap_offset: u32,
    pub total_clusters: u32,
}

// ============================================================
// exFAT 目录项定义
// ============================================================

/// exFAT 目录项类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExfatDirEntryType {
    Regular,
    LongName,
    Bitmap,
}

/// exFAT 目录项
#[derive(Debug, Clone)]
pub struct ExfatDirEntry {
    pub entry_type: ExfatDirEntryType,
    pub start_cluster: u32,
    pub size: u64,
    pub created: i64,
    pub modified: i64,
    pub name: Vec<u8>,
}

// ============================================================
// exFAT 文件系统核心
// ============================================================

/// exFAT 文件系统
pub struct Exfat {
    boot_sector: ExfatBootSector,
    nodes: Vec<Option<ExfatInode>>,
    next_ino: InodeNumber,
}

/// exFAT inode 节点
struct ExfatInode {
    inode_number: InodeNumber,
    dir_entry: ExfatDirEntry,
    metadata: InodeMetadata,
}

impl Exfat {
    pub fn new() -> KernelResult<Self> {
        let boot_sector = ExfatBootSector {
            cluster_size_bits: 12,
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

        exfat.nodes.push(None);
        exfat.nodes.push(None);

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

    fn get_inode(&self, ino: InodeNumber) -> Option<&ExfatInode> {
        self.nodes.get(ino as usize)?.as_ref()
    }

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
        Ok(2)
    }

    fn unmount(&self) -> KernelResult<()> {
        Ok(())
    }

    fn lookup(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<InodeNumber> {
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
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn write(&self, _ino: InodeNumber, _offset: u64, _data: &[u8]) -> KernelResult<usize> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<Vec<DirectoryEntry>> {
        let _inode = self.get_inode(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

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
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn mkdir(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn unlink(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn rmdir(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn symlink(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _target: &[u8],
    ) -> KernelResult<InodeNumber> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readlink(&self, _ino: InodeNumber, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }
}

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
