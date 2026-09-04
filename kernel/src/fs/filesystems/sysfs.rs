// ============================================================
// sysfs - sys 文件系统
// ============================================================
// 提供对系统设备和驱动的访问（/sys/devices、/sys/class 等）
// 类似 procfs，但更结构化

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use alloc::vec::Vec;

// ============================================================
// sysfs 节点
// ============================================================

/// sysfs 文件节点
pub struct SysfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 文件类型
    file_type: FileType,
    /// 是否为虚拟文件（动态生成）
    is_virtual: bool,
    /// 子项（如果是目录）
    children: Vec<(Vec<u8>, InodeNumber)>,
    /// 元数据
    metadata: InodeMetadata,
}

impl SysfsNode {
    /// 创建目录节点
    fn new_dir(inode_number: InodeNumber) -> Self {
        let now = 0i64;
        SysfsNode {
            inode_number,
            file_type: FileType::Directory,
            is_virtual: false,
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::Directory,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o555),
                uid: 0,
                gid: 0,
                nlink: 2,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 创建虚拟文件节点
    fn new_virtual_file(inode_number: InodeNumber) -> Self {
        let now = 0i64;
        SysfsNode {
            inode_number,
            file_type: FileType::File,
            is_virtual: true,
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::File,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o444),
                uid: 0,
                gid: 0,
                nlink: 1,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    fn is_dir(&self) -> bool {
        self.metadata.file_type == FileType::Directory
    }
}

// ============================================================
// sysfs 文件系统实现
// ============================================================

/// sysfs 文件系统
pub struct Sysfs {
    /// 所有节点
    nodes: Vec<Option<SysfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

impl Sysfs {
    /// 创建新的 sysfs 实例
    pub fn new() -> KernelResult<Self> {
        let mut sysfs = Sysfs {
            nodes: Vec::new(),
            next_ino: 2,
        };

        // 预留 inode 0/1
        sysfs.nodes.push(None);
        sysfs.nodes.push(None);

        // 创建根目录（inode 2）
        sysfs.nodes.push(Some(SysfsNode::new_dir(2)));

        // 创建子目录
        // inode 3: /sys/devices
        sysfs.nodes.push(Some(SysfsNode::new_dir(3)));
        // inode 4: /sys/class
        sysfs.nodes.push(Some(SysfsNode::new_dir(4)));
        // inode 5: /sys/module
        sysfs.nodes.push(Some(SysfsNode::new_dir(5)));

        sysfs.next_ino = 6;

        // 在根目录中添加子目录项
        if let Some(root) = sysfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
            root.children.push((b"devices".to_vec(), 3));
            root.children.push((b"class".to_vec(), 4));
            root.children.push((b"module".to_vec(), 5));
        }

        Ok(sysfs)
    }

    /// 获取节点
    fn get_node(&self, ino: InodeNumber) -> Option<&SysfsNode> {
        self.nodes.get(ino as usize)?.as_ref()
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
}

impl FileSystem for Sysfs {
    fn name(&self) -> &'static str {
        "sysfs"
    }

    fn mount(&self) -> KernelResult<InodeNumber> {
        Ok(2)
    }

    fn unmount(&self) -> KernelResult<()> {
        Ok(())
    }

    fn lookup(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
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

        if !node.is_virtual {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 生成虚拟文件内容（根据 inode）
        let content = match ino {
            _ => b"sysfs virtual file\n".to_vec(),
        };

        let offset = offset as usize;
        if offset >= content.len() {
            return Ok(0);
        }

        let to_read = core::cmp::min(buf.len(), content.len() - offset);
        buf[..to_read].copy_from_slice(&content[offset..offset + to_read]);
        Ok(to_read)
    }

    fn write(&self, _ino: InodeNumber, _offset: u64, _data: &[u8]) -> KernelResult<usize> {
        // sysfs 文件只读
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<Vec<DirectoryEntry>> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !node.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut entries = Vec::new();

        entries.push(DirectoryEntry::new(b".", ino, FileType::Directory)?);
        entries.push(DirectoryEntry::new(b"..", ino, FileType::Directory)?);

        for (name, child_ino) in &node.children {
            if let Some(child_node) = self.get_node(*child_ino) {
                entries.push(DirectoryEntry::new(name, *child_ino, child_node.metadata.file_type)?);
            }
        }

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

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_creation() {
        let sysfs = Sysfs::new().unwrap();
        assert_eq!(sysfs.root_inode(), 2);
    }

    #[test]
    fn test_sysfs_readdir() {
        let sysfs = Sysfs::new().unwrap();
        let entries = sysfs.readdir(2).unwrap();
        // 应该有 . .. devices class module
        assert!(entries.len() >= 5);
    }
}
