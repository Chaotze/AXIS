// ============================================================
// 文件对象和文件描述符表
// ============================================================
// 实现打开文件对象和进程的文件描述符表。
// 支持文件的读写、定位等操作。

use crate::fs::vfs::{FileOffset, InodeNumber, OpenFlags};
use crate::lib::result::KernelResult;
use alloc::sync::Arc;

// ============================================================
// 打开文件对象
// ============================================================

/// 打开的文件对象
/// 为什么用专门的 OpenFile 结构：
/// - 多个进程可能同时打开同一个文件
/// - 需要独立跟踪每个打开实例的状态（偏移、权限等）
/// - 支持引用计数（多个 fd 可能引用同一个 OpenFile）
#[derive(Debug, Clone)]
pub struct OpenFile {
    /// 关联的 inode 号
    pub inode_number: InodeNumber,
    /// 当前文件偏移（字节）
    pub offset: FileOffset,
    /// 打开时的标志
    pub flags: OpenFlags,
    /// 引用计数（有多少个进程的 fd 指向此文件）
    pub refcount: u32,
}

impl OpenFile {
    /// 创建新的打开文件对象
    pub fn new(inode_number: InodeNumber, flags: OpenFlags) -> Self {
        OpenFile {
            inode_number,
            offset: 0,
            flags,
            refcount: 1,
        }
    }

    /// 读取（移动文件指针）
    /// 为什么返回新的偏移：便于调用者检查是否到达文件末尾
    pub fn read(&mut self, len: usize) -> FileOffset {
        let old_offset = self.offset;
        self.offset += len as u64;
        old_offset
    }

    /// 写入（移动文件指针）
    pub fn write(&mut self, len: usize) -> FileOffset {
        let old_offset = self.offset;
        self.offset += len as u64;
        old_offset
    }

    /// 定位到指定位置
    pub fn seek(&mut self, offset: FileOffset) -> KernelResult<FileOffset> {
        // 为什么检查：防止负偏移（在 u64 中无法直接检查，但概念上如此）
        self.offset = offset;
        Ok(self.offset)
    }

    /// 获取当前位置
    pub fn tell(&self) -> FileOffset {
        self.offset
    }

    /// 重置位置到文件开始
    pub fn rewind(&mut self) {
        self.offset = 0;
    }

    /// 增加引用计数
    pub fn inc_refcount(&mut self) -> KernelResult<()> {
        if self.refcount >= u32::MAX {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }
        self.refcount += 1;
        Ok(())
    }

    /// 减少引用计数
    pub fn dec_refcount(&mut self) -> KernelResult<()> {
        if self.refcount == 0 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }
        self.refcount -= 1;
        Ok(())
    }

    /// 检查是否为可读
    pub fn is_readable(&self) -> bool {
        self.flags.is_readable()
    }

    /// 检查是否为可写
    pub fn is_writable(&self) -> bool {
        self.flags.is_writable()
    }

    /// 检查是否为追加模式
    pub fn is_append(&self) -> bool {
        self.flags.is_append()
    }
}

// ============================================================
// 文件描述符表
// ============================================================

/// 进程的文件描述符表
/// 为什么用数组而非 Vec：
/// - 文件描述符通常是小的整数（0, 1, 2, ...）
/// - 固定大小避免动态分配的复杂性
/// - 标准 Unix 通常有最大 FD 限制（如 1024）
pub struct FileDescriptorTable {
    /// FD 表（索引即为 fd 号）
    /// 为什么用 Option：某些 fd 可能被关闭（None）
    fds: [Option<Arc<OpenFile>>; 256],
}

impl FileDescriptorTable {
    /// 创建新的 FD 表
    pub fn new() -> Self {
        // 为什么使用循环：Arc<OpenFile> 不是 Copy，
        // 需要逐个元素初始化为 None
        let mut fds: [Option<Arc<OpenFile>>; 256] =
            [None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None,
             None, None, None, None, None, None, None, None];

        FileDescriptorTable { fds }
    }

    /// 打开文件并分配 fd
    /// 为什么返回 fd 号：调用者需要知道分配的 fd
    pub fn open(&mut self, file: Arc<OpenFile>) -> KernelResult<i32> {
        // 为什么从 3 开始：0/1/2 为标准流（stdin/stdout/stderr）
        for fd in 3..256 {
            if self.fds[fd].is_none() {
                self.fds[fd] = Some(file);
                return Ok(fd as i32);
            }
        }

        // 为什么返回错误：没有可用的 fd
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    /// 关闭 fd
    pub fn close(&mut self, fd: i32) -> KernelResult<()> {
        // 为什么检查范围：fd 号必须有效
        if fd < 0 || fd >= 256 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 为什么检查是否打开：不能关闭未打开的 fd
        if self.fds[fd as usize].is_none() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        self.fds[fd as usize] = None;
        Ok(())
    }

    /// 获取指定 fd 的文件对象
    pub fn get(&self, fd: i32) -> Option<Arc<OpenFile>> {
        if fd < 0 || fd >= 256 {
            return None;
        }

        self.fds[fd as usize].clone()
    }

    /// 获取可变引用（用于修改文件状态）
    /// 为什么需要可变访问：需要修改文件偏移等状态
    pub fn get_mut(&mut self, fd: i32) -> Option<&mut Arc<OpenFile>> {
        if fd < 0 || fd >= 256 {
            return None;
        }

        self.fds[fd as usize].as_mut()
    }

    /// 复制 fd（dup）
    pub fn dup(&mut self, old_fd: i32) -> KernelResult<i32> {
        // 为什么先获取旧 fd：确保它存在
        let file = self.get(old_fd)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        self.open(file)
    }

    /// 复制 fd 到指定位置（dup2）
    pub fn dup2(&mut self, old_fd: i32, new_fd: i32) -> KernelResult<i32> {
        // 为什么检查范围：new_fd 必须有效
        if new_fd < 0 || new_fd >= 256 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let file = self.get(old_fd)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        // 为什么关闭目标 fd：如果已打开则需要先关闭
        let _ = self.close(new_fd);

        self.fds[new_fd as usize] = Some(file);
        Ok(new_fd)
    }

    /// 清空 FD 表
    pub fn clear(&mut self) {
        for fd in &mut self.fds {
            *fd = None;
        }
    }

    /// 获取已使用的 fd 数
    pub fn count_open(&self) -> usize {
        self.fds.iter().filter(|fd| fd.is_some()).count()
    }
}

impl Default for FileDescriptorTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_file_creation() {
        let flags = OpenFlags(OpenFlags::O_RDWR);
        let file = OpenFile::new(1, flags);

        assert_eq!(file.inode_number, 1);
        assert_eq!(file.offset, 0);
        assert!(file.is_readable());
        assert!(file.is_writable());
    }

    #[test]
    fn test_open_file_seek() {
        let flags = OpenFlags(OpenFlags::O_RDWR);
        let mut file = OpenFile::new(1, flags);

        assert_eq!(file.tell(), 0);
        file.seek(100).unwrap();
        assert_eq!(file.tell(), 100);
    }

    #[test]
    fn test_fd_table_open() {
        let mut table = FileDescriptorTable::new();
        let file = Arc::new(OpenFile::new(1, OpenFlags(OpenFlags::O_RDWR)));

        let fd = table.open(file.clone()).unwrap();
        assert_eq!(fd, 3);  // 第一个可用的 fd
        assert!(table.get(fd).is_some());
    }

    #[test]
    fn test_fd_table_close() {
        let mut table = FileDescriptorTable::new();
        let file = Arc::new(OpenFile::new(1, OpenFlags(OpenFlags::O_RDWR)));

        let fd = table.open(file).unwrap();
        assert!(table.close(fd).is_ok());
        assert!(table.get(fd).is_none());
    }

    #[test]
    fn test_fd_table_dup() {
        let mut table = FileDescriptorTable::new();
        let file = Arc::new(OpenFile::new(1, OpenFlags(OpenFlags::O_RDWR)));

        let fd1 = table.open(file).unwrap();
        let fd2 = table.dup(fd1).unwrap();

        assert_ne!(fd1, fd2);
        assert_eq!(table.get(fd1).unwrap().inode_number, table.get(fd2).unwrap().inode_number);
    }

    #[test]
    fn test_fd_table_dup2() {
        let mut table = FileDescriptorTable::new();
        let file = Arc::new(OpenFile::new(1, OpenFlags(OpenFlags::O_RDWR)));

        let fd1 = table.open(file).unwrap();
        let fd2 = table.dup2(fd1, 10).unwrap();

        assert_eq!(fd2, 10);
        assert!(table.get(10).is_some());
    }
}
