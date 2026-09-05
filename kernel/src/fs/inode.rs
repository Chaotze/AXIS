// ============================================================
// Inode 元数据和操作
// ============================================================
// 定义 Inode 在内存中的表示和权限检查逻辑。
// 本模块为纯逻辑层，支持独立的单元测试。

use crate::lib::result::KernelResult;
use crate::fs::vfs::{FileType, InodeMetadata};

// ============================================================
// Inode 权限检查
// ============================================================

/// 用户身份（简化版，暂不支持组）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId {
    /// 用户 ID（UID）
    pub uid: u32,
}

impl UserId {
    /// root 用户
    pub fn root() -> Self {
        UserId { uid: 0 }
    }

    /// 是否为 root
    pub fn is_root(&self) -> bool {
        self.uid == 0
    }
}

/// 权限检查类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionType {
    /// 读权限
    Read,
    /// 写权限
    Write,
    /// 执行权限
    Execute,
}

/// 检查给定用户是否有指定权限
/// 规则（简化版）：
/// - root (uid=0) 拥有所有权限（除了检查执行权限时需要至少有一个执行位）
/// - 所有者检查对应的用户权限位
/// - 其他用户检查"其他"权限位
pub fn check_permission(
    metadata: &InodeMetadata,
    user: UserId,
    perm: PermissionType,
) -> bool {
    // 为什么特殊处理 root：Unix 约定 root 可以做几乎任何事（除了直接执行无执行位的文件）
    if user.is_root() {
        if perm == PermissionType::Execute {
            // root 执行非目录文件需要至少一个执行位
            return (metadata.mode.0 & 0o111) != 0;
        }
        return true;
    }

    // 为什么逐级检查：先检查所有者，再检查其他
    let (mask, shift) = if user.uid == metadata.uid {
        (0o7, 6)  // 所有者权限（bits 6-8）
    } else {
        (0o7, 0)  // 其他权限（bits 0-2）
    };

    let perm_bit = match perm {
        PermissionType::Read => 4,
        PermissionType::Write => 2,
        PermissionType::Execute => 1,
    };

    ((metadata.mode.0 >> shift) & mask & perm_bit) != 0
}

// ============================================================
// Inode 操作辅助函数
// ============================================================

/// 更新 Inode 的修改时间
/// 为什么独立函数：修改时间更新是常见操作，应该分离
pub fn update_mtime(metadata: &mut InodeMetadata, now: i64) {
    metadata.mtime = now;
    // ctime 应该总是被更新（任何元数据改变都会更新）
    metadata.ctime = now;
}

/// 更新 Inode 的访问时间
pub fn update_atime(metadata: &mut InodeMetadata, now: i64) {
    metadata.atime = now;
}

/// 增加硬链接计数
pub fn increment_nlink(metadata: &mut InodeMetadata) -> KernelResult<()> {
    // 为什么检查溢出：防止计数器溢出（虽然极少见）
    if metadata.nlink >= u32::MAX {
        return Err(crate::lib::result::KernelError::InvalidArgument);
    }
    metadata.nlink += 1;
    Ok(())
}

/// 减少硬链接计数
pub fn decrement_nlink(metadata: &mut InodeMetadata) -> KernelResult<()> {
    // 为什么检查下界：nlink 不能为负
    if metadata.nlink == 0 {
        return Err(crate::lib::result::KernelError::InvalidArgument);
    }
    metadata.nlink -= 1;
    Ok(())
}

// ============================================================
// Inode 状态检查
// ============================================================

/// 检查 Inode 是否可删除
/// 规则：
/// - 文件必须没有被打开（nlink >= 1 但被打开时暂不删除）
/// - 目录必须为空（由文件系统保证）
pub fn can_delete(metadata: &InodeMetadata) -> bool {
    // 为什么检查 nlink：硬链接计数决定是否真正删除
    // nlink > 0 表示仍有其他名称指向此 inode
    metadata.nlink == 1
}

/// 检查 Inode 是否应该被释放
/// 规则：
/// - 当 nlink == 0 时
/// - 且没有进程打开它（引用计数为 0）
pub fn should_free(metadata: &InodeMetadata) -> bool {
    metadata.nlink == 0
}

// ============================================================
// 文件类型判定
// ============================================================

/// 检查是否为可执行文件
pub fn is_executable(metadata: &InodeMetadata) -> bool {
    // 为什么检查文件类型：只有普通文件可以执行
    metadata.file_type == FileType::File && (metadata.mode.0 & 0o111) != 0
}

/// 检查是否为目录
pub fn is_directory(metadata: &InodeMetadata) -> bool {
    metadata.file_type == FileType::Directory
}

/// 检查是否为符号链接
pub fn is_symlink(metadata: &InodeMetadata) -> bool {
    metadata.file_type == FileType::Symlink
}

/// 检查是否为块设备
pub fn is_block_device(metadata: &InodeMetadata) -> bool {
    metadata.file_type == FileType::BlockDevice
}

/// 检查是否为字符设备
pub fn is_char_device(metadata: &InodeMetadata) -> bool {
    metadata.file_type == FileType::CharDevice
}

/// 检查是否为管道/FIFO
pub fn is_fifo(metadata: &InodeMetadata) -> bool {
    metadata.file_type == FileType::Fifo
}

// ============================================================
// 大小检查和验证
// ============================================================

/// 检查文件大小是否超过限制
/// 为什么限制：防止过大文件导致内存溢出
pub const MAX_FILE_SIZE: u64 = 1 << 40;  // 1TB（保守估计）

pub fn validate_file_size(size: u64) -> KernelResult<()> {
    if size > MAX_FILE_SIZE {
        Err(crate::lib::result::KernelError::InvalidArgument)
    } else {
        Ok(())
    }
}

/// 计算所需的块数（512字节块）
pub fn calculate_blocks(size: u64) -> u64 {
    (size + 511) / 512
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_check_owner() {
        let mut meta = InodeMetadata {
            inode_number: 1,
            file_type: FileType::File,
            size: 100,
            blocks: 1,
            mode: FileMode::new(0o644),
            uid: 100,
            gid: 100,
            nlink: 1,
            atime: 0,
            mtime: 0,
            ctime: 0,
            btime: None,
        };

        // 所有者有读权限
        assert!(check_permission(&meta, UserId { uid: 100 }, PermissionType::Read));
        // 所有者无写权限
        assert!(!check_permission(&meta, UserId { uid: 100 }, PermissionType::Write));
    }

    #[test]
    fn test_permission_check_root() {
        let meta = InodeMetadata {
            inode_number: 1,
            file_type: FileType::File,
            size: 100,
            blocks: 1,
            mode: FileMode::new(0o644),
            uid: 100,
            gid: 100,
            nlink: 1,
            atime: 0,
            mtime: 0,
            ctime: 0,
            btime: None,
        };

        // root 有所有权限
        assert!(check_permission(&meta, UserId::root(), PermissionType::Read));
        assert!(check_permission(&meta, UserId::root(), PermissionType::Write));
    }

    #[test]
    fn test_nlink_operations() {
        let mut meta = InodeMetadata {
            inode_number: 1,
            file_type: FileType::File,
            size: 100,
            blocks: 1,
            mode: FileMode::new(0o644),
            uid: 100,
            gid: 100,
            nlink: 2,
            atime: 0,
            mtime: 0,
            ctime: 0,
            btime: None,
        };

        assert!(decrement_nlink(&mut meta).is_ok());
        assert_eq!(meta.nlink, 1);

        assert!(increment_nlink(&mut meta).is_ok());
        assert_eq!(meta.nlink, 2);
    }

    #[test]
    fn test_file_type_checks() {
        let dir_meta = InodeMetadata {
            inode_number: 1,
            file_type: FileType::Directory,
            size: 4096,
            blocks: 8,
            mode: FileMode::new(0o755),
            uid: 100,
            gid: 100,
            nlink: 2,
            atime: 0,
            mtime: 0,
            ctime: 0,
            btime: None,
        };

        assert!(is_directory(&dir_meta));
        assert!(!is_executable(&dir_meta));

        let file_meta = InodeMetadata {
            inode_number: 2,
            file_type: FileType::File,
            size: 100,
            blocks: 1,
            mode: FileMode::new(0o755),
            uid: 100,
            gid: 100,
            nlink: 1,
            atime: 0,
            mtime: 0,
            ctime: 0,
            btime: None,
        };

        assert!(!is_directory(&file_meta));
        assert!(is_executable(&file_meta));
    }
}
