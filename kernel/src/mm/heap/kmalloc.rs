// ============================================================
// 内核内存分配接口（kmalloc / kfree）
// ============================================================
// 常规内核对象的分配入口：
// - 小对象（<= 2048 字节）：尺寸桶（size bucket）路由到对应 Slab 缓存，
//   同尺寸对象共享 slab 页，碎片极小
// - 大对象 / 高对齐：直接向页提供者申请连续页（伙伴系统 order 块）
//
// 设计要点（为什么这么做）：
// 1. kfree 无需调用方告知尺寸：对象所在页的页头魔数 + cache_id 即可
//    定位缓存（slab 对象）；大对象则查直接分配登记表。这使 kfree
//    保持 C 语言风格的单指针签名，同时保持类型安全。
// 2. 尺寸桶是 2 的幂：向上取整到最近的桶即可，且每个桶的对象地址
//    天然满足该尺寸的对齐要求（stride 本身是 2 的幂）。
// 3. 直接分配按「保底一页余量」的多页块申请，页内手工对齐，登记表
//    记录原始页基址以便释放。

use super::slab_cache::{CacheSet, KMALLOC_BASE_ID};
use super::slub::{align_up, PageProvider, SLAB_MAGIC};

/// 内核页大小（字节）
pub const KERNEL_PAGE_SIZE: usize = 4096;

/// 尺寸桶列表（2 的幂阶梯）
pub const KMALLOC_BUCKETS: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
/// 桶数量
pub const KMALLOC_BUCKET_COUNT: usize = 9;
/// 超过该尺寸走直接页分配
pub const DIRECT_LIMIT: usize = 2048;
/// 常规桶支持的最大对象对齐（超过则走直接分配，保证对齐正确）
pub const BUCKET_ALIGN: usize = 8;
/// 直接分配登记表容量（并发对象数量上限；内核阶段远用不满）
pub const DIRECT_MAX: usize = 32;

/// 直接分配登记条目
#[derive(Clone, Copy)]
struct DirectEntry {
    /// 用户可见的起始地址
    start: usize,
    /// 页块基址（page-aligned）
    base: usize,
    /// 页块页数
    pages: usize,
    /// 槽位是否占用
    used: bool,
}

/// 大对象/直接分配登记表
struct DirectTable {
    entries: [DirectEntry; DIRECT_MAX],
}

impl DirectTable {
    const fn uninit() -> Self {
        Self {
            entries: [DirectEntry {
                start: 0,
                base: 0,
                pages: 0,
                used: false,
            }; DIRECT_MAX],
        }
    }

    fn insert(&mut self, start: usize, base: usize, pages: usize) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| !e.used) {
            *e = DirectEntry { start, base, pages, used: true };
            true
        } else {
            false
        }
    }

    fn remove(&mut self, start: usize) -> Option<(usize, usize)> {
        let e = self.entries.iter_mut().find(|e| e.used && e.start == start)?;
        e.used = false;
        Some((e.base, e.pages))
    }

}

/// kmalloc 状态（全局唯一实例由 heap.rs 作为 Spinlock 持有）
pub struct Kmalloc {
    /// 缓存注册表（桶 + 用户缓存）
    set: CacheSet,
    /// 直接分配登记表
    direct: DirectTable,
    /// 桶是否已初始化
    buckets_ready: bool,
    /// 统计：直接分配在用的对象数
    direct_used: usize,
}

impl Kmalloc {
    /// 未初始化占位
    pub const fn uninit() -> Self {
        Self {
            set: CacheSet::uninit(),
            direct: DirectTable::uninit(),
            buckets_ready: false,
            direct_used: 0,
        }
    }

    /// 就绪（创建全部尺寸桶）；由 heap::init 在堆启用前调用
    pub fn ensure_buckets(&mut self) {
        if self.buckets_ready {
            return;
        }
        const PAGE: usize = KERNEL_PAGE_SIZE;
        for (i, size) in KMALLOC_BUCKETS.iter().enumerate() {
            let ok = self.set.create_at(
                i as usize,
                bucket_name(*size),
                *size,
                BUCKET_ALIGN,
                PAGE,
            );
            debug_assert!(ok, "kmalloc 桶初始化失败");
        }
        self.buckets_ready = true;
    }

    /// 是否就绪
    #[inline]
    pub const fn is_ready(&self) -> bool {
        self.buckets_ready
    }

    /// 返回可容纳 size 的桶下标
    #[inline]
    pub fn bucket_for(size: usize) -> Option<usize> {
        if size > DIRECT_LIMIT {
            return None;
        }
        KMALLOC_BUCKETS.iter().position(|&b| size <= b)
    }

    /// 分配 size 字节（默认 8 字节对齐），失败返回 null
    pub fn kmalloc(&mut self, size: usize, provider: &mut dyn PageProvider) -> *mut u8 {
        self.kmalloc_aligned(size, BUCKET_ALIGN, provider)
    }

    /// 分配 size 字节并按 align 对齐（2 的幂），失败返回 null
    pub fn kmalloc_aligned(
        &mut self,
        size: usize,
        align: usize,
        provider: &mut dyn PageProvider,
    ) -> *mut u8 {
        if size == 0 {
            return core::ptr::null_mut();
        }
        let align = align.max(1);

        // 桶路径：尺寸桶足够且对齐要求不高
        if align <= BUCKET_ALIGN {
            if let Some(bucket) = Self::bucket_for(size) {
                return self.set.alloc_object(bucket, provider);
            }
        }

        // 直接路径：连续多页 + 页内对齐
        self.direct_alloc(size, align, provider)
    }

    /// 连续页直接分配
    fn direct_alloc(
        &mut self,
        size: usize,
        align: usize,
        provider: &mut dyn PageProvider,
    ) -> *mut u8 {
        const PAGE: usize = KERNEL_PAGE_SIZE;
        // 需要满足 size 的连续字节，外加最多一页的对齐余量
        let needed = align_up(size, PAGE) + PAGE;
        let order = ilog2_ceil(needed / PAGE);
        let pages = 1usize << order;
        let Some(base) = provider.alloc_pages(order) else {
            return core::ptr::null_mut();
        };
        let end = base + pages * PAGE;
        // 在 [base, end) 内找到 align 对齐的地址（保底有 PAGE 余量）
        let start = align_up(base, align);
        if start + size > end {
            // 理论不会发生（余量 >= PAGE >= align），防御性回退
            provider.free_pages(order, base);
            return core::ptr::null_mut();
        }
        if !self.direct.insert(start, base, pages) {
            provider.free_pages(order, base);
            return core::ptr::null_mut();
        }
        self.direct_used += 1;
        start as *mut u8
    }

    /// 分配并清零
    pub fn kmalloc_zeroed(&mut self, size: usize, provider: &mut dyn PageProvider) -> *mut u8 {
        let p = self.kmalloc(size, provider);
        if !p.is_null() {
            // 桶路径对象步长与 size 可能不同（向上取整），清零按 size
            unsafe { core::ptr::write_bytes(p, 0, size) };
        }
        p
    }

    /// 分配并清零（带对齐）
    pub fn kmalloc_zeroed_with_align(
        &mut self,
        size: usize,
        align: usize,
        provider: &mut dyn PageProvider,
    ) -> *mut u8 {
        let p = self.kmalloc_aligned(size, align, provider);
        if !p.is_null() {
            unsafe { core::ptr::write_bytes(p, 0, size) };
        }
        p
    }

    /// 释放 kmalloc 分配的指针（无需尺寸），返回是否成功识别
    pub fn kfree(&mut self, ptr: *mut u8, provider: &mut dyn PageProvider) -> bool {
        if ptr.is_null() {
            return false;
        }
        let addr = ptr as usize;
        let page = addr & !(KERNEL_PAGE_SIZE - 1);

        // 1) slab 对象：页头魔数识别
        //
        // 为什么不在魔数命中但归属校验失败时回退查直接登记表：
        // 直接登记表里的页面若已“改嫁”成为 slab 页（页头有效），
        // 回退释放会误归还真正被 slab 占用的页，造成双所有权。
        // 残留魔数问题由 SLUB 归还页面前的页头擦除 + 页复用清零
        // 双保险解决，而不是靠 kfree 的模糊判定。
        let magic = unsafe { core::ptr::read_volatile(page as *const u32) };
        if magic == SLAB_MAGIC {
            let cache_id = unsafe { core::ptr::read_volatile((page + 4) as *const u32) };
            if cache_id >= KMALLOC_BASE_ID && cache_id < super::slab_cache::MAX_CACHES as u32 {
                return self.set.free_object(cache_id as usize, provider, ptr);
            }
            return false;
        }

        // 2) 直接分配：查登记表
        if let Some((base, pages)) = self.direct.remove(addr) {
            // 归还连续页块（登记表记录了分配时的阶与基址）
            provider.free_pages(ilog2_floor(pages), base);
            self.direct_used = self.direct_used.saturating_sub(1);
            return true;
        }

        false
    }

    /// 缓存注册表（只读）
    #[inline]
    pub const fn cache_set(&self) -> &CacheSet {
        &self.set
    }

    /// 缓存注册表（可变）
    #[inline]
    pub fn cache_set_mut(&mut self) -> &mut CacheSet {
        &mut self.set
    }

    /// 直接登记表在用在册数
    #[inline]
    pub const fn direct_used(&self) -> usize {
        self.direct_used
    }

    /// 总对象数（桶 + 直接）
    pub fn total_objects(&self) -> usize {
        let buckets: usize = self
            .set
            .iter()
            .map(|(_, c)| c.objs_allocated())
            .sum();
        buckets + self.direct_used
    }
}

#[inline]
pub const fn ilog2_ceil(n: usize) -> usize {
    let mut v = if n == 0 { 1 } else { n };
    let mut log = 0;
    while v > 1 {
        v = (v + 1) >> 1;
        log += 1;
    }
    log
}

#[inline]
pub const fn ilog2_floor(n: usize) -> usize {
    let mut v = if n == 0 { 1 } else { n };
    let mut log = 0;
    while v > 1 {
        v >>= 1;
        log += 1;
    }
    log
}

/// 尺寸桶名字（统计输出）
fn bucket_name(size: usize) -> &'static str {
    match size {
        8 => "kmalloc-8",
        16 => "kmalloc-16",
        32 => "kmalloc-32",
        64 => "kmalloc-64",
        128 => "kmalloc-128",
        256 => "kmalloc-256",
        512 => "kmalloc-512",
        1024 => "kmalloc-1k",
        _ => "kmalloc-2k",
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    // 复用 slub 测试桩（super::super = heap 组，与内核 mm::heap 同构）
    use super::super::slub::FakeProvider;

    #[test]
    fn test_bucket_select() {
        assert_eq!(Kmalloc::bucket_for(0), Some(0));
        assert_eq!(Kmalloc::bucket_for(7), Some(0));
        assert_eq!(Kmalloc::bucket_for(9), Some(1));
        assert_eq!(Kmalloc::bucket_for(2048), Some(8));
        assert_eq!(Kmalloc::bucket_for(2049), None);
        assert_eq!(Kmalloc::bucket_for(8192), None);
    }

    #[test]
    fn test_basic_kmalloc_kfree() {
        let mut provider = FakeProvider::new(8);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        let a = k.kmalloc(32, &mut provider);
        let b = k.kmalloc(100, &mut provider);
        assert!(!a.is_null());
        assert!(!b.is_null());
        // 桶语义：100 字节落在 128 桶
        assert_eq!(k.direct_used(), 0);
        assert!(k.kfree(a, &mut provider));
        assert!(k.kfree(b, &mut provider));
        assert_eq!(k.total_objects(), 0);
    }

    #[test]
    fn test_alignment_buckets() {
        let mut provider = FakeProvider::new(8);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        for &size in &[8usize, 16, 24, 64, 128, 1024] {
            let p = k.kmalloc(size, &mut provider);
            assert!(!p.is_null(), "size {size} 分配失败");
            assert_eq!(p as usize % 8, 0, "size {size} 至少 8 字节对齐");
            assert!(k.kfree(p, &mut provider));
        }
    }

    #[test]
    fn test_direct_large_and_overalign() {
        let mut provider = FakeProvider::new(64);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();

        // 大对象：2049 字节以上直接分配，返回地址按 4096/2^order 对齐
        let big = k.kmalloc(5000, &mut provider);
        assert!(!big.is_null());
        assert!(big as usize % KERNEL_PAGE_SIZE == 0, "高阶页块按页对齐");
        assert!(k.kfree(big, &mut provider));

        // 高对齐（16 字节）：也会走直接路径保证对齐
        let al = k.kmalloc_aligned(24, 16, &mut provider);
        assert!(!al.is_null());
        assert_eq!(al as usize % 16, 0);
        assert!(k.kfree(al, &mut provider));

        // 超大对齐（4096）
        let huge_align = k.kmalloc_aligned(100, 4096, &mut provider);
        assert!(!huge_align.is_null());
        assert_eq!(huge_align as usize % 4096, 0);
        assert!(k.kfree(huge_align, &mut provider));
        assert_eq!(k.direct_used(), 0);
    }

    #[test]
    fn test_interleave_2048_rounds() {
        // 单对象桶（2048）的高频整页往返
        let mut provider = FakeProvider::new(16);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        for _ in 0..300 {
            let a = k.kmalloc(2048, &mut provider);
            let b = k.kmalloc(2048, &mut provider);
            assert!(!a.is_null() && !b.is_null());
            assert!(k.kfree(a, &mut provider));
            assert!(k.kfree(b, &mut provider));
        }
        assert_eq!(k.total_objects(), 0);
        assert_eq!(provider.free_count(), provider.capacity());
    }

    #[test]
    fn test_interleave_mixed_sizes() {
        // 多种尺寸 + 2048 单对象桶交替，制造频繁整页往返与页复用
        let mut provider = FakeProvider::new(8);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        for i in 0..2000u32 {
            let p = k.kmalloc((2000 + (i % 200)) as usize % 3000 + 100, &mut provider);
            assert!(!p.is_null());
            assert!(k.kfree(p, &mut provider));
        }
        assert_eq!(k.total_objects(), 0);
        assert_eq!(provider.free_count(), provider.capacity());
    }

    #[test]
    fn test_interleave_with_direct() {
        // 桶（尤其 2048 单对象桶）与直接分配（>2KB / 高对齐）交替，
        // 制造「直接页块与 slab 页在提供者池中交错往返」
        let mut provider = FakeProvider::new(16);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        let mut live: Vec<*mut u8> = Vec::new();
        for i in 0..50_000u32 {
            let size = match i % 5 {
                0 => 2048,
                1 => 3000,
                2 => 100,
                3 => 8192,
                _ => 24,
            };
            let align = if i % 3 == 0 { 16 } else { 8 };
            if i % 4 == 0 && !live.is_empty() {
                let p = live.pop().unwrap();
                assert!(k.kfree(p, &mut provider), "free({}) failed", i);
            } else {
                let p = k.kmalloc_aligned(size, align, &mut provider);
                if !p.is_null() {
                    live.push(p);
                }
            }
        }
        for p in live {
            assert!(k.kfree(p, &mut provider));
        }
        assert_eq!(k.total_objects(), 0);
        assert_eq!(provider.free_count(), provider.capacity());
    }

    #[test]
    fn test_kfree_bad_ptr() {
        let mut provider = FakeProvider::new(8);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();
        assert!(!k.kfree(core::ptr::null_mut(), &mut provider));
    }

    #[test]
    fn test_random_stress_no_leak() {
        // 大量随机分配/释放：结束时所有对象归还、提供者页数复原
        // （= 无泄漏；FakeProvider 归还页会回到池中）
        let mut provider = FakeProvider::new(64);
        let mut k = Kmalloc::uninit();
        k.ensure_buckets();

        let mut live: std::collections::VecDeque<*mut u8> = std::collections::VecDeque::new();
        let mut rng = 0x1234_5678_9ABC_DEF0u64;
        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        for _ in 0..50_000 {
            if (rand() & 1) == 0 || live.is_empty() {
                let size = (rand() as usize) % 9000 + 1;
                let p = if (rand() & 1) == 0 {
                    k.kmalloc(size, &mut provider)
                } else {
                    k.kmalloc_aligned(size, 1usize << ((rand() as usize) % 6), &mut provider)
                };
                if !p.is_null() {
                    live.push_back(p);
                }
            } else {
                let idx = (rand() as usize) % live.len();
                let p = live.remove(idx).unwrap();
                assert!(k.kfree(p, &mut provider), "释放失败");
            }
        }
        for p in live {
            assert!(k.kfree(p, &mut provider));
        }
        assert_eq!(k.total_objects(), 0, "压力测试后不应有残留对象");
        // FakeProvider 的页全部回到空闲池 = 无泄漏
        assert_eq!(provider.free_count(), provider.capacity());
    }
}