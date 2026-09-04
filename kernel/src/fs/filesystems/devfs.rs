// ============================================================
// devfs - 设备文件系统
// ============================================================
// 提供对虚拟设备的访问（/dev/null、/dev/zero 等）
// 大多数文件都是字符设备或块设备

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use alloc::vec::Vec;

// ============================================================
// devfs 设备节点类型
// ============================================================

/// 设备类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// /dev/null - 黑洞设备
    Null,
    /// /dev/zero - 全零设备
    Zero,
    /// /dev/full - 写满设备
    Full,
    /// /dev/random - 随机数设备
    Random,
}

/// devfs 中的设备节点
pub struct DevfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 文件类型
    file_type: FileType,
    /// 设备类型（如果是设备）
    device_type: Option<DeviceType>,
    /// 是目录时的子项
    children: Vec<(Vec<u8>, InodeNumber)>,
    /// 元数据
    metadata: InodeMetadata,
}

impl DevfsNode {
    /// 创建目录节点
    fn new_dir(inode_number: InodeNumber) -> Self {
        let now = 0i64;
        DevfsNode {
            inode_number,
            file_type: FileType::CharDevice,
            device_type: None,
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

    /// 创建设备节点
    fn new_device(inode_number: InodeNumber, device_type: DeviceType) -> Self {
        let now = 0i64;
        DevfsNode {
            inode_number,
            file_type: FileType::CharDevice,
            device_type: Some(device_type),
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

    fn is_dir(&self) -> bool {
        self.metadata.file_type == FileType::Directory
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
        devfs.nodes.push(Some(DevfsNode::new_dir(2)));

        // 创建标准设备
        // inode 3: /dev/null
        devfs.nodes.push(Some(DevfsNode::new_device(3, DeviceType::Null)));
        // inode 4: /dev/zero
        devfs.nodes.push(Some(DevfsNode::new_device(4, DeviceType::Zero)));
        // inode 5: /dev/full
        devfs.nodes.push(Some(DevfsNode::new_device(5, DeviceType::Full)));
        // inode 6: /dev/random
        devfs.nodes.push(Some(DevfsNode::new_device(6, DeviceType::Random)));

        devfs.next_ino = 7;

        // 在根目录中添加设备项
        if let Some(root) = devfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
            root.children.push((b"null".to_vec(), 3));
            root.children.push((b"zero".to_vec(), 4));
            root.children.push((b"full".to_vec(), 5));
            root.children.push((b"random".to_vec(), 6));
        }

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
        let device_type = node.device_type
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        match device_type {
            DeviceType::Null => {
                // /dev/null 总是返回 EOF
                Ok(0)
            }
            DeviceType::Zero => {
                // /dev/zero 返回全零
                for b in buf.iter_mut() {
                    *b = 0;
                }
                Ok(buf.len())
            }
            DeviceType::Full => {
                // /dev/full 返回错误（设备满）
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
            DeviceType::Random => {
                // /dev/random 返回伪随机数（简化实现）
                let mut seed = offset as u32;
                for b in buf.iter_mut() {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                    *b = (seed >> 16) as u8;
                }
                Ok(buf.len())
            }
        }
    }

    fn write(&self, ino: InodeNumber, _offset: u64, data: &[u8]) -> KernelResult<usize> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        let device_type = node.device_type
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        match device_type {
            DeviceType::Null => {
                // /dev/null 丢弃所有数据
                Ok(data.len())
            }
            DeviceType::Zero => {
                // /dev/zero 不支持写
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
            DeviceType::Full => {
                // /dev/full 设备满，拒绝写
                Err(crate::lib::result::KernelError::InvalidArgument)
            }
            DeviceType::Random => {
                // /dev/random 不支持写
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
        // 应该有 . .. null zero full random
        assert!(entries.len() >= 4);
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
}
