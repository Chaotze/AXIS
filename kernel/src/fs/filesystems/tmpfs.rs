// ============================================================
// tmpfs - 临时内存文件系统
// ============================================================
// 最简单的文件系统实现，完全基于内存。
// 用于测试 VFS 框架和作为根文件系统。

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use alloc::vec::Vec;

// ============================================================
// tmpfs 内存节点
// ============================================================

/// tmpfs 中的文件/目录节点
pub struct TmpfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 文件类型
    file_type: FileType,
    /// 文件内容（仅用于普通文件）
    content: Vec<u8>,
    /// 子项（仅用于目录）
    children: Vec<(Vec<u8>, InodeNumber)>,  // (name, child_ino)
    /// 元数据
    metadata: InodeMetadata,
}

impl TmpfsNode {
    /// 创建新节点
    fn new(inode_number: InodeNumber, file_type: FileType, mode: FileMode) -> Self {
        let now = 0i64;  // 简化：使用固定时间戳

        TmpfsNode {
            inode_number,
            file_type,
            content: Vec::new(),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type,
                size: 0,
                blocks: 0,
                mode,
                uid: 0,
                gid: 0,
                nlink: if file_type == FileType::Directory { 2 } else { 1 },
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 检查是否为目录
    fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }
}

// ============================================================
// tmpfs 文件系统实现
// ============================================================

/// tmpfs 文件系统
pub struct Tmpfs {
    /// 所有 inode（按 inode 号索引）
    nodes: Vec<Option<TmpfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

impl Tmpfs {
    /// 创建新的 tmpfs 实例
    pub fn new() -> KernelResult<Self> {
        let mut tmpfs = Tmpfs {
            nodes: Vec::new(),
            next_ino: 2,  // inode 0/1 保留
        };

        // 创建根目录（inode 2）
        let root = TmpfsNode::new(2, FileType::Directory, FileMode::new(0o755));
        tmpfs.nodes.push(None);  // inode 0
        tmpfs.nodes.push(None);  // inode 1
        tmpfs.nodes.push(Some(root));
        tmpfs.next_ino = 3;

        Ok(tmpfs)
    }

    /// 根据 inode 号获取节点
    fn get_node(&self, ino: InodeNumber) -> Option<&TmpfsNode> {
        self.nodes.get(ino as usize)?.as_ref()
    }

    /// 根据 inode 号获取可变节点
    fn get_node_mut(&mut self, ino: InodeNumber) -> Option<&mut TmpfsNode> {
        self.nodes.get_mut(ino as usize)?.as_mut()
    }

    /// 分配新 inode
    fn allocate_inode(&mut self, file_type: FileType, mode: FileMode) -> KernelResult<InodeNumber> {
        let ino = self.next_ino;
        let node = TmpfsNode::new(ino, file_type, mode);
        self.nodes.push(Some(node));
        self.next_ino += 1;
        Ok(ino)
    }

    /// 在目录中查找子项
    fn find_child(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
        let parent = self.get_node(parent_ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !parent.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        for (child_name, child_ino) in &parent.children {
            if child_name == name {
                return Ok(*child_ino);
            }
        }

        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    /// 在目录中添加子项
    fn add_child(&mut self, parent_ino: InodeNumber, name: &[u8], child_ino: InodeNumber) -> KernelResult<()> {
        let parent = self.get_node_mut(parent_ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !parent.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        parent.children.push((name.to_vec(), child_ino));
        Ok(())
    }
}

impl FileSystem for Tmpfs {
    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn mount(&self) -> KernelResult<InodeNumber> {
        // 返回根 inode 号
        Ok(2)
    }

    fn unmount(&self) -> KernelResult<()> {
        Ok(())
    }

    fn lookup(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
        // 为什么这样实现：tmpfs 不支持 lookup，应该通过 readdir 获取子项
        // 这里提供基本实现
        self.find_child(parent_ino, name)
    }

    fn root_inode(&self) -> InodeNumber {
        2
    }

    fn stat(&self, ino: InodeNumber) -> KernelResult<InodeMetadata> {
        self.get_node(ino)
            .map(|node| node.metadata)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)
    }

    fn read(&self, ino: InodeNumber, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if node.file_type != FileType::File {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let offset = offset as usize;
        if offset >= node.content.len() {
            return Ok(0);
        }

        let to_read = core::cmp::min(buf.len(), node.content.len() - offset);
        buf[..to_read].copy_from_slice(&node.content[offset..offset + to_read]);
        Ok(to_read)
    }

    fn write(&self, ino: InodeNumber, offset: u64, data: &[u8]) -> KernelResult<usize> {
        // tmpfs 的 FileSystem trait 中 write 接收 &self
        // 这是设计限制，实际实现需要 &mut self
        // 此处返回错误作为占位符
        let _ = (ino, offset, data);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<Vec<DirectoryEntry>> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !node.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut entries = Vec::new();

        // 为什么添加 . 和 ..：POSIX 标准
        entries.push(DirectoryEntry::new(b".", ino, FileType::Directory)?);
        // 注意：.. 应该指向父目录，这里简化为 .
        entries.push(DirectoryEntry::new(b"..", ino, FileType::Directory)?);

        for (name, child_ino) in &node.children {
            if let Some(child_node) = self.get_node(*child_ino) {
                entries.push(DirectoryEntry::new(name, *child_ino, child_node.file_type)?);
            }
        }

        Ok(entries)
    }

    fn create(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        // 为什么返回错误：create 需要修改文件系统（&mut self）
        let _ = (parent_ino, name, mode);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn mkdir(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        let _ = (parent_ino, name, mode);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn unlink(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()> {
        let _ = (parent_ino, name);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn rmdir(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()> {
        let _ = (parent_ino, name);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn symlink(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        target: &[u8],
    ) -> KernelResult<InodeNumber> {
        let _ = (parent_ino, name, target);
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readlink(&self, ino: InodeNumber, buf: &mut [u8]) -> KernelResult<usize> {
        let _ = (ino, buf);
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
    fn test_tmpfs_creation() {
        let tmpfs = Tmpfs::new().unwrap();
        assert_eq!(tmpfs.root_inode(), 2);
    }

    #[test]
    fn test_tmpfs_stat() {
        let tmpfs = Tmpfs::new().unwrap();
        let stat = tmpfs.stat(2).unwrap();
        assert_eq!(stat.inode_number, 2);
        assert_eq!(stat.file_type, FileType::Directory);
    }

    #[test]
    fn test_tmpfs_readdir() {
        let tmpfs = Tmpfs::new().unwrap();
        let entries = tmpfs.readdir(2).unwrap();
        // 应该有 . 和 ..
        assert!(entries.len() >= 2);
    }
}
