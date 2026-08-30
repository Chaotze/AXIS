// ============================================================
// VFS 抽象层：核心 trait 定义
// ============================================================
// 定义虚拟文件系统的统一接口，支持不同的具体文件系统实现
// 本模块为纯逻辑层，无全局状态、无锁、无 arch 依赖，
// 可被 unitest 宿主测试独立编译验证。

use crate::lib::result::KernelResult;
use core::fmt;

// ============================================================
// 类型定义
// ============================================================

/// Inode 号（文件系统内唯一 ID）
/// 为什么用 u64：兼容各文件系统的 inode 编号方案
pub type InodeNumber = u64;

/// 文件偏移（字节）
pub type FileOffset = u64;

/// 文件大小（字节）
pub type FileSize = u64;

/// Unix 时间戳（秒）
pub type UnixTime = i64;

// ============================================================
// 文件类型枚举
// ============================================================

/// 文件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// 普通文件
    File,
    /// 目录
    Directory,
    /// 符号链接
    Symlink,
    /// 字符设备
    CharDevice,
    /// 块设备
    BlockDevice,
    /// FIFO/管道
    Fifo,
    /// Unix 套接字
    Socket,
}

impl FileType {
    /// 是否为目录
    pub fn is_dir(&self) -> bool {
        *self == FileType::Directory
    }

    /// 是否为文件
    pub fn is_file(&self) -> bool {
        *self == FileType::File
    }

    /// 是否为符号链接
    pub fn is_symlink(&self) -> bool {
        *self == FileType::Symlink
    }
}

// ============================================================
// 权限和属性结构
// ============================================================

/// Unix 权限模式（9 位：rwxrwxrwx）
/// 存储为 u16 以留出空间给 setuid/setgid/sticky 等特殊权限
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMode(pub u16);

impl FileMode {
    /// 所有者读权限
    pub const OWNER_READ: u16 = 0o400;
    /// 所有者写权限
    pub const OWNER_WRITE: u16 = 0o200;
    /// 所有者执行权限
    pub const OWNER_EXEC: u16 = 0o100;
    /// 组读权限
    pub const GROUP_READ: u16 = 0o040;
    /// 组写权限
    pub const GROUP_WRITE: u16 = 0o020;
    /// 组执行权限
    pub const GROUP_EXEC: u16 = 0o010;
    /// 其他读权限
    pub const OTHERS_READ: u16 = 0o004;
    /// 其他写权限
    pub const OTHERS_WRITE: u16 = 0o002;
    /// 其他执行权限
    pub const OTHERS_EXEC: u16 = 0o001;

    /// 创建新的权限对象
    pub fn new(mode: u16) -> Self {
        FileMode(mode & 0o777)  // 只保留权限位
    }

    /// 检查所有者是否有指定权限
    pub fn owner_has_permission(&self, perm: u16) -> bool {
        (self.0 & perm) != 0
    }

    /// 检查组是否有指定权限（简化版，不考虑组成员）
    pub fn group_has_permission(&self, perm: u16) -> bool {
        (self.0 & (perm >> 3)) != 0
    }

    /// 检查其他是否有指定权限
    pub fn others_has_permission(&self, perm: u16) -> bool {
        (self.0 & (perm >> 6)) != 0
    }
}

/// Inode 元数据
#[derive(Debug, Clone, Copy)]
pub struct InodeMetadata {
    /// Inode 号
    pub inode_number: InodeNumber,
    /// 文件类型
    pub file_type: FileType,
    /// 文件大小（字节，目录时为逻辑大小）
    pub size: FileSize,
    /// 块数（物理占用块数，512字节单位）
    pub blocks: u64,
    /// Unix 权限模式
    pub mode: FileMode,
    /// 所有者 UID
    pub uid: u32,
    /// 组 ID
    pub gid: u32,
    /// 硬链接计数
    pub nlink: u32,
    /// 最后访问时间
    pub atime: UnixTime,
    /// 最后修改时间
    pub mtime: UnixTime,
    /// 最后状态改变时间
    pub ctime: UnixTime,
    /// 创建时间（部分文件系统支持）
    pub btime: Option<UnixTime>,
}

impl InodeMetadata {
    /// 检查当前用户是否有读权限
    /// 为什么只返回 bool：简化版，实际内核会根据当前 uid/gid 检查
    pub fn check_read_permission(&self) -> bool {
        // 简化实现：假设当前为所有者，检查所有者读权限
        self.mode.owner_has_permission(FileMode::OWNER_READ)
    }

    /// 检查当前用户是否有写权限
    pub fn check_write_permission(&self) -> bool {
        self.mode.owner_has_permission(FileMode::OWNER_WRITE)
    }

    /// 检查当前用户是否有执行权限
    pub fn check_exec_permission(&self) -> bool {
        self.mode.owner_has_permission(FileMode::OWNER_EXEC)
    }
}

// ============================================================
// 目录项结构（轻量级，用于目录列表）
// ============================================================

/// 目录项（用于 readdir）
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// 文件名
    pub name: [u8; 256],  // 定长数组避免堆分配
    /// 文件名实际长度
    pub name_len: usize,
    /// Inode 号
    pub inode_number: InodeNumber,
    /// 文件类型
    pub file_type: FileType,
}

impl DirectoryEntry {
    /// 创建新的目录项
    pub fn new(name: &[u8], inode_number: InodeNumber, file_type: FileType) -> KernelResult<Self> {
        // 为什么检查 name 长度：255 是标准 POSIX 限制
        if name.len() > 255 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut name_arr = [0u8; 256];
        name_arr[..name.len()].copy_from_slice(name);

        Ok(DirectoryEntry {
            name: name_arr,
            name_len: name.len(),
            inode_number,
            file_type,
        })
    }

    /// 获取文件名切片
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 获取文件名字符串（假设 UTF-8）
    pub fn name_str(&self) -> Option<&str> {
        core::str::from_utf8(self.name()).ok()
    }
}

// ============================================================
// 打开文件标志
// ============================================================

/// 打开文件时的标志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags(pub u32);

impl OpenFlags {
    /// 读模式
    pub const O_RDONLY: u32 = 0x00;
    /// 写模式
    pub const O_WRONLY: u32 = 0x01;
    /// 读写模式
    pub const O_RDWR: u32 = 0x02;
    /// 模式掩码
    pub const O_ACCMODE: u32 = 0x03;
    /// 追加模式
    pub const O_APPEND: u32 = 0x08;
    /// 创建文件（如果不存在）
    pub const O_CREAT: u32 = 0x100;
    /// 创建文件失败（如果已存在）
    pub const O_EXCL: u32 = 0x200;
    /// 截断文件
    pub const O_TRUNC: u32 = 0x1000;

    /// 检查是否为读模式
    pub fn is_readable(&self) -> bool {
        (self.0 & Self::O_ACCMODE) != Self::O_WRONLY
    }

    /// 检查是否为写模式
    pub fn is_writable(&self) -> bool {
        (self.0 & Self::O_ACCMODE) != Self::O_RDONLY
    }

    /// 检查是否为追加模式
    pub fn is_append(&self) -> bool {
        (self.0 & Self::O_APPEND) != 0
    }

    /// 检查是否创建文件
    pub fn should_create(&self) -> bool {
        (self.0 & Self::O_CREAT) != 0
    }

    /// 检查是否排斥已有文件
    pub fn is_exclusive(&self) -> bool {
        (self.0 & Self::O_EXCL) != 0
    }

    /// 检查是否截断文件
    pub fn should_truncate(&self) -> bool {
        (self.0 & Self::O_TRUNC) != 0
    }
}

// ============================================================
// VFS 核心 trait
// ============================================================

/// 文件系统 trait
/// 为什么分离 FileSystem trait：
/// - 允许多个文件系统实现同时存在
/// - 支持动态注册和卸载
/// - 定义统一的挂载/卸载接口
pub trait FileSystem: Send + Sync {
    /// 获取文件系统名称（如 "tmpfs"、"devfs"）
    fn name(&self) -> &'static str;

    /// 挂载文件系统
    /// 返回根 Inode
    fn mount(&self) -> KernelResult<InodeNumber>;

    /// 卸载文件系统
    fn unmount(&self) -> KernelResult<()>;

    /// 查找 Inode
    fn lookup(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber>;

    /// 获取根 Inode 号
    fn root_inode(&self) -> InodeNumber;

    /// 获取 Inode 元数据
    fn stat(&self, ino: InodeNumber) -> KernelResult<InodeMetadata>;

    /// 读取文件内容
    fn read(&self, ino: InodeNumber, offset: FileOffset, buf: &mut [u8]) -> KernelResult<usize>;

    /// 写入文件内容
    fn write(&self, ino: InodeNumber, offset: FileOffset, data: &[u8]) -> KernelResult<usize>;

    /// 列出目录内容
    fn readdir(&self, ino: InodeNumber) -> KernelResult<alloc::vec::Vec<DirectoryEntry>>;

    /// 创建文件
    fn create(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        mode: FileMode,
    ) -> KernelResult<InodeNumber>;

    /// 创建目录
    fn mkdir(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        mode: FileMode,
    ) -> KernelResult<InodeNumber>;

    /// 删除文件
    fn unlink(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()>;

    /// 删除目录
    fn rmdir(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<()>;

    /// 创建符号链接
    fn symlink(
        &self,
        parent_ino: InodeNumber,
        name: &[u8],
        target: &[u8],
    ) -> KernelResult<InodeNumber>;

    /// 读取符号链接目标
    fn readlink(&self, ino: InodeNumber, buf: &mut [u8]) -> KernelResult<usize>;
}

// ============================================================
// 错误类型映射
// ============================================================

/// 文件系统特定的错误
/// 为什么定义 FsError：提供比 KernelError 更细致的文件系统错误信息
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// 文件不存在
    NoEntry,
    /// 没有权限
    PermissionDenied,
    /// 文件已存在
    Exists,
    /// 不是目录
    NotDirectory,
    /// 是目录而非文件
    IsDirectory,
    /// 目录不为空
    DirectoryNotEmpty,
    /// 无效的参数
    InvalidArgument,
    /// 内存不足
    OutOfMemory,
    /// 设备/资源忙
    Busy,
    /// 符号链接循环
    SymlinkLoop,
    /// I/O 错误
    IoError,
    /// 只读文件系统
    ReadOnlyFilesystem,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NoEntry => write!(f, "No such file or directory"),
            FsError::PermissionDenied => write!(f, "Permission denied"),
            FsError::Exists => write!(f, "File exists"),
            FsError::NotDirectory => write!(f, "Not a directory"),
            FsError::IsDirectory => write!(f, "Is a directory"),
            FsError::DirectoryNotEmpty => write!(f, "Directory not empty"),
            FsError::InvalidArgument => write!(f, "Invalid argument"),
            FsError::OutOfMemory => write!(f, "Out of memory"),
            FsError::Busy => write!(f, "Device or resource busy"),
            FsError::SymlinkLoop => write!(f, "Too many levels of symbolic links"),
            FsError::IoError => write!(f, "Input/output error"),
            FsError::ReadOnlyFilesystem => write!(f, "Read-only file system"),
        }
    }
}

/// VFS 操作结果
pub type VfsResult<T> = Result<T, FsError>;
