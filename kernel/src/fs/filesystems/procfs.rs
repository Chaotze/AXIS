// ============================================================
// procfs - proc 文件系统
// ============================================================
// 提供对进程和系统信息的访问（/proc/cpuinfo、/proc/meminfo 等）
// 大多数文件是动态生成的

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use alloc::vec::Vec;

// ============================================================
// procfs 节点类型
// ============================================================

/// procfs 中的文件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcFileType {
    /// /proc/cpuinfo
    CpuInfo,
    /// /proc/meminfo
    MemInfo,
    /// /proc/uptime
    Uptime,
    /// /proc/version
    Version,
}

/// procfs 文件节点
pub struct ProcfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 文件类型
    file_type: FileType,
    /// procfs 文件类型（如果是文件）
    proc_file_type: Option<ProcFileType>,
    /// 子项（如果是目录）
    children: Vec<(Vec<u8>, InodeNumber)>,
    /// 元数据
    metadata: InodeMetadata,
}

impl ProcfsNode {
    /// 创建目录节点
    fn new_dir(inode_number: InodeNumber) -> Self {
        let now = 0i64;
        ProcfsNode {
            inode_number,
            file_type: FileType::Directory,
            proc_file_type: None,
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

    /// 创建文件节点
    fn new_file(inode_number: InodeNumber, proc_file_type: ProcFileType) -> Self {
        let now = 0i64;
        ProcfsNode {
            inode_number,
            file_type: FileType::File,
            proc_file_type: Some(proc_file_type),
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
// procfs 文件系统实现
// ============================================================

/// procfs 文件系统
pub struct Procfs {
    /// 所有节点
    nodes: Vec<Option<ProcfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

impl Procfs {
    /// 创建新的 procfs 实例
    pub fn new() -> KernelResult<Self> {
        let mut procfs = Procfs {
            nodes: Vec::new(),
            next_ino: 2,
        };

        // 预留 inode 0/1
        procfs.nodes.push(None);
        procfs.nodes.push(None);

        // 创建根目录（inode 2）
        procfs.nodes.push(Some(ProcfsNode::new_dir(2)));

        // 创建标准文件
        // inode 3: /proc/cpuinfo
        procfs.nodes.push(Some(ProcfsNode::new_file(3, ProcFileType::CpuInfo)));
        // inode 4: /proc/meminfo
        procfs.nodes.push(Some(ProcfsNode::new_file(4, ProcFileType::MemInfo)));
        // inode 5: /proc/uptime
        procfs.nodes.push(Some(ProcfsNode::new_file(5, ProcFileType::Uptime)));
        // inode 6: /proc/version
        procfs.nodes.push(Some(ProcfsNode::new_file(6, ProcFileType::Version)));

        procfs.next_ino = 7;

        // 在根目录中添加文件项
        if let Some(root) = procfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
            root.children.push((b"cpuinfo".to_vec(), 3));
            root.children.push((b"meminfo".to_vec(), 4));
            root.children.push((b"uptime".to_vec(), 5));
            root.children.push((b"version".to_vec(), 6));
        }

        Ok(procfs)
    }

    /// 获取节点
    fn get_node(&self, ino: InodeNumber) -> Option<&ProcfsNode> {
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

    /// 生成 cpuinfo 内容
    fn generate_cpuinfo(&self) -> Vec<u8> {
        let content = b"processor\t: 0\n\
vendor_id\t: GenuineIntel\n\
cpu family\t: 6\n\
model\t\t: 142\n\
stepping\t: 10\n\
microcode\t: 0xd6\n\
cpu MHz\t\t: 2400.0\n\
cache size\t: 3072 KB\n";
        content.to_vec()
    }

    /// 生成 meminfo 内容
    fn generate_meminfo(&self) -> Vec<u8> {
        let content = b"MemTotal:        8388608 kB\n\
MemFree:         5242880 kB\n\
MemAvailable:    5242880 kB\n\
Buffers:              0 kB\n\
Cached:               0 kB\n";
        content.to_vec()
    }

    /// 生成 version 内容
    fn generate_version(&self) -> Vec<u8> {
        let content = b"Linux version 0.1.2 (AXIS Kernel)\n";
        content.to_vec()
    }
}

impl FileSystem for Procfs {
    fn name(&self) -> &'static str {
        "procfs"
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

        let proc_file_type = node.proc_file_type
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        // 动态生成文件内容
        let content = match proc_file_type {
            ProcFileType::CpuInfo => self.generate_cpuinfo(),
            ProcFileType::MemInfo => self.generate_meminfo(),
            ProcFileType::Uptime => b"100000 500000\n".to_vec(),  // uptime, idle time
            ProcFileType::Version => self.generate_version(),
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
        // procfs 文件只读
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
    fn test_procfs_creation() {
        let procfs = Procfs::new().unwrap();
        assert_eq!(procfs.root_inode(), 2);
    }

    #[test]
    fn test_procfs_readdir() {
        let procfs = Procfs::new().unwrap();
        let entries = procfs.readdir(2).unwrap();
        // 应该有 . .. cpuinfo meminfo uptime version
        assert!(entries.len() >= 6);
    }

    #[test]
    fn test_procfs_cpuinfo() {
        let procfs = Procfs::new().unwrap();
        let mut buf = [0u8; 256];
        let n = procfs.read(3, 0, &mut buf).unwrap();
        assert!(n > 0);
        assert!(core::str::from_utf8(&buf[..n]).unwrap().contains("processor"));
    }
}
