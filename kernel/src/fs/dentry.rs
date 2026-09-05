// ============================================================
// 目录项缓存（Dentry）
// ============================================================
// 定义目录项在内存中的表示，支持快速的路径查询。
// 本模块为纯逻辑层，Dentry 结构本身无缓存管理，
// 缓存管理由 dcache.rs 负责。

use crate::fs::vfs::InodeNumber;
use alloc::boxed::Box;

// ============================================================
// 目录项键（用于哈希表或 BTreeMap）
// ============================================================

/// Dentry 缓存键
/// 为什么用元组键：结合文件名和父 inode 号保证唯一性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DentryKey {
    /// 父 inode 号
    pub parent_ino: InodeNumber,
    /// 文件名哈希（减少内存占用）
    pub name_hash: u64,
}

impl DentryKey {
    /// 创建新的 dentry 键
    pub fn new(parent_ino: InodeNumber, name: &[u8]) -> Self {
        DentryKey {
            parent_ino,
            name_hash: hash_name(name),
        }
    }
}

/// 计算文件名的哈希值
/// 为什么用哈希而非完整字符串：减少内存占用，同时保证高概率唯一性
fn hash_name(name: &[u8]) -> u64 {
    let mut hash: u64 = 5381;  // DJB2 哈希初值
    for &byte in name {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

// ============================================================
// 目录项结构
// ============================================================

/// 内存中的目录项（Dentry）
/// 为什么需要这个结构：
/// - 缓存从路径名到 inode 号的映射
/// - 支持快速的路径查询而无需每次都调用文件系统
/// - 跟踪目录项的有效性（缓存可能失效）
#[derive(Debug, Clone)]
pub struct Dentry {
    /// 父 inode 号（0 表示无父，如根目录）
    pub parent_ino: InodeNumber,
    /// 当前 Inode 号
    pub inode_number: InodeNumber,
    /// 文件名（定长避免堆分配）
    pub name: [u8; 256],
    /// 文件名实际长度
    pub name_len: usize,
    /// 是否有效（缓存可能失效）
    pub valid: bool,
    /// 是否为"负缓存"（文件不存在）
    /// 为什么需要负缓存：某些文件系统操作频繁查询不存在的文件，
    /// 缓存"不存在"可以避免重复查询
    pub negative: bool,
    /// 引用计数（用户和缓存的引用总数）
    pub refcount: u32,
}

impl Dentry {
    /// 创建新的 dentry
    pub fn new(
        parent_ino: InodeNumber,
        inode_number: InodeNumber,
        name: &[u8],
    ) -> crate::lib::result::KernelResult<Self> {
        // 为什么检查名称长度：255 是 POSIX 标准限制
        if name.len() > 255 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        let mut name_arr = [0u8; 256];
        name_arr[..name.len()].copy_from_slice(name);

        Ok(Dentry {
            parent_ino,
            inode_number,
            name: name_arr,
            name_len: name.len(),
            valid: true,
            negative: false,
            refcount: 1,
        })
    }

    /// 创建负缓存目录项（表示文件不存在）
    pub fn new_negative(
        parent_ino: InodeNumber,
        name: &[u8],
    ) -> crate::lib::result::KernelResult<Self> {
        let mut dentry = Self::new(parent_ino, 0, name)?;
        dentry.negative = true;
        Ok(dentry)
    }

    /// 获取文件名切片
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// 获取文件名字符串（假设 UTF-8）
    pub fn name_str(&self) -> Option<&str> {
        core::str::from_utf8(self.name()).ok()
    }

    /// 检查名称是否匹配
    /// 为什么需要单独函数：避免重复的切片操作和比较
    pub fn name_matches(&self, other: &[u8]) -> bool {
        self.name() == other
    }

    /// 增加引用计数
    /// 为什么检查溢出：防止计数器溢出（虽然极少见）
    pub fn inc_refcount(&mut self) -> crate::lib::result::KernelResult<()> {
        if self.refcount >= u32::MAX {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }
        self.refcount += 1;
        Ok(())
    }

    /// 减少引用计数
    pub fn dec_refcount(&mut self) -> crate::lib::result::KernelResult<()> {
        if self.refcount == 0 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }
        self.refcount -= 1;
        Ok(())
    }

    /// 检查 dentry 是否可以被释放
    /// 规则：引用计数为 0 时可以释放
    pub fn can_free(&self) -> bool {
        self.refcount == 0
    }

    /// 失效化 dentry（标记为需要重新验证）
    /// 为什么需要失效化：文件系统状态改变（如删除文件），缓存需要失效
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// 重新验证 dentry（标记为有效）
    pub fn revalidate(&mut self) {
        self.valid = true;
    }
}

// ============================================================
// Dentry 比较
// ============================================================

impl PartialEq for Dentry {
    fn eq(&self, other: &Self) -> bool {
        self.parent_ino == other.parent_ino && self.name() == other.name()
    }
}

impl Eq for Dentry {}

// ============================================================
// Dentry 操作工具
// ============================================================

/// Dentry 池（内存池）
/// 为什么使用对象池：频繁的目录项创建/销毁会产生大量内存碎片
pub struct DentryPool {
    /// 空闲 dentry 列表（栈形式，LIFO）
    free_list: alloc::vec::Vec<Box<Dentry>>,
    /// 已分配但未返回的 dentry 数
    allocated_count: usize,
}

impl DentryPool {
    /// 创建新的 dentry 池
    pub fn new() -> Self {
        DentryPool {
            free_list: alloc::vec::Vec::new(),
            allocated_count: 0,
        }
    }

    /// 从池中分配一个 dentry
    pub fn allocate(
        &mut self,
        parent_ino: InodeNumber,
        inode_number: InodeNumber,
        name: &[u8],
    ) -> crate::lib::result::KernelResult<Box<Dentry>> {
        // 为什么先查看空闲列表：重用比新建更快
        if let Some(mut dentry) = self.free_list.pop() {
            // 为什么重置 dentry：确保从干净的状态开始
            dentry.parent_ino = parent_ino;
            dentry.inode_number = inode_number;
            dentry.valid = true;
            dentry.negative = false;
            dentry.refcount = 1;

            if name.len() > 255 {
                return Err(crate::lib::result::KernelError::InvalidArgument);
            }
            dentry.name[..name.len()].copy_from_slice(name);
            dentry.name_len = name.len();

            self.allocated_count += 1;
            Ok(dentry)
        } else {
            // 为什么新建 dentry：池为空，需要分配新对象
            let dentry = Dentry::new(parent_ino, inode_number, name)?;
            self.allocated_count += 1;
            Ok(Box::new(dentry))
        }
    }

    /// 将 dentry 返回到池中
    pub fn free(&mut self, dentry: Box<Dentry>) {
        self.allocated_count = self.allocated_count.saturating_sub(1);
        self.free_list.push(dentry);
    }

    /// 获取已分配的 dentry 数
    pub fn allocated_count(&self) -> usize {
        self.allocated_count
    }

    /// 获取空闲的 dentry 数
    pub fn free_count(&self) -> usize {
        self.free_list.len()
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dentry_creation() {
        let dentry = Dentry::new(1, 2, b"test.txt").unwrap();
        assert_eq!(dentry.parent_ino, 1);
        assert_eq!(dentry.inode_number, 2);
        assert_eq!(dentry.name(), b"test.txt");
        assert!(!dentry.negative);
        assert!(dentry.valid);
        assert_eq!(dentry.refcount, 1);
    }

    #[test]
    fn test_negative_dentry() {
        let dentry = Dentry::new_negative(1, b"nonexistent.txt").unwrap();
        assert!(dentry.negative);
        assert_eq!(dentry.inode_number, 0);
    }

    #[test]
    fn test_dentry_key() {
        let key1 = DentryKey::new(1, b"test");
        let key2 = DentryKey::new(1, b"test");
        assert_eq!(key1, key2);

        let key3 = DentryKey::new(1, b"test2");
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_refcount_operations() {
        let mut dentry = Dentry::new(1, 2, b"test.txt").unwrap();
        assert_eq!(dentry.refcount, 1);

        assert!(dentry.inc_refcount().is_ok());
        assert_eq!(dentry.refcount, 2);

        assert!(dentry.dec_refcount().is_ok());
        assert_eq!(dentry.refcount, 1);

        assert!(dentry.dec_refcount().is_ok());
        assert_eq!(dentry.refcount, 0);
        assert!(dentry.can_free());
    }

    #[test]
    fn test_dentry_pool() {
        let mut pool = DentryPool::new();
        assert_eq!(pool.allocated_count(), 0);

        let dentry1 = pool.allocate(1, 2, b"test").unwrap();
        assert_eq!(pool.allocated_count(), 1);

        let dentry2 = pool.allocate(1, 3, b"test2").unwrap();
        assert_eq!(pool.allocated_count(), 2);

        pool.free(dentry2);
        assert_eq!(pool.allocated_count(), 1);
        assert_eq!(pool.free_count(), 1);

        let dentry3 = pool.allocate(1, 4, b"test3").unwrap();
        // 为什么 allocated_count 还是 1：从池中重用了一个 dentry
        assert_eq!(pool.allocated_count(), 2);
    }
}
