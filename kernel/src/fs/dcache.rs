// ============================================================
// 目录项缓存（Dentry Cache）
// ============================================================
// 实现基于 LRU 的 Dentry 缓存管理，加速路径查询。
// 使用下标池而非 Arc，避免 Copy 约束问题。

use crate::fs::dentry::{Dentry, DentryKey};
use crate::fs::vfs::InodeNumber;
use alloc::boxed::Box;

// ============================================================
// Dentry 下标
// ============================================================

/// Dentry 下标（用于引用缓存中的项）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DentryIndex(pub usize);

// ============================================================
// Dentry 缓存管理
// ============================================================

/// Dentry 缓存
/// 为什么用下标而非 Arc：
/// - LruCache 要求值实现 Copy
/// - 用下标代替，实际对象存储在 Vec 中
pub struct DentryCache {
    /// LRU 缓存（键 → 下标）
    cache: crate::lib::collections::LruCache<DentryKey, DentryIndex>,
    /// Dentry 对象存储池
    storage: alloc::vec::Vec<Box<Dentry>>,
    /// 缓存命中次数（统计用）
    hits: u64,
    /// 缓存缺失次数（统计用）
    misses: u64,
}

impl DentryCache {
    /// 创建新的 dentry 缓存
    pub fn new(capacity: usize) -> Self {
        DentryCache {
            cache: crate::lib::collections::LruCache::with_capacity(capacity),
            storage: alloc::vec::Vec::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// 在缓存中查找 dentry（返回引用）
    pub fn get(&mut self, key: &DentryKey) -> Option<&Dentry> {
        if let Some(idx) = self.cache.get(key) {
            self.hits += 1;
            self.storage.get(idx.0).map(|d| d.as_ref())
        } else {
            self.misses += 1;
            None
        }
    }

    /// 在缓存中查找但不改变 LRU 顺序
    pub fn peek(&mut self, key: &DentryKey) -> Option<&Dentry> {
        if let Some(idx) = self.cache.get(key) {
            self.storage.get(idx.0).map(|d| d.as_ref())
        } else {
            None
        }
    }

    /// 插入 dentry 到缓存
    pub fn insert(&mut self, key: DentryKey, dentry: Box<Dentry>) -> Option<Box<Dentry>> {
        // 为什么检查有效性：缓存无效的 dentry 没有意义
        if !dentry.valid {
            return None;
        }

        let idx = DentryIndex(self.storage.len());
        self.storage.push(dentry);

        // 为什么忽略旧值：LruCache 返回的是下标，被淘汰的对象仍在存储中
        let _ = self.cache.put(key, idx);
        None
    }

    /// 从缓存中删除指定的 dentry
    pub fn remove(&mut self, key: &DentryKey) -> Option<Box<Dentry>> {
        if let Some(idx) = self.cache.remove(key) {
            self.storage.get(idx.0).cloned()
        } else {
            None
        }
    }

    /// 清空整个缓存
    pub fn clear(&mut self) {
        // LruCache 没有 clear 方法，重新创建一个新的
        self.cache = crate::lib::collections::LruCache::with_capacity(self.cache.capacity());
        self.storage.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// 检查缓存中是否存在指定的 dentry
    pub fn contains(&self, key: &DentryKey) -> bool {
        self.cache.contains(key)
    }

    /// 获取缓存的容量
    pub fn capacity(&self) -> usize {
        self.cache.capacity()
    }

    /// 获取缓存的当前大小
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 检查缓存是否为空
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 获取缓存统计信息
    pub fn stats(&self) -> DentryCacheStats {
        let total = self.hits + self.misses;
        let hit_rate = if total > 0 {
            (self.hits * 100) / total
        } else {
            0
        };

        DentryCacheStats {
            hits: self.hits,
            misses: self.misses,
            hit_rate,
            size: self.cache.len(),
            capacity: self.cache.capacity(),
        }
    }

    /// 失效化缓存中的所有 dentry（标记为需要重新验证）
    pub fn invalidate_all(&mut self) {
        for dentry in &mut self.storage {
            dentry.invalidate();
        }
    }

    /// 根据父 inode 号失效化所有相关的 dentry
    pub fn invalidate_parent(&mut self, _parent_ino: InodeNumber) {
        // 为什么这样做：遍历缓存，标记所有属于该父目录的 dentry 为无效
        for dentry in &mut self.storage {
            if dentry.parent_ino == _parent_ino {
                dentry.invalidate();
            }
        }
    }
}

/// Dentry 缓存统计信息
#[derive(Debug, Clone, Copy)]
pub struct DentryCacheStats {
    /// 缓存命中次数
    pub hits: u64,
    /// 缓存缺失次数
    pub misses: u64,
    /// 命中率（百分比）
    pub hit_rate: u64,
    /// 当前缓存大小
    pub size: usize,
    /// 缓存容量
    pub capacity: usize,
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dcache_insert_and_get() {
        let mut cache = DentryCache::new(10);
        let key = DentryKey::new(1, b"test.txt");
        let dentry = Dentry::new(1, 2, b"test.txt").unwrap();

        cache.insert(key, Box::new(dentry));
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_dcache_miss() {
        let mut cache = DentryCache::new(10);
        let key = DentryKey::new(1, b"nonexistent");

        assert!(cache.get(&key).is_none());
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn test_dcache_stats() {
        let mut cache = DentryCache::new(10);
        let key = DentryKey::new(1, b"test");
        let dentry = Dentry::new(1, 2, b"test").unwrap();

        cache.insert(key, Box::new(dentry));
        cache.get(&key);  // 1 hit
        cache.get(&key);  // 2 hits
        let _ = cache.get(&DentryKey::new(1, b"other"));  // 1 miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate, 66);  // 2/(2+1) * 100 = 66%
    }
}
