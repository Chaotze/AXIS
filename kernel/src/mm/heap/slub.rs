// ============================================================
// SLUB 分配器核心
// ============================================================
// 面向「同尺寸小对象」的高性能分配器（Linux SLUB 的精简实现）：
// - 从页提供者（PageProvider）取物理页作为 slab
// - slab 页头部存放页内对象空闲链表与链表指针
// - 对象按固定步长（stride）均匀铺满整页
//
// 设计要点（为什么这么做）：
// 1. 【共置元数据】slab 的页头、对象空闲链表全部「嵌在页内」：
//    - 分配器于是完全不依赖堆（分配器不能通过分配自身来初始化）
//    - kfree 只需拿 ptr 的页基址读魔数，就能 O(1) 判定并定位缓存，
//      无需任何「页 → 缓存」的反向映射表
// 2. 【缓存不拥有页】页来自调用方传入的 PageProvider：
//    - 内核中提供者是 PMM（直接映射地址）；宿主编译时可注入桩，
//      便于把整套逻辑拿到宿主环境做单元测试
//    - 结构体与 trait 分离，职责清晰，符合「高组件复用率」要求
// 3. 【空闲页立即归还】一旦 slab 页上所有对象都被释放，立刻把整页
//    还给 PageProvider：
//    - 这是“长时间运行无内存泄漏”的关键（空页滞留是隐性泄漏）
//    - 代价是频繁分配/释放可能多一次取还操作——对里程碑足够合理
// 4. 【步长幂次对齐】stride 取 2 的幂（≥8，容纳空闲链指针），
//    保证任何对象地址满足其尺寸对应的对齐要求。

/// slab 页头魔数：用于快速识别「某地址所在的页是否 slab 页」
pub const SLAB_MAGIC: u32 = 0x534C_4142; // ASCII "SLAB"

/// 空闲链结束标记
const FREE_END: u32 = u32::MAX;

/// 释放对象时填充的毒药字节（调试用，可检测 use-after-free）
pub const POISON_BYTE: u8 = 0xAB;

/// 页提供者：向 SLUB 提供/回收「页对齐的内存页」
///
/// 为什么返回 usize（页基址）而非页号：
/// - 页基址在物理直接映射环境（PMM）与宿主桩环境下语义一致，
///   分配器不需要关心“地址换算”的架构细节
/// - 页基址满足页对齐即可（内核 = 直接映射地址；宿主测试 = 对齐堆块）
pub trait PageProvider: Send {
    /// 分配一页，返回页基址；失败返回 None
    ///
    /// 为什么要求 Send：提供者可能被放进全局自旋锁（如 VMM 的
    /// 交换存储），dyn 对象需要具 Send 才能伴锁存储。
    fn alloc_page(&mut self) -> Option<usize> {
        self.alloc_pages(0)
    }
    /// 归还一页
    fn free_page(&mut self, page_addr: usize) {
        self.free_pages(0, page_addr)
    }
    /// 分配 2^order 个「连续」页（返回块基址，按 2^order*page 对齐）
    ///
    /// 为什么需要多页分配：kmalloc 的大对象路径需要连续内存，
    /// 若用多次单页拼接则无法满足连续性；伙伴系统恰好提供
    /// order 块的连续性，故将接口直接对齐到「阶」语义。
    fn alloc_pages(&mut self, order: usize) -> Option<usize>;
    /// 归还 2^order 个连续页
    fn free_pages(&mut self, order: usize, page_addr: usize);
}

/// slab 页头：常驻于每一页的开头（共置元数据）
#[repr(C)]
struct SlabPageHeader {
    /// 魔数（SLAB_MAGIC）
    magic: u32,
    /// 所属缓存 id（kfree 通过它找到缓存）
    cache_id: u32,
    /// 对象步长
    stride: u32,
    /// 本页对象总数
    count: u32,
    /// 剩余空闲对象数
    /// （为什么是 u32 而不是 usize：最大对象数 <= 页大小/步长 <= 512，u32 足够）
    free: u32,
    /// 空闲链表头（对象下标）
    next_free: u32,
    /// 双向链表：同缓存 slab 链表前驱（页基址，0 = 无）
    prev_slab: usize,
    /// 双向链表：同缓存 slab 链表后继
    next_slab: usize,
    /// 是否启用毒药（调试用）
    poison: u8,
    /// 填充字节（保证页头大小为 8 的倍数，便于步长对齐取整）
    _pad: [u8; 3],
}

impl SlabPageHeader {
    /// 页头大小（字节）
    const SIZE: usize = core::mem::size_of::<Self>();

    /// 读取某一页的页头（页已按 4096 对齐）
    #[inline]
    unsafe fn at(page: usize) -> *mut Self {
        debug_assert_eq!(page & 0xFFF, 0, "slab 页必须页对齐");
        page as *mut Self
    }
}

/// 一个对象缓存（kmem_cache）
pub struct SlabCache {
    /// 缓存 id（kfree 反查使用）
    id: u32,
    /// 缓存名（统计/调试输出）
    name: &'static str,
    /// 对象步长（对齐后的实际占用字节，幂次对齐）
    stride: usize,
    /// 请求对齐（字节）
    align: usize,
    /// 页大小（字节，一般 4096）
    page_size: usize,
    /// 页内对象区起始偏移（= align_up(页头, stride)）
    objects_offset: usize,
    /// 每页对象数
    objs_per_slab: usize,
    /// 部分空闲 slab 链表头（页基址；0 = 空）
    partial: usize,
    /// 全满 slab 链表头
    full: usize,
    /// 当前持有的 slab 页数
    slabs: usize,
    /// 当前已分配对象数
    objs_allocated: usize,
    /// 是否启用毒药
    poison: bool,
    /// 参数是否已初始化
    valid: bool,
    /// 统计
    pub total_allocs: u64,
    pub total_frees: u64,
    pub page_allocs: u64,
}

impl SlabCache {
    /// 未初始化占位（必须经 [init] 后才能使用）
    pub const fn uninit() -> Self {
        Self {
            id: 0,
            name: "",
            stride: 0,
            align: 8,
            page_size: 0,
            objects_offset: 0,
            objs_per_slab: 0,
            partial: 0,
            full: 0,
            slabs: 0,
            objs_allocated: 0,
            poison: false,
            valid: false,
            total_allocs: 0,
            total_frees: 0,
            page_allocs: 0,
        }
    }

    /// 初始化缓存参数（不取页）
    ///
    /// - request_size：调用方请求的对象大小（下取整到 8 的倍数）
    /// - align：对象对齐要求（2 的幂）
    pub fn init(
        &mut self,
        id: u32,
        name: &'static str,
        request_size: usize,
        align: usize,
        page_size: usize,
    ) {
        // 步长至少 8 字节（空闲链表指针占 4 字节 + 冗余），并向上取
        // 到 align 的倍数；由于 align 是 2 的幂，stride 也是 2 的幂
        let align = align.max(8);
        let stride = align_up(request_size.max(8), align);
        let objects_offset = align_up(SlabPageHeader::SIZE, stride);
        let objs_per_slab = (page_size - objects_offset) / stride;

        self.id = id;
        self.name = name;
        self.stride = stride;
        self.align = align;
        self.page_size = page_size;
        self.objects_offset = objects_offset;
        self.objs_per_slab = objs_per_slab;
        self.partial = 0;
        self.full = 0;
        self.slabs = 0;
        self.objs_allocated = 0;
        self.poison = cfg!(debug_assertions);
        self.valid = true;
    }

    /// 缓存是否就绪
    #[inline]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }

    /// 缓存名
    #[inline]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// 对象步长
    #[inline]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// 已分配对象数
    #[inline]
    pub const fn objs_allocated(&self) -> usize {
        self.objs_allocated
    }

    /// 持有的 slab 页数
    #[inline]
    pub const fn slab_pages(&self) -> usize {
        self.slabs
    }

    /// 初始化一个新 slab 页：写页头、串起对象空闲链表
    ///
    /// 空闲链表结构：对象 i 的首 4 字节存「下一个空闲对象下标」，
    /// 最后一个对象存 FREE_END。这样链表本身不占用额外内存。
    unsafe fn init_slab(&mut self, page: usize) {
        let h = unsafe { &mut *SlabPageHeader::at(page) };
        h.magic = SLAB_MAGIC;
        h.cache_id = self.id;
        h.stride = self.stride as u32;
        h.count = self.objs_per_slab as u32;
        h.free = self.objs_per_slab as u32;
        h.next_free = 0;
        h.prev_slab = 0;
        h.next_slab = 0;
        h.poison = self.poison as u8;

        for i in 0..self.objs_per_slab {
            let obj = page + self.objects_offset + i * self.stride;
            let next = if i + 1 < self.objs_per_slab {
                (i + 1) as u32
            } else {
                FREE_END
            };
            unsafe { core::ptr::write(obj as *mut u32, next) };
        }
        self.page_allocs += 1;
    }

    /// 对象地址 ↔ 下标换算
    #[inline]
    fn slot_of(&self, page: usize, obj_addr: usize) -> Option<usize> {
        let off = obj_addr.checked_sub(page + self.objects_offset)?;
        if off % self.stride != 0 {
            return None;
        }
        let i = off / self.stride;
        if i >= self.objs_per_slab {
            return None;
        }
        Some(i)
    }

    /// 从链表中摘除一个 slab 页（prev/next 双向链维护）
    ///
    /// 为什么是模块级函数而非方法：方法会同时借用 `self` 与作为参数
    /// 的 `&mut self.partial`，产生双可变借用；把链表本身作为显式参数
    /// 传入则没有该问题。
    #[inline]
    unsafe fn slab_unlink(list: &mut usize, page: usize) {
        let h = unsafe { &mut *SlabPageHeader::at(page) };
        let (prev, next) = (h.prev_slab, h.next_slab);
        if prev == 0 {
            *list = next;
        } else {
            (unsafe { &mut *SlabPageHeader::at(prev) }).next_slab = next;
        }
        if next != 0 {
            (unsafe { &mut *SlabPageHeader::at(next) }).prev_slab = prev;
        }
        h.prev_slab = 0;
        h.next_slab = 0;
    }

    /// 把一个 slab 页推入链表头
    #[inline]
    unsafe fn slab_push(list: &mut usize, page: usize) {
        let h = unsafe { &mut *SlabPageHeader::at(page) };
        h.prev_slab = 0;
        h.next_slab = *list;
        if *list != 0 {
            (unsafe { &mut *SlabPageHeader::at(*list) }).prev_slab = page;
        }
        *list = page;
    }

    /// 取一个对象的地址（空闲链表弹出）
    ///
    /// 为什么让调用方传 provider：缓存不保存提供者引用，避免多个
    /// 缓存争用同一提供者的借用冲突，也能让「取页」时机完全由
    /// 调用方控制（加锁粒度最细）。
    pub fn alloc(&mut self, provider: &mut dyn PageProvider) -> *mut u8 {
        debug_assert!(self.valid);
        if self.objs_per_slab == 0 {
            return core::ptr::null_mut();
        }

        // 没有部分空闲页则取新页
        if self.partial == 0 {
            let Some(page) = provider.alloc_page() else {
                return core::ptr::null_mut();
            };
            unsafe { self.init_slab(page) };
            self.slabs += 1;
            unsafe { Self::slab_push(&mut self.partial, page) };
        }

        let page = self.partial;
        let header = unsafe { &mut *SlabPageHeader::at(page) };
        debug_assert_eq!(header.magic, SLAB_MAGIC);
        debug_assert!(header.free > 0, "partial 链上的页必然还有空闲对象");

        // 弹出空闲链头对象
        let slot = header.next_free as usize;
        let obj = page + self.objects_offset + slot * self.stride;
        let next = unsafe { core::ptr::read(obj as *const u32) };
        header.next_free = next;
        header.free -= 1;

        // 用尽则挪到 full 链
        if header.free == 0 {
            unsafe { Self::slab_unlink(&mut self.partial, page) };
            unsafe { Self::slab_push(&mut self.full, page) };
        }

        self.objs_allocated += 1;
        self.total_allocs += 1;
        obj as *mut u8
    }

    /// 归还对象（若整页空闲则把页归还给提供者）
    ///
    /// 返回 false 表示 ptr 不属于本缓存（调用方应视为错误）。
    pub fn free(&mut self, provider: &mut dyn PageProvider, ptr: *mut u8) -> bool {
        debug_assert!(self.valid);
        if ptr.is_null() {
            return false;
        }
        let addr = ptr as usize;
        let page = addr & !(self.page_size - 1);

        let header = unsafe { &mut *SlabPageHeader::at(page) };
        if header.magic != SLAB_MAGIC || header.cache_id != self.id {
            return false;
        }
        let Some(slot) = self.slot_of(page, addr) else {
            return false;
        };

        // 双释放/错误释放检测：目标对象不得已经在空闲链表上。
        // 通过“沿空闲链从表头走到表尾”判定（链表通常很短）；
        // 与毒化标记相比，链表包含性判定没有误报（对象内容里
        // 出现 0xAB 不该被当成“已经释放”）。
        if self.poison {
            let mut cursor = header.next_free;
            while cursor != FREE_END {
                if cursor as usize == slot {
                    return false;
                }
                let o = page + self.objects_offset + cursor as usize * self.stride;
                cursor = unsafe { core::ptr::read(o as *const u32) };
            }
        }

        // 先毒化再写链指针（毒化会覆盖对象前 4 字节）
        if self.poison {
            unsafe { core::ptr::write_bytes(ptr, POISON_BYTE, self.stride) };
        }

        // 推回空闲链
        let was_full = header.free == 0;
        unsafe { core::ptr::write(addr as *mut u32, header.next_free) };
        header.next_free = slot as u32;
        header.free += 1;

        let emptied = header.free == header.count;
        if emptied {
            // 整页空闲 → 归还（防泄漏的关键路径）
            //
            // 必须从「页面实际所在的链表」摘除：was_full 时页面在
            // full 链上，若误从 partial 链摘除会写入陈旧链接，使
            // partial 头指向一条已转移到 full 的页 —— 这是单对象
            // slab（如 kmalloc-2k）曾出现的链表损坏根因
            if was_full {
                unsafe { Self::slab_unlink(&mut self.full, page) };
            } else {
                unsafe { Self::slab_unlink(&mut self.partial, page) };
            }
            // 归还前抹掉页头：避免残留 SLAB 魔数误导 kfree 路由
            unsafe { core::ptr::write_bytes(page as *mut u8, 0, SlabPageHeader::SIZE) };
            provider.free_page(page);
            self.slabs -= 1;
        } else if was_full {
            // 全满 → 部分空闲
            unsafe { Self::slab_unlink(&mut self.full, page) };
            unsafe { Self::slab_push(&mut self.partial, page) };
        }

        self.objs_allocated = self.objs_allocated.saturating_sub(1);
        self.total_frees += 1;
        true
    }

    /// 页基址（白盒：供测试与监控确认对象所在页）
    #[inline]
    pub fn page_of(&self, ptr: *const u8) -> usize {
        (ptr as usize) & !(self.page_size - 1)
    }
}

/// 向上对齐到 align 的整数倍
///
/// 为什么 align 不必是 2 的幂：kmalloc 尺寸桶是 2 的幂，但用户
/// 自定义缓存（slab_cache）可能用任意对象尺寸；对象地址的对齐
/// 保证来自「stride 是对齐要求 align 的倍数」，与 stride 是否为
/// 2 的幂无关。
#[inline]
pub fn align_up(x: usize, align: usize) -> usize {
    debug_assert!(align != 0);
    let rem = x % align;
    if rem == 0 { x } else { x + (align - rem) }
}

// ---------------------------------------------------------------------
// 宿主测试桩（仅 cfg(test) 构建存在，不进入内核）
// ---------------------------------------------------------------------
// FakeProvider 供 buddy/slub/slab_cache/kmalloc/swap 的宿主单元测试
// 复用：维护一块连续页对齐缓冲区，支持单页与连续多页（order）分配。
#[cfg(test)]
use alloc::vec;
#[cfg(test)]
use alloc::vec::Vec;

/// 宿主测试用的页提供者：从一块连续对齐大缓冲区里维护空闲池
#[cfg(test)]
pub struct FakeProvider {
    /// 整块缓冲区基址（4096 对齐）
    base: usize,
    /// 容量（页数）
    cap: usize,
    /// 每页占用标记
    used: Vec<bool>,
    /// 空闲页数
    free: usize,
}

#[cfg(test)]
impl FakeProvider {
    pub fn new(capacity_pages: usize) -> Self {
        let layout =
            std::alloc::Layout::from_size_align(4096 * capacity_pages, 4096).unwrap();
        let ptr = unsafe { std::alloc::alloc(layout) };
        assert!(!ptr.is_null(), "测试缓冲区分配失败");
        Self {
            base: ptr as usize,
            cap: capacity_pages,
            used: vec![false; capacity_pages],
            free: capacity_pages,
        }
    }

    /// 当前空闲页数
    pub fn free_count(&self) -> usize {
        self.free
    }

    /// 容量（页数）
    pub fn capacity(&self) -> usize {
        self.cap
    }

    fn page_addr(&self, idx: usize) -> usize {
        self.base + idx * 4096
    }
}

#[cfg(test)]
impl PageProvider for FakeProvider {
    fn alloc_pages(&mut self, order: usize) -> Option<usize> {
        let n = 1usize << order;
        // 扫描连续空闲且起始页对齐到 2^order 的块
        for start in (0..self.cap).step_by(n) {
            if start + n > self.cap {
                break;
            }
            if self.used[start..start + n].iter().all(|&u| !u) {
                for b in &mut self.used[start..start + n] {
                    *b = true;
                }
                self.free -= n;
                return Some(self.page_addr(start));
            }
        }
        None
    }

    fn free_pages(&mut self, order: usize, page_addr: usize) {
        let n = 1usize << order;
        let start = (page_addr - self.base) / 4096;
        debug_assert!(start % n == 0, "释放的块必须与分配时的对齐一致");
        for b in &mut self.used[start..start + n] {
            debug_assert!(*b, "释放未占用页");
            *b = false;
        }
        // 模拟真实分配器“页面被复用前不保留旧内容”：清零整块，
        // 彻底消除残留 SLAB 头对 kfree 魔数判定的干扰
        unsafe { core::ptr::write_bytes(page_addr as *mut u8, 0, n * 4096) };
        self.free += n;
    }
}

// ---------- 宿主单元测试（通过 mmtest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use std::prelude::v1::*;

    fn page_size() -> usize {
        4096
    }

    #[test]
    fn test_basic_alloc_free() {
        let mut provider = FakeProvider::new(8);
        let mut cache = SlabCache::uninit();
        cache.init(0, "test-64", 64, 8, page_size());

        let a = cache.alloc(&mut provider);
        assert!(!a.is_null());
        let b = cache.alloc(&mut provider);
        assert!(!b.is_null());
        assert_ne!(a, b);
        assert_eq!(cache.objs_allocated(), 2);

        assert!(cache.free(&mut provider, a));
        assert!(cache.free(&mut provider, b));
        assert_eq!(cache.objs_allocated(), 0);
        // 两对象同页且整页空闲 → 页应归还
        assert_eq!(cache.slab_pages(), 0);
        assert_eq!(provider.free_count(), 8);
    }

    #[test]
    fn test_object_alignment() {
        let mut provider = FakeProvider::new(4);
        let mut cache = SlabCache::uninit();
        cache.init(1, "align-16", 12, 16, page_size());
        assert_eq!(cache.stride(), 16);

        let ptrs: Vec<*mut u8> = (0..20).map(|_| cache.alloc(&mut provider)).collect();
        for p in ptrs {
            assert_eq!(p as usize % 16, 0, "对象必须 16 字节对齐");
        }
    }

    #[test]
    fn test_poison_detects_use_after_free() {
        let mut provider = FakeProvider::new(4);
        let mut cache = SlabCache::uninit();
        cache.init(2, "poison", 32, 8, page_size());
        // 分配两个对象，只释放 a：页面保持“部分空闲”不被回收，
        // a 上应留下毒药痕迹
        let a = cache.alloc(&mut provider);
        let _keep = cache.alloc(&mut provider);
        assert!(cache.free(&mut provider, a));
        // 毒药：对象前 4 字节是空闲链表后继指针（非毒药），
        // 其余字节应全部为 0xAB
        let bytes = unsafe { core::slice::from_raw_parts(a as *const u8, 32) };
        assert!(bytes[4..].iter().all(|&b| b == POISON_BYTE), "释放后应有毒药痕迹");
        // 收尾：释放保持页存活的对象
        assert!(cache.free(&mut provider, _keep));
    }

    #[test]
    fn test_exhaust_provider() {
        let mut provider = FakeProvider::new(1);
        let mut cache = SlabCache::uninit();
        cache.init(3, "small", 8, 8, page_size());
        // 一页可容纳 (4096-48)/8 ≈ 506 个对象
        let mut got = 0;
        while !cache.alloc(&mut provider).is_null() {
            got += 1;
        }
        assert!(got >= 500, "单页应容纳至少 500 个 8 字节对象，实际 {got}");
        assert_eq!(cache.slab_pages(), 1);
        // 没有更多页 → 分配失败返回空
        assert!(cache.alloc(&mut provider).is_null());
    }

    #[test]
    fn test_free_wrong_ptr_rejected() {
        let mut provider = FakeProvider::new(4);
        let mut cache = SlabCache::uninit();
        cache.init(4, "reject", 16, 8, page_size());
        // 未对齐的指针应被拒绝（同一页，但偏移不匹配步长）
        let a = cache.alloc(&mut provider);
        assert!(!cache.free(&mut provider, unsafe { a.add(1) }));
        assert!(cache.free(&mut provider, a));
        // double free：页面已归还并清零，魔数消失，应被拒绝
        // （fake 提供者归还时清零页面，等价于真实内核里页面被复用）
        assert!(!cache.free(&mut provider, a), "double free 应拒绝");
    }

    #[test]
    fn test_roundtrip_stress() {
        // 随机分配/释放压力：最终对象数归零、页全部归还
        let mut provider = FakeProvider::new(16);
        let mut cache = SlabCache::uninit();
        cache.init(5, "stress", 32, 8, page_size());

        let mut live: Vec<*mut u8> = vec![];
        let mut rng = 0xABCD_EF01u64;
        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        for _ in 0..10_000 {
            if (rand() & 1) == 0 || live.is_empty() {
                if let Some(p) = std::ptr::NonNull::new(cache.alloc(&mut provider)) {
                    live.push(p.as_ptr());
                }
            } else {
                let i = (rand() as usize) % live.len();
                let p = live.swap_remove(i);
                assert!(cache.free(&mut provider, p));
            }
        }
        for p in live {
            assert!(cache.free(&mut provider, p));
        }
        assert_eq!(cache.objs_allocated(), 0);
        assert_eq!(cache.slab_pages(), 0, "压力测试后不应残留 slab 页（无泄漏）");
    }
}