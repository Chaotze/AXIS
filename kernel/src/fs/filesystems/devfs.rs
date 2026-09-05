// ============================================================
// devfs - 设备文件系统
// ============================================================
// 提供对虚拟设备的访问（/dev/null、/dev/zero 等）
// 大多数文件都是字符设备或块设备
//
// 架构特性：
// - 字符设备：/dev/null、/dev/zero、/dev/random 等
// - 块设备：/dev/sda、/dev/sdb 等（待集成块设备驱动层）
// - 设备文件是虚拟的，通过 devfs 管理设备树
// - 支持动态添加设备

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use crate::lib::time::current_timestamp_secs;
use alloc::vec;
use alloc::vec::Vec;

// ============================================================
// 设备类型定义
// ============================================================

/// 字符设备类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharDeviceType {
    Null,       // /dev/null - 黑洞设备
    Zero,       // /dev/zero - 全零设备
    Full,       // /dev/full - 写满设备
    Random,     // /dev/random - 随机数设备
}

/// 块设备类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceType {
    SDA,        // /dev/sda - 第一块硬盘
    SDB,        // /dev/sdb - 第二块硬盘
    // 未来支持更多块设备
}

/// devfs 节点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DevfsNodeKind {
    Root,
    CharDevice(CharDeviceType),
    BlockDevice(BlockDeviceType),
}

/// devfs 中的设备节点
pub struct DevfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 节点类型
    kind: DevfsNodeKind,
    /// 是目录时的子项
    children: Vec<(Vec<u8>, InodeNumber)>,
    /// 元数据
    metadata: InodeMetadata,
}

impl DevfsNode {
    /// 创建根目录节点
    fn new_root(inode_number: InodeNumber) -> Self {
        let now = current_timestamp_secs();
        DevfsNode {
            inode_number,
            kind: DevfsNodeKind::Root,
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
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
        }
    }

    /// 创建字符设备节点
    fn new_char_device(inode_number: InodeNumber, device_type: CharDeviceType) -> Self {
        let now = current_timestamp_secs();
        DevfsNode {
            inode_number,
            kind: DevfsNodeKind::CharDevice(device_type),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::CharDevice,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o666),
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

    /// 创建块设备节点
    fn new_block_device(inode_number: InodeNumber, device_type: BlockDeviceType) -> Self {
        let now = current_timestamp_secs();
        DevfsNode {
            inode_number,
            kind: DevfsNodeKind::BlockDevice(device_type),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::BlockDevice,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o660),
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
        matches!(self.kind, DevfsNodeKind::Root)
    }
}

// ============================================================
// devfs 文件系统实现
// ============================================================

/// devfs 文件系统
pub struct Devfs {
    /// 所有节点
    nodes: Vec<Option<DevfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

impl Devfs {
    /// 创建新的 devfs 实例
    pub fn new() -> KernelResult<Self> {
        let mut devfs = Devfs {
            nodes: Vec::new(),
            next_ino: 2,
        };

        // 预留 inode 0/1
        devfs.nodes.push(None);
        devfs.nodes.push(None);

        // 创建根目录（inode 2）
        devfs.nodes.push(Some(DevfsNode::new_root(2)));

        // 创建标准字符设备
        let char_devices = vec![
            (b"null".to_vec(), CharDeviceType::Null, 3),
            (b"zero".to_vec(), CharDeviceType::Zero, 4),
            (b"full".to_vec(), CharDeviceType::Full, 5),
            (b"random".to_vec(), CharDeviceType::Random, 6),
        ];

        for (name, dev_type, ino) in char_devices {
            devfs.nodes.push(Some(DevfsNode::new_char_device(ino, dev_type)));
            if let Some(root) = devfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
                root.children.push((name, ino));
            }
        }

        devfs.next_ino = 7;

        // 创建块设备节点（演示用途，实际驱动还未就绪）
        // /dev/sda 和 /dev/sdb
        let block_devices = vec![
            (b"sda".to_vec(), BlockDeviceType::SDA, 7),
            (b"sdb".to_vec(), BlockDeviceType::SDB, 8),
        ];

        for (name, dev_type, ino) in block_devices {
            devfs.nodes.push(Some(DevfsNode::new_block_device(ino, dev_type)));
            if let Some(root) = devfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
                root.children.push((name, ino));
            }
        }

        devfs.next_ino = 9;

        Ok(devfs)
    }

    /// 获取节点
    fn get_node(&self, ino: InodeNumber) -> Option<&DevfsNode> {
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

impl FileSystem for Devfs {
    fn name(&self) -> &'static str {
        "devfs"
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

        // 为什么检查设备类型：只有设备节点可以读
        match node.kind {
            DevfsNodeKind::CharDevice(dev_type) => {
                // 字符设备读操作
                match dev_type {
                    CharDeviceType::Null => {
                        // /dev/null 总是返回 EOF
                        Ok(0)
                    }
                    CharDeviceType::Zero => {
                        // /dev/zero 返回全零
                        for b in buf.iter_mut() {
                            *b = 0;
                        }
                        Ok(buf.len())
                    }
                    CharDeviceType::Full => {
                        // /dev/full 返回错误（设备满）
                        Err(crate::lib::result::KernelError::InvalidArgument)
                    }
                    CharDeviceType::Random => {
                        // /dev/random 返回伪随机数
                        let mut seed = offset as u32;
                        for b in buf.iter_mut() {
                            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                            *b = (seed >> 16) as u8;
                        }
                        Ok(buf.len())
                    }
                }
            }
            DevfsNodeKind::BlockDevice(_dev_type) => {
                // 块设备读操作
                // TODO: 集成块设备驱动层后实现真实的块 I/O
                // 目前返回错误（驱动未就绪）
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
            DevfsNodeKind::Root => {
                // 目录不可读
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
        }
    }

    fn write(&self, ino: InodeNumber, _offset: u64, data: &[u8]) -> KernelResult<usize> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        match node.kind {
            DevfsNodeKind::CharDevice(dev_type) => {
                match dev_type {
                    CharDeviceType::Null => {
                        // /dev/null 丢弃所有数据
                        Ok(data.len())
                    }
                    CharDeviceType::Zero => {
                        // /dev/zero 不支持写
                        Err(crate::lib::result::KernelError::InvalidArgument)
                    }
                    CharDeviceType::Full => {
                        // /dev/full 设备满，拒绝写
                        Err(crate::lib::result::KernelError::InvalidArgument)
                    }
                    CharDeviceType::Random => {
                        // /dev/random 不支持写
                        Err(crate::lib::result::KernelError::InvalidArgument)
                    }
                }
            }
            DevfsNodeKind::BlockDevice(_dev_type) => {
                // 块设备写操作
                // TODO: 集成块设备驱动层后实现真实的块 I/O
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
            DevfsNodeKind::Root => {
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
        }
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
        // devfs 不支持创建设备
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
    fn test_devfs_creation() {
        let devfs = Devfs::new().unwrap();
        assert_eq!(devfs.root_inode(), 2);
    }

    #[test]
    fn test_devfs_readdir() {
        let devfs = Devfs::new().unwrap();
        let entries = devfs.readdir(2).unwrap();
        // 应该有 . .. 加上各类设备
        assert!(entries.len() >= 6);
    }

    #[test]
    fn test_devfs_null_device() {
        let devfs = Devfs::new().unwrap();
        let mut buf = [1u8; 100];
        // /dev/null 返回 EOF
        let n = devfs.read(3, 0, &mut buf).unwrap();
        assert_eq!(n, 0);
        // 写入返回成功
        let n = devfs.write(3, 0, b"test").unwrap();
        assert_eq!(n, 4);
    }

    #[test]
    fn test_devfs_zero_device() {
        let devfs = Devfs::new().unwrap();
        let mut buf = [1u8; 10];
        // /dev/zero 返回全零
        let n = devfs.read(4, 0, &mut buf).unwrap();
        assert_eq!(n, 10);
        for b in &buf {
            assert_eq!(*b, 0);
        }
    }

    #[test]
    fn test_devfs_block_devices_present() {
        let devfs = Devfs::new().unwrap();
        let entries = devfs.readdir(2).unwrap();
        // 查找 sda 和 sdb
        let has_sda = entries.iter().any(|e| e.name == b"sda".to_vec());
        let has_sdb = entries.iter().any(|e| e.name == b"sdb".to_vec());
        assert!(has_sda && has_sdb);
    }
}


// ============================================================
// 单元测试
