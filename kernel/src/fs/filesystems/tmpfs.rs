// ============================================================
// tmpfs - 临时内存文件系统
// ============================================================
// 最简单的文件系统实现，完全基于内存。
// 用于测试 VFS 框架和作为根文件系统。
// 为什么用 Spinlock：文件系统内部需要可变状态管理，
// 通过 Spinlock 提供内部可变性而保持 FileSystem trait 的 &self 接口。

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use crate::sync::Spinlock;
use alloc::vec::Vec;
use alloc::sync::Arc;

// ============================================================
// tmpfs 内存节点
// ============================================================

/// tmpfs 中的文件/目录节点
#[derive(Clone)]
struct TmpfsNode {
    /// inode 号
    pub inode_number: InodeNumber,
    /// 文件类型
    pub file_type: FileType,
    /// 文件内容（仅用于普通文件）
    pub content: Vec<u8>,
    /// 子项（仅用于目录）
    pub children: Vec<(Vec<u8>, InodeNumber)>,  // (name, child_ino)
    /// 元数据
    pub metadata: InodeMetadata,
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

/// Tmpfs 内部状态
struct TmpfsState {
    /// 所有 inode（按 inode 号索引）
    nodes: Vec<Option<TmpfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

// ============================================================
// tmpfs 文件系统实现
// ============================================================

/// tmpfs 文件系统
pub struct Tmpfs {
    /// 内部状态（使用 Spinlock 提供内部可变性）
    state: Spinlock<TmpfsState>,
}

impl Tmpfs {
    /// 创建新的 tmpfs 实例
    pub fn new() -> KernelResult<Arc<Self>> {
        let mut state = TmpfsState {
            nodes: Vec::new(),
            next_ino: 2,  // inode 0/1 保留
        };

        // 创建根目录（inode 2）
        let root = TmpfsNode::new(2, FileType::Directory, FileMode::new(0o755));
        state.nodes.push(None);  // inode 0
        state.nodes.push(None);  // inode 1
        state.nodes.push(Some(root));
        state.next_ino = 3;

        Ok(Arc::new(Tmpfs {
            state: Spinlock::new(state),
        }))
    }

    /// 根据 inode 号获取节点的副本
    fn get_node(&self, ino: InodeNumber) -> Option<TmpfsNode> {
        let guard = self.state.lock();
        guard.nodes
            .get(ino as usize)
            .and_then(|opt| opt.as_ref().cloned())
    }

    /// 在目录中查找子项
    fn find_child(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
        let guard = self.state.lock();

        let parent = guard.nodes
            .get(parent_ino as usize)
            .and_then(|opt| opt.as_ref())
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !parent.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        for (child_name, child_ino) in &parent.children {
            if child_name.as_slice() == name {
                return Ok(*child_ino);
            }
        }

        Err(crate::lib::result::KernelError::InvalidArgument)
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
        // 使用 Spinlock 获取可变访问
        let mut guard = self.state.lock();

        let node = guard.nodes
            .get_mut(ino as usize)
            .and_then(|opt| opt.as_mut())
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if node.file_type != FileType::File {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let offset = offset as usize;

        // 为什么需要扩展内容缓冲区：写入可能超过当前文件大小
        if offset + data.len() > node.content.len() {
            node.content.resize(offset + data.len(), 0);
        }

        node.content[offset..offset + data.len()].copy_from_slice(data);
        node.metadata.size = node.content.len() as u64;
        node.metadata.blocks = ((node.content.len() + 511) / 512) as u64;

        Ok(data.len())
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<alloc::vec::Vec<DirectoryEntry>> {
        let guard = self.state.lock();

        let node = guard.nodes
            .get(ino as usize)
            .and_then(|opt| opt.as_ref())
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !node.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut entries = alloc::vec::Vec::new();

        // 为什么添加 . 和 ..：POSIX 标准
        entries.push(DirectoryEntry::new(b".", ino, FileType::Directory)?);
        // 注意：.. 应该指向父目录，这里简化为 .
        entries.push(DirectoryEntry::new(b"..", ino, FileType::Directory)?);

        for (name, child_ino) in &node.children {
            if let Some(child_node) = guard.nodes.get(*child_ino as usize).and_then(|opt| opt.as_ref()) {
                entries.push(DirectoryEntry::new(name.as_slice(), *child_ino, child_node.file_type)?);
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
        let mut guard = self.state.lock();

        // 验证父目录存在且是目录
        let _parent = guard.nodes
            .get(parent_ino as usize)
            .and_then(|opt| opt.as_ref())
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        // 为什么检查名称长度：255 是 POSIX 标准限制
        if name.len() > 255 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 分配新 inode
        let ino = guard.next_ino;
        let node = TmpfsNode::new(ino, FileType::File, mode);
        guard.nodes.push(Some(node));
        guard.next_ino += 1;

        // 在父目录中添加子项
        if let Some(parent) = guard.nodes.get_mut(parent_ino as usize).and_then(|opt| opt.as_mut()) {
            parent.children.push((name.to_vec(), ino));
        }

        Ok(ino)
    }

    fn mkdir(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        let mut guard = self.state.lock();

        // 验证父目录存在且是目录
        let _parent = guard.nodes
            .get(parent_ino as usize)
            .and_then(|opt| opt.as_ref())
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if name.len() > 255 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 分配新 inode
        let ino = guard.next_ino;
        let node = TmpfsNode::new(ino, FileType::Directory, mode);
        guard.nodes.push(Some(node));
        guard.next_ino += 1;

        // 在父目录中添加子项
        if let Some(parent) = guard.nodes.get_mut(parent_ino as usize).and_then(|opt| opt.as_mut()) {
            parent.children.push((name.to_vec(), ino));
        }

        Ok(ino)
    }

    fn unlink(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()> {
        let mut guard = self.state.lock();

        // 查找要删除的子项
        let mut child_ino = None;
        if let Some(parent) = guard.nodes.get_mut(parent_ino as usize).and_then(|opt| opt.as_mut()) {
            if let Some(pos) = parent.children.iter().position(|(n, _)| n.as_slice() == name) {
                child_ino = Some(parent.children.remove(pos).1);
            }
        }

        if let Some(ino) = child_ino {
            // 为什么保留空槽位：避免改变其他 inode 的索引
            if let Some(slot) = guard.nodes.get_mut(ino as usize) {
                *slot = None;
            }
            Ok(())
        } else {
            Err(crate::lib::result::KernelError::InvalidArgument)
        }
    }

    fn rmdir(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()> {
        let mut guard = self.state.lock();

        // 查找要删除的目录
        let mut child_ino = None;
        if let Some(parent) = guard.nodes.get_mut(parent_ino as usize).and_then(|opt| opt.as_mut()) {
            if let Some(pos) = parent.children.iter().position(|(n, _)| n.as_slice() == name) {
                child_ino = Some(parent.children.remove(pos).1);
            }
        }

        if let Some(ino) = child_ino {
            // 为什么检查目录是否为空：POSIX 规定删除的目录必须为空
            if let Some(Some(node)) = guard.nodes.get(ino as usize) {
                if !node.children.is_empty() {
                    // 恢复删除的项
                    if let Some(parent) = guard.nodes.get_mut(parent_ino as usize).and_then(|opt| opt.as_mut()) {
                        parent.children.push((name.to_vec(), ino));
                    }
                    return Err(crate::lib::result::KernelError::InvalidArgument);
                }
            }

            // 删除目录
            if let Some(slot) = guard.nodes.get_mut(ino as usize) {
                *slot = None;
            }
            Ok(())
        } else {
            Err(crate::lib::result::KernelError::InvalidArgument)
        }
    }

    fn symlink(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _target: &[u8],
    ) -> KernelResult<InodeNumber> {
        // tmpfs 暂不支持符号链接
        Err(crate::lib::result::KernelError::Unsupported)
    }

    fn readlink(&self, _ino: InodeNumber, _buf: &mut [u8]) -> KernelResult<usize> {
        // tmpfs 暂不支持符号链接
        Err(crate::lib::result::KernelError::Unsupported)
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

    #[test]
    fn test_tmpfs_create_file() {
        let tmpfs = Tmpfs::new().unwrap();
        let ino = tmpfs.create(2, b"test.txt", FileMode::new(0o644)).unwrap();
        assert!(ino > 2);

        // 验证文件能被找到
        let found_ino = tmpfs.lookup(2, b"test.txt").unwrap();
        assert_eq!(found_ino, ino);
    }

    #[test]
    fn test_tmpfs_write_read() {
        let tmpfs = Tmpfs::new().unwrap();
        let ino = tmpfs.create(2, b"data.txt", FileMode::new(0o644)).unwrap();

        let data = b"Hello, tmpfs!";
        let written = tmpfs.write(ino, 0, data).unwrap();
        assert_eq!(written, data.len());

        let mut buf = [0u8; 20];
        let read = tmpfs.read(ino, 0, &mut buf).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);
    }

    #[test]
    fn test_tmpfs_mkdir() {
        let tmpfs = Tmpfs::new().unwrap();
        let dir_ino = tmpfs.mkdir(2, b"testdir", FileMode::new(0o755)).unwrap();
        assert!(dir_ino > 2);

        // 验证目录能被找到
        let found_ino = tmpfs.lookup(2, b"testdir").unwrap();
        assert_eq!(found_ino, dir_ino);
    }

    #[test]
    fn test_tmpfs_unlink() {
        let tmpfs = Tmpfs::new().unwrap();
        let ino = tmpfs.create(2, b"todelete.txt", FileMode::new(0o644)).unwrap();

        // 验证文件存在
        assert!(tmpfs.lookup(2, b"todelete.txt").is_ok());

        // 删除文件
        tmpfs.unlink(2, b"todelete.txt").unwrap();

        // 验证文件已删除
        assert!(tmpfs.lookup(2, b"todelete.txt").is_err());
    }
}
