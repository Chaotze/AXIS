// ============================================================
// 内核堆（Heap / GlobalAlloc 实现）
// ============================================================
// 把 SLUB + kmalloc 装配成 Rust 全局分配器：
// - 提供 #[global_allocator]，让 Box / Vec / String 等标准容器
//   可以直接在内核中使用
// - 页来源是 PMM（物理内存直接映射），故“堆对象地址”就是直接映射
//   虚拟地址，无需为堆另建页表映射
//
// 与其他模块的关系（依赖方向）：
//   heap  ->  kmalloc  ->  CacheSet/SlabCache/Slub  ->  PageProvider
//                                                          ↑
//   PmmState 实现 PageProvider（pmm 胶水层）---------------┘
//
// 设计要点（为什么这么做）：
// 1. 所有分配路径共用同一把 PMM 锁 + 堆锁、且锁序恒为 PMM→KHEAP，
//    全内核唯一，杜绝死锁（唯一反向路径在 vmm，见 vmm 说明）。
// 2. 分配器内部零堆依赖（slab 页头/空闲链都在页内），所以 GlobalAlloc
//    的实现在堆“启用”后即可安全工作，且不会递归分配。
// 3. 未初始化时分配返回 null，符合 GlobalAlloc 契约（上层容器会 abort
//    或由调用方检查）——这保证了“堆未就绪就使用 Vec”会被显式暴露。

use core::alloc::{GlobalAlloc, Layout};

pub mod kmalloc;
pub mod slab_cache;
pub mod slub;

use crate::mm::pmm;
use crate::sync::Spinlock;

use self::kmalloc::Kmalloc;
use self::slub::PageProvider;

/// 全局堆锁（kmalloc 状态）
static KHEAP: Spinlock<Kmalloc> = Spinlock::new(Kmalloc::uninit());

/// 内核全局分配器标记类型
///
/// 采用「零字段结构体 + 全局 static」：
/// - 全局分配器必须是零尺寸、零运行时构造的类型
/// - [global_allocator] 属性要求它实现 [core::alloc::GlobalAlloc]
pub struct KernelAllocator;

/// 注册为 Rust 全局分配器（Box/Vec/String 的后端）
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

/// 同时取得 PMM 与堆锁，把 PMM 作为页提供者交给堆
///
/// 为什么需要两把锁：PMM 提供物理页，堆把页切成对象，二者状态
/// 必须互斥修改；锁序固定 PMM → KHEAP，任何调用路径不得倒序。
#[inline]
fn with_provider<R>(f: impl FnOnce(&mut Kmalloc, &mut dyn PageProvider) -> R) -> Option<R> {
    pmm::with_pmm(|pm| f(&mut KHEAP.lock(), pm as &mut dyn PageProvider))
}

/// 内核堆初始化：在 pmm::init 之后、任何 alloc 容器使用之前调用
pub fn init() {
    let mut k = KHEAP.lock();
    k.ensure_buckets();
    println!("[HEAP] kmalloc size buckets ready (8B ~ 2KB)");
}

/// 堆是否已就绪
pub fn is_ready() -> bool {
    KHEAP.lock().is_ready()
}

// ---------------------------------------------------------------------
// 内核堆 API（对外暴露的 kmalloc / kfree）
// ---------------------------------------------------------------------

/// 分配 size 字节（align 对数对齐），失败返回 null
pub fn kmalloc(size: usize, align: usize) -> *mut u8 {
    with_provider(|k, p| k.kmalloc_aligned(size, align, p)).unwrap_or(core::ptr::null_mut())
}

/// 分配并清零
pub fn kmalloc_zeroed(size: usize) -> *mut u8 {
    with_provider(|k, p| k.kmalloc_zeroed(size, p)).unwrap_or(core::ptr::null_mut())
}

/// 释放堆对象（自动识别尺寸桶/直接页块路径）
pub fn kfree(ptr: *mut u8) {
    // 释放失败（非法/重复释放）在 debug 下记录，release 下忽略
    let ok = with_provider(|k, p| k.kfree(ptr, p)).unwrap_or(false);
    debug_assert!(ok, "kfree: 非法指针 0x{:x}", ptr as usize);
}

// ---------------------------------------------------------------------
// GlobalAlloc 实现
// ---------------------------------------------------------------------

unsafe impl GlobalAlloc for KernelAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        kmalloc(layout.size(), layout.align())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        kfree(ptr);
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // 覆写默认实现：零内存页可能已被复用，按 layout 大小清零最稳
        with_provider(|k, p| {
            k.kmalloc_zeroed_with_align(layout.size(), layout.align(), p)
        })
        .unwrap_or(core::ptr::null_mut())
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SLUB/Slab 缓存不支持原地扩容；走「分配-拷贝-释放」通用路径
        if new_size == 0 {
            kfree(ptr);
            return core::ptr::null_mut();
        }
        let new_ptr = kmalloc(new_size, layout.align());
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }
        let copy_len = layout.size().min(new_size);
        if !ptr.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_len) };
            kfree(ptr);
        }
        new_ptr
    }
}

// ---------------------------------------------------------------------
// 统计与监控
// ---------------------------------------------------------------------

/// 堆统计快照
#[derive(Debug, Clone, Copy, Default)]
pub struct HeapStats {
    /// 已分配对象总数（桶 + 直接分配）
    pub objects: usize,
    /// 当前持有的 slab 页数
    pub slab_pages: usize,
    /// 各桶占用对象数（按桶序）
    pub bucket_objects: [usize; self::kmalloc::KMALLOC_BUCKET_COUNT],
    /// 直接分配在册数
    pub direct_objects: usize,
    /// 估算的已用字节数（对象数 × 步长之和）
    pub bytes_in_use: usize,
}

/// 获取堆统计快照
pub fn stats() -> HeapStats {
    let k = KHEAP.lock();
    let mut s = HeapStats::default();
    if !k.is_ready() {
        return s;
    }
    s.direct_objects = k.direct_used();
    s.objects = k.total_objects();
    for (id, cache) in k.cache_set().iter() {
        s.slab_pages += cache.slab_pages();
        s.bytes_in_use += cache.objs_allocated() * cache.stride();
        if id < self::kmalloc::KMALLOC_BUCKET_COUNT {
            s.bucket_objects[id] = cache.objs_allocated();
        }
    }
    s
}
