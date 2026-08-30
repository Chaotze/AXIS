// ============================================================
// Slab 缓存管理（Cache Registry）
// ============================================================
// 在 SLUB 核心之上提供「缓存注册表」：内核复合对象（如 inode、PCB、
// 信号量等）可以显式创建命名缓存，长期复用同一批 slab 页，避免
// 反复向页提供者取还页的开销。
//
// 与 kmalloc 的关系：
// - kmalloc 的固定尺寸桶也是本注册表中的缓存（占用前几个槽位）
// - kfree 通过对象所在页头里的 cache_id 定位缓存，因此所有缓存
//   必须在同一个注册表里，id 全局唯一
//
// 为什么用固定容量数组而非动态容器：
// - 注册表是分配器自身的一部分，必须能工作于堆就绪之前（自举）
// - 缓存数量在编译期可预估（数十个以内），固定容量足够且零分配

use super::slub::{PageProvider, SlabCache};

/// 缓存注册表容量（9 个 kmalloc 尺寸桶 + 若干用户缓存）
pub const MAX_CACHES: usize = 16;
/// kmalloc 尺寸桶占用的起始槽位
pub const KMALLOC_BASE_ID: u32 = 0;
/// 用户自定义缓存起始槽位
pub const USER_CACHE_START: usize = 10;

/// slab 缓存注册表
pub struct CacheSet {
    caches: [SlabCache; MAX_CACHES],
    /// 槽位是否已被创建
    slots: [bool; MAX_CACHES],
    /// 缓存名（统计输出）
    names: [&'static str; MAX_CACHES],
}

impl CacheSet {
    /// 未初始化占位
    pub const fn uninit() -> Self {
        Self {
            caches: [
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
                SlabCache::uninit(),
            ],
            slots: [false; MAX_CACHES],
            names: [""; MAX_CACHES],
        }
    }

    /// 在指定槽位创建缓存（kmalloc 桶使用固定槽位）
    ///
    /// 为什么返回 bool 而非 Result：槽位被占用/参数无效直接返回 false，
    /// 由调用方（kmalloc/heap 初始化）处理，避免引入错误类型依赖。
    pub fn create_at(
        &mut self,
        id: usize,
        name: &'static str,
        request_size: usize,
        align: usize,
        page_size: usize,
    ) -> bool {
        if id >= MAX_CACHES || self.slots[id] {
            return false;
        }
        self.caches[id].init(
            id as u32,
            name,
            request_size,
            align,
            page_size,
        );
        self.slots[id] = true;
        self.names[id] = name;
        true
    }

    /// 自动分配一个空闲槽位创建缓存（用户命名缓存）
    pub fn create(
        &mut self,
        name: &'static str,
        request_size: usize,
        align: usize,
        page_size: usize,
    ) -> Option<usize> {
        let id = (USER_CACHE_START..MAX_CACHES).find(|&i| !self.slots[i])?;
        self.create_at(id, name, request_size, align, page_size).then_some(id)
    }

    /// 销毁缓存（要求无未释放对象）
    pub fn destroy(&mut self, id: usize) -> bool {
        if id >= MAX_CACHES || !self.slots[id] {
            return false;
        }
        // 还有未释放对象不能销毁：统计异常应被显式发现
        if self.caches[id].objs_allocated() != 0 || self.caches[id].slab_pages() != 0 {
            return false;
        }
        self.slots[id] = false;
        self.names[id] = "";
        true
    }

    /// 缓存是否存在
    #[inline]
    pub fn exists(&self, id: usize) -> bool {
        id < MAX_CACHES && self.slots[id]
    }

    /// 获取缓存名
    #[inline]
    pub fn name(&self, id: usize) -> &'static str {
        if self.exists(id) { self.names[id] } else { "<invalid>" }
    }

    /// 从缓存分配对象（加锁由调用方负责）
    pub fn alloc_object(&mut self, id: usize, provider: &mut dyn PageProvider) -> *mut u8 {
        if !self.exists(id) {
            return core::ptr::null_mut();
        }
        self.caches[id].alloc(provider)
    }

    /// 归还对象（加锁由调用方负责）；返回是否成功找到缓存
    pub fn free_object(&mut self, id: usize, provider: &mut dyn PageProvider, ptr: *mut u8) -> bool {
        if !self.exists(id) {
            return false;
        }
        self.caches[id].free(provider, ptr)
    }

    /// 迭代已创建的缓存（统计输出用）
    pub fn iter(&self) -> impl Iterator<Item = (usize, &SlabCache)> {
        (0..MAX_CACHES).filter(move |&i| self.slots[i]).map(move |i| (i, &self.caches[i]))
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    // 复用 slub 测试桩（宿主测试才编译）
    use super::super::slub::FakeProvider;

    #[test]
    fn test_create_destroy() {
        let mut set = CacheSet::uninit();
        assert!(!set.exists(2));
        assert!(set.create_at(2, "tmp", 12, 8, 4096));
        assert!(set.exists(2));
        assert_eq!(set.name(2), "tmp");
        // 重复创建同槽位失败
        assert!(!set.create_at(2, "dup", 12, 8, 4096));
        // 自动槽位
        let id = set.create("user-cache", 48, 8, 4096).expect("auto slot");
        assert!(id >= USER_CACHE_START);
        // 销毁：有对象时不能销毁
        let mut provider = FakeProvider::new(2);
        let p = set.alloc_object(id, &mut provider);
        assert!(!p.is_null());
        assert!(!set.destroy(id), "有未释放对象不能销毁");
        assert!(set.free_object(id, &mut provider, p));
        assert!(set.destroy(id));
        assert!(!set.exists(id));
    }

    #[test]
    fn test_alloc_roundtrip_across_caches() {
        let mut set = CacheSet::uninit();
        set.create_at(0, "a", 16, 8, 4096);
        set.create_at(1, "b", 64, 8, 4096);
        let mut provider = FakeProvider::new(4);
        let pa = set.alloc_object(0, &mut provider);
        let pb = set.alloc_object(1, &mut provider);
        assert!(!pa.is_null() && !pb.is_null());
        // 跨缓存释放：free_object 必须路由到各自缓存
        assert!(set.free_object(1, &mut provider, pb));
        assert!(set.free_object(0, &mut provider, pa));
        // 非法 id 拒绝
        assert!(set.alloc_object(99, &mut provider).is_null());
        assert!(!set.free_object(99, &mut provider, pa));
    }
}