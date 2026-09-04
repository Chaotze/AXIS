// ============================================================
// 页缓存（Page Cache）
// ============================================================
// 实现基于下标池的页缓存，用于缓存文件内容到内存。
// 使用下标而非 Arc 以避免 Copy 约束。

use crate::fs::vfs::{FileOffset, InodeNumber};
use crate::lib::result::KernelResult;

// ============================================================
// 页结构
// ============================================================

/// 内存中的缓存页
/// 为什么用专门的 Page 结构：
/// - 页可能有多个使用者（引用计数）
/// - 需要跟踪页的脏标记（是否需要写回）
/// - 需要跟踪页的锁定状态
#[derive(Debug, Clone)]
pub struct CachedPage {
    /// 页的偏移（字节，页内偏移）
    pub offset: FileOffset,
    /// 页的内容（4KB）
    pub data: [u8; 4096],
    /// 是否为脏页（需要写回磁盘）
    pub dirty: bool,
    /// 引用计数
    pub refcount: u32,
}

impl CachedPage {
    /// 创建新的缓存页
    pub fn new(offset: FileOffset) -> Self {
        CachedPage {
            offset,
            data: [0u8; 4096],
            dirty: false,
            refcount: 1,
        }
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

    /// 标记为脏页
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 清除脏标记
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

// ============================================================
// 页缓存管理
// ============================================================

// ============================================================
// 页缓存管理
// ============================================================

/// 页下标（用于引用缓存中的页）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageIndex(pub usize);

/// 单个 inode 的页缓存
pub struct PageCache {
    /// 关联的 inode 号
    pub inode_number: InodeNumber,
    /// 页存储池（简化版，用 Vec 而非 RadixTree）
    pages: alloc::vec::Vec<CachedPage>,
    /// 页到下标的映射（文件偏移 → 下标）
    page_map: crate::lib::collections::BTreeMap<FileOffset, PageIndex, 256, 257>,
    /// 缓存统计
    stats: PageCacheStats,
}

impl PageCache {
    /// 创建新的页缓存
    pub fn new(inode_number: InodeNumber) -> Self {
        PageCache {
            inode_number,
            pages: alloc::vec::Vec::new(),
            page_map: crate::lib::collections::BTreeMap::new(),
            stats: PageCacheStats::new(),
        }
    }

    /// 在缓存中查找页
    pub fn get(&mut self, offset: FileOffset) -> Option<&CachedPage> {
        if let Some(idx) = self.page_map.get(&offset) {
            self.stats.hits += 1;
            self.pages.get(idx.0)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// 在缓存中查找但不改变统计（简化版）
    pub fn peek(&self, offset: FileOffset) -> Option<&CachedPage> {
        if let Some(idx) = self.page_map.get(&offset) {
            self.pages.get(idx.0).map(|p| p)
        } else {
            None
        }
    }

    /// 插入页到缓存
    pub fn insert(&mut self, offset: FileOffset, mut page: CachedPage) -> Option<CachedPage> {
        page.offset = offset;
        let idx = PageIndex(self.pages.len());
        self.pages.push(page);
        let _ = self.page_map.insert(offset, idx);
        None
    }

    /// 从缓存中删除页
    pub fn remove(&mut self, offset: FileOffset) -> Option<CachedPage> {
        if let Some(idx) = self.page_map.remove(&offset) {
            self.pages.get(idx.0).cloned()
        } else {
            None
        }
    }

    /// 清空缓存
    pub fn clear(&mut self) {
        self.pages.clear();
        self.page_map = crate::lib::collections::BTreeMap::new();
    }

    /// 获取缓存统计信息
    pub fn stats(&self) -> PageCacheStats {
        self.stats.clone()
    }

    /// 获取所有脏页
    pub fn get_dirty_pages(&self) -> alloc::vec::Vec<&CachedPage> {
        self.pages.iter()
            .filter(|p| p.dirty)
            .collect()
    }

    /// 写回所有脏页
    pub fn flush(&mut self) -> KernelResult<()> {
        // 注意：此函数需要文件系统的支持（调用 write callback）
        // 此处为概念设计
        self.stats.flushes += 1;
        Ok(())
    }
}

/// 页缓存统计信息
#[derive(Debug, Clone, Copy)]
pub struct PageCacheStats {
    /// 缓存命中次数
    pub hits: u64,
    /// 缓存缺失次数
    pub misses: u64,
    /// 写入次数
    pub writes: u64,
    /// 写回次数
    pub flushes: u64,
}

impl PageCacheStats {
    /// 创建新的统计对象
    pub fn new() -> Self {
        PageCacheStats {
            hits: 0,
            misses: 0,
            writes: 0,
            flushes: 0,
        }
    }

    /// 获取命中率（百分比）
    pub fn hit_rate(&self) -> u64 {
        let total = self.hits + self.misses;
        if total > 0 {
            (self.hits * 100) / total
        } else {
            0
        }
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_page_creation() {
        let page = CachedPage::new(0);
        assert_eq!(page.offset, 0);
        assert!(!page.dirty);
        assert_eq!(page.refcount, 1);
    }

    #[test]
    fn test_page_cache_insert_and_get() {
        let mut cache = PageCache::new(1);
        let page = Arc::new(CachedPage::new(0));

        cache.insert(0, page.clone());
        assert!(cache.get(0).is_some());
    }

    #[test]
    fn test_page_cache_stats() {
        let mut cache = PageCache::new(1);
        let page = Arc::new(CachedPage::new(0));

        cache.insert(0, page);
        cache.get(0);  // hit
        cache.get(0);  // hit
        let _ = cache.get(4096);  // miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 66);  // 2/(2+1) * 100 = 66%
    }

    #[test]
    fn test_page_cache_write() {
        let mut cache = PageCache::new(1);
        let data = b"Hello, World!";

        let written = cache.write(0, data).unwrap();
        assert_eq!(written, data.len());
        assert_eq!(cache.stats().writes, 1);
    }
}
