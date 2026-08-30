// ============================================================
// 物理内存管理（PMM）接口层
// ============================================================
// 把「区域（Zone）→ 伙伴系统 → 页」的纯逻辑装配成内核可直接使用的
// 物理页分配服务：
// - 初始化：从引导提供的可用内存区构建 DMA / Normal 区域；
//   伙伴系统元数据与页帧元数据从被管理内存的顶部划分（Linux
//   bootmem 的做法），使 PMM 自身的初始化完全不依赖堆
// - 分配接口：alloc/free 单页与高阶页块，带 gfp 标志与所有者标签
// - 对外契约：实现 arch/paging.rs 的 FrameAllocator（供页表映射器
//   使用）与 heap/slub.rs 的 PageProvider（供堆分配器取页）
//
// 模块结构说明（与 dev/arch.md 对齐）：
//   pmm.rs      —— 本文件：状态、初始化、公开接口（胶水层）
//   pmm/buddy.rs、pmm/zone.rs、pmm/watermark.rs、pmm/frame.rs、
//   pmm/numa.rs —— 纯算法/数据结构（无依赖，可宿主单测）

pub mod buddy;
pub mod frame;
pub mod numa;
pub mod watermark;
pub mod zone;

use crate::arch::x86_64::memory::{Frame, PhysAddr};
use crate::arch::x86_64::paging::FrameAllocator;
use crate::config::PAGE_SIZE;
use crate::prelude::{KernelError, KernelResult};
use crate::sync::Spinlock;

use self::buddy::order_pages;
use self::frame::{FrameMeta, FrameOwner, FrameState};
use self::numa::NumaTopology;

// 重新导出（zone 的类型是本模块对外接口的一部分）
pub use self::zone::{GfpFlags, Zone, ZoneType};

use super::addr::{align_up, MemoryRegion, MemoryRegionType};

/// 全局 PMM 状态（初始化后为 Some）
///
/// 用 Spinlock<Option<PmmState>> 而非 static mut：
/// - 与项目既有并发惯例一致（print 使用的 Spinlock 单例模式）
/// - 初始化前访问会得到 None，由调用方显式处理，语义更安全
static PMM: Spinlock<Option<PmmState>> = Spinlock::new(None);

/// 区域数量上限（DMA / Normal 各自可叠加多个返回值，含 NUMA 扩展）
pub const MAX_ZONES: usize = 8;

/// DMA 区域上限物理地址（16MB）：老式 ISA 设备只能访问该范围以下的
/// 内存做 DMA，低于此地址的页专门留给这类设备。
pub const DMA_ZONE_LIMIT: u64 = 16 * 1024 * 1024;

/// 物理内存管理状态
pub struct PmmState {
    /// 区域数组（只使用前 zone_count 个）
    zones: [Zone; MAX_ZONES],
    /// 区域数量
    zone_count: usize,
    /// 每个区域的首个页帧元数据下标（累计索引，用于 pfn→index）
    zone_base: [usize; MAX_ZONES],
    /// 页帧元数据数组（来源：物理内存顶部划分的字节区）
    frames: Option<&'static mut [FrameMeta]>,
    /// 页帧元数据对应的首个物理页号
    frames_base_pfn: usize,
    /// NUMA 拓扑（当前为单节点 UMA）
    topology: NumaTopology,
    /// 页面大小（字节）
    page_size: usize,
}

impl PmmState {
    /// 未初始化占位
    pub const fn uninit() -> Self {
        Self {
            zones: [
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
                Zone::uninit(),
            ],
            zone_count: 0,
            zone_base: [0; MAX_ZONES],
            frames: None,
            frames_base_pfn: 0,
            topology: NumaTopology::single_node(&[]),
            page_size: PAGE_SIZE,
        }
    }

    /// 区域列表（只读）
    pub fn zones(&self) -> &[Zone] {
        &self.zones[..self.zone_count]
    }

    /// 可变区域列表
    pub fn zones_mut(&mut self) -> &mut [Zone] {
        &mut self.zones[..self.zone_count]
    }

    /// NUMA 拓扑
    #[inline]
    pub const fn topology(&self) -> &NumaTopology {
        &self.topology
    }

    /// 页帧元数据（监控用）
    #[inline]
    pub fn frames(&self) -> Option<&[FrameMeta]> {
        self.frames.as_deref()
    }

    /// 区域总数
    #[inline]
    pub const fn zone_count(&self) -> usize {
        self.zone_count
    }

    /// 总页数（全部可用区合计）
    pub fn total_pages(&self) -> usize {
        self.zones()[..].iter().map(|z| z.total_pages()).sum()
    }

    /// 空闲页数
    pub fn free_page_count(&self) -> usize {
        self.zones().iter().map(|z| z.free_pages()).sum()
    }

    /// 找到包含 pfn 的区域下标
    fn zone_of(&self, pfn: usize) -> Option<usize> {
        self.zones().iter().position(|z| {
            pfn >= z.start_pfn() && pfn + order_pages(0) <= z.end_pfn()
        })
    }

    /// 把 pfn 换算为页帧元数据下标
    fn frame_index(&self, zone: usize, pfn: usize) -> Option<usize> {
        let z = self.zones.get(zone)?;
        if pfn < z.start_pfn() || pfn >= z.end_pfn() {
            return None;
        }
        Some(self.zone_base[zone] + (pfn - z.start_pfn()))
    }

    /// 更新一批页帧的状态（order 块内的每一页）
    fn mark_frames(&mut self, zone: usize, pfn: usize, pages: usize, state: FrameState) {
        // 先单独计算下标，再借用帧数组——避免对 self 的双重可变借用
        let base = self.frame_index(zone, pfn);
        if let (Some(meta), Some(base)) = (self.frames.as_deref_mut(), base) {
            for i in 0..pages {
                if let Some(m) = meta.get_mut(base + i) {
                    m.state = state;
                    m.order = 0;
                }
            }
        }
    }

    /// 分配 2^order 页，返回物理地址
    ///
    /// 区域选择策略（与 Linux GFP 语义对齐）：
    /// - flags 带 DMA：优先 DMA 区，DMA 区耗尽时回退 Normal（宽松回退，
    ///   避免驱动因拿不到低地址页而假死；严格的 DMA-only 语义由调用方把关）
    /// - 否则优先 Normal，DMA 区作为兜底
    pub fn alloc_pages(
        &mut self,
        order: usize,
        flags: GfpFlags,
        owner: FrameOwner,
    ) -> Option<PhysAddr> {
        let want_dma = flags.contains(GfpFlags::DMA);
        // 两轮扫描：第一轮选期望类型，第二轮兜底另一类型
        for pass in 0..2 {
            for i in 0..self.zone_count {
                let ty = self.zones[i].ty();
                let matches = if want_dma {
                    (pass == 0 && ty == ZoneType::Dma) || (pass == 1 && ty != ZoneType::Dma)
                } else {
                    (pass == 0 && ty != ZoneType::Dma) || (pass == 1 && ty == ZoneType::Dma)
                };
                if !matches || !self.zones[i].is_ready() {
                    continue;
                }
                if let Some(pfn) = self.zones[i].alloc(order, flags) {
                    self.mark_frames(
                        i,
                        pfn,
                        order_pages(order),
                        FrameState::Used(owner),
                    );
                    return Some(PhysAddr::new((pfn * self.page_size) as u64));
                }
            }
            // 第一轮如果根本没有该类型的区域，直接结束（避免无谓第二轮）
            let has_desired = self.zones()[..].iter().any(|z| {
                (z.ty() == ZoneType::Dma) == want_dma
            });
            if !has_desired {
                break;
            }
        }
        None
    }

    /// 释放 2^order 页（物理地址）
    pub fn dealloc_pages(&mut self, phys: PhysAddr, order: usize) {
        let pfn = (phys.0 as usize) / self.page_size;
        if let Some(i) = self.zone_of(pfn) {
            self.zones[i].free(pfn, order);
            self.mark_frames(i, pfn, order_pages(order), FrameState::Free);
        }
    }

    /// 分配单页并返回物理地址（便捷接口）
    #[inline]
    pub fn alloc_page(&mut self, flags: GfpFlags, owner: FrameOwner) -> Option<PhysAddr> {
        self.alloc_pages(0, flags, owner)
    }
}

/// 页帧元数据数组对应的首个可用页号（供统计换算）
pub fn frames_base() -> usize {
    PMM.lock().as_ref().map(|p| p.frames_base_pfn).unwrap_or(0)
}

// ---------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------

// 内核映像物理末端（= __bss_end 的高半区虚拟地址 - 内核基址）
//
// 链接脚本 kernel.ld 在末尾定义了 `__bss_end`（高半区虚拟地址），
// 通过 extern 声明取其“地址”再减去 KERNEL_BASE 即得物理末端——
// 物理内存管理从这里开始接管剩余内存。
//
// 2024 Edition 要求 extern 块显式标记 unsafe（块内的项是未经
// 编译器验证的外部符号）。
unsafe extern "C" {
    static __bss_end: u8;
}

/// 计算内核映像物理末端（页对齐）
fn kernel_image_phys_end() -> u64 {
    let virt = (&raw const __bss_end) as *const u8 as u64;
    let phys = virt - crate::config::KERNEL_BASE;
    align_up(phys, PAGE_SIZE as u64)
}

/// 使用默认内存布局初始化 PMM
///
/// 本阶段引导加载程序尚未向内核传递内存映射表，因此按 QEMU 配置的
/// RAM 上限（见 config::PHYSICAL_RAM_TOP）构造一个单可用区：
/// [内核映像末端, RAM 上限)。
/// 将来打通 bootloader 内存映射传递后，改调用 init_with_regions 即可。
pub fn init() -> KernelResult<()> {
    let end = kernel_image_phys_end();
    let top = crate::config::PHYSICAL_RAM_TOP;
    if end >= top {
        return Err(KernelError::InvalidArgument);
    }
    let regions = [MemoryRegion::new(end, top, MemoryRegionType::Usable)];
    init_with_regions(&regions)
}

/// 按引导内存地图初始化 PMM
///
/// 处理流程（全部在堆就绪前完成，零堆依赖）：
/// 1. 过滤/对齐/合并可用内存区
/// 2. 计算各区域的元数据占用（伙伴系统字节区 + 页帧元数据数组）
/// 3. 从可用内存顶部向下划分字节区（Linux bootmem 式，元数据不占
///    额外静态内存，且不挤占 DMA 低地址稀缺资源）
/// 4. 按 DMA 上限把每个可用区裁成 DMA / Normal 子区域，逐个构建
/// 5. 互斥写全局状态
pub fn init_with_regions(regions: &[MemoryRegion]) -> KernelResult<()> {
    if PMM.lock().is_some() {
        return Err(KernelError::AlreadyExists);
    }
    let page_mask = (PAGE_SIZE as u64) - 1;

    // 1) 收集可用区：页对齐裁剪 + 简单合并（区域数量少，直接遍历）
    //    用栈数组而非 Vec——此时堆尚未就绪
    let mut usable: [(u64, u64); MAX_ZONES] = [(0, 0); MAX_ZONES];
    let mut usable_count = 0usize;
    for r in regions {
        if r.ty != MemoryRegionType::Usable || r.is_empty() {
            continue;
        }
        let s = align_up(r.start, PAGE_SIZE as u64);
        let e = r.end & !page_mask;
        if s >= e {
            continue;
        }
        // 与已有区间合并（简化：仅处理单区场景也可以正确工作）
        let mut merged = false;
        for m in usable[..usable_count].iter_mut() {
            if s <= m.1 && e >= m.0 {
                m.0 = m.0.min(s);
                m.1 = m.1.max(e);
                merged = true;
                break;
            }
        }
        if !merged {
            if usable_count >= MAX_ZONES {
                return Err(KernelError::InvalidArgument);
            }
            usable[usable_count] = (s, e);
            usable_count += 1;
        }
    }
    if usable_count == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // 2) 按 DMA 上限裁出子区间，统计页数与各子区元数据需求
    //    结构：zones 描述 = (ty, start_pfn, end_pfn)
    let mut zd: [[usize; 3]; MAX_ZONES] = [[0; 3]; MAX_ZONES]; // 0=ty_idx 1=start 2=end(pfn)
    let mut zi = 0usize;
    let mut total_pages = 0usize;
    for i in 0..usable_count {
        let (s, e) = usable[i];
        let dma_end = e.min(DMA_ZONE_LIMIT);
        if s < dma_end {
            let (sp, ep) = ((s / PAGE_SIZE as u64) as usize, (dma_end / PAGE_SIZE as u64) as usize);
            zd[zi] = [0, sp, ep];
            total_pages += ep - sp;
            zi += 1;
        }
        let norm_start = s.max(DMA_ZONE_LIMIT);
        if norm_start < e {
            let (sp, ep) = ((norm_start / PAGE_SIZE as u64) as usize, (e / PAGE_SIZE as u64) as usize);
            zd[zi] = [1, sp, ep];
            total_pages += ep - sp;
            zi += 1;
        }
    }
    let zones_total = zi;
    if zones_total == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // 3) 从区域顶向下划分元数据字节区
    //    cursor 始终指向“下一个可用低地址”
    let mut cursor = usable[usable_count - 1].1;

    // 3a) 伙伴系统字节区（每个区域一块独立 arena）
    let mut zone_arenas: [(u64, u64); MAX_ZONES] = [(0, 0); MAX_ZONES];
    for i in 0..zones_total {
        let pages = zd[i][2] - zd[i][1];
        let need = self::buddy::BuddyAllocator::needed_bytes(pages) as u64;
        let need_al = align_up(need, PAGE_SIZE as u64);
        cursor -= need_al;
        zone_arenas[i] = (cursor, cursor + need_al);
    }

    // 3b) 页帧元数据数组
    let frames_need = (total_pages * core::mem::size_of::<FrameMeta>()) as u64;
    let frames_need_al = align_up(frames_need, PAGE_SIZE as u64);
    cursor -= frames_need_al;
    let (frames_base, _frames_end) = (cursor, cursor + frames_need_al);
    // frames_base 必须落在第一个可用区之内（元数据不挤占低地址 DMA）
    if frames_base < usable[0].0 {
        return Err(KernelError::OutOfMemory);
    }

    // 4) 逐个构建区域
    let mut zones: [Zone; MAX_ZONES] = [
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
        Zone::uninit(),
    ];
    let mut zone_base = [0usize; MAX_ZONES];
    let mut acc = 0usize;
    for i in 0..zones_total {
        let ty = if zd[i][0] == 0 { ZoneType::Dma } else { ZoneType::Normal };
        let (sp, ep) = (zd[i][1], zd[i][2]);
        // 该区域元数据字节区（页对齐内实际可用的 need 字节）
        let (as_, ae) = zone_arenas[i];
        // 裁剪：区域结束 pfn 不能越过元数据区起始地址
        let arena_start_addr = as_ as usize;
        let zone_end_addr = ep * PAGE_SIZE;
        let capped_end_addr = zone_end_addr.min(arena_start_addr);
        if capped_end_addr <= sp * PAGE_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        let capped_ep = capped_end_addr / PAGE_SIZE;
        let pages = capped_ep - sp;
        // arena 池：从 as_ 起 need 字节
        //
        // 必须经「物理内存映射区」（直接映射）访问：boot.asm 已在跳转
        // 高半区后删除低端恒等映射，物理地址不再可直接解引用；
        // 直接映射地址 = 物理地址 + PHYSICAL_MEMORY_OFFSET
        let arena_ptr = (as_ + crate::config::PHYSICAL_MEMORY_OFFSET) as *mut u8;
        let arena_slice: &'static mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(arena_ptr, (ae - as_) as usize) };
        // 页数可能因裁剪变小，导致 arena 变大（安全，只是多留了余量）
        let mut z = unsafe {
            Zone::from_arena(ty, sp, capped_ep, arena_slice)
        };
        z.finalize();
        zones[i] = z;
        zone_base[i] = acc;
        acc += pages;
    }

    // 帧元数据初始化（同样经直接映射区访问）
    let frame_meta_ptr = (frames_base + crate::config::PHYSICAL_MEMORY_OFFSET) as *mut FrameMeta;
    let frame_meta_slice: &'static mut [FrameMeta] = unsafe {
        core::slice::from_raw_parts_mut(frame_meta_ptr, total_pages)
    };
    for m in frame_meta_slice.iter_mut() {
        *m = FrameMeta::free();
    }

    // 5) 拓扑：单节点拥有全部区域
    //    zone 索引表需要 'static：放一个 static 数组里
    static ZONE_IDS: [usize; MAX_ZONES] = [0, 1, 2, 3, 4, 5, 6, 7];
    let topology = NumaTopology::single_node(&ZONE_IDS[..zones_total]);

    *PMM.lock() = Some(PmmState {
        zones,
        zone_count: zones_total,
        zone_base,
        frames: Some(frame_meta_slice),
        frames_base_pfn: zd[0][1],
        topology,
        page_size: PAGE_SIZE,
    });

    Ok(())
}

// ---------------------------------------------------------------------
// 对外接口（加锁的便捷函数）
// ---------------------------------------------------------------------

/// 分配 2^order 页（加锁），返回物理地址；失败返回 None
pub fn alloc_pages(order: usize, flags: GfpFlags, owner: FrameOwner) -> Option<PhysAddr> {
    let mut pm = PMM.lock();
    pm.as_mut()?.alloc_pages(order, flags, owner)
}

/// 释放 2^order 页（加锁）
pub fn free_pages(phys: PhysAddr, order: usize) {
    if let Some(pm) = PMM.lock().as_mut() {
        pm.dealloc_pages(phys, order);
    }
}

/// 分配一页（加锁）
#[inline]
pub fn alloc_page(flags: GfpFlags, owner: FrameOwner) -> Option<PhysAddr> {
    alloc_pages(0, flags, owner)
}

/// 帧分配器适配（实现 arch/paging.rs 的 FrameAllocator）
///
/// 为什么单独实现这个 trait：页表映射器（PageTableMapper::map）通过
/// 该 trait 获取页表中间级页，PMM 承担这一职责正好消除循环依赖。
impl FrameAllocator for PmmState {
    fn allocate(&mut self) -> Option<Frame> {
        self.alloc_pages(0, GfpFlags::NONE, FrameOwner::PageTable)
            .map(Frame::containing)
    }

    fn deallocate(&mut self, frame: Frame) {
        self.dealloc_pages(frame.start, 0);
    }
}

/// 页提供者适配（实现 heap/slub.rs 的 PageProvider）
///
/// 堆分配器（SLUB/kmalloc）向 PMM 取页；返回的是物理内存直接映射
/// 虚拟地址（phys + PHYSICAL_MEMORY_OFFSET），因为 slab 页头、对象
/// 都在这个地址空间被访问。
impl crate::mm::heap::slub::PageProvider for PmmState {
    fn alloc_pages(&mut self, order: usize) -> Option<usize> {
        self.alloc_pages(order, GfpFlags::NONE, FrameOwner::Slab)
            .map(|phys| (phys.0 + crate::config::PHYSICAL_MEMORY_OFFSET) as usize)
    }

    fn free_pages(&mut self, order: usize, page_addr: usize) {
        let phys = PhysAddr(page_addr as u64 - crate::config::PHYSICAL_MEMORY_OFFSET);
        self.dealloc_pages(phys, order);
    }
}

/// 在持锁状态下获取 PMM 的可变引用并执行闭包
///
/// 为什么提供这个接口：堆（GlobalAlloc）等下游需要“拿一把锁、
/// 借出 &mut PmmState、用完后归还锁”的同一调用模式；集中封装可
/// 避免各处重复加锁逻辑，并保证锁序统一。
pub fn with_pmm<R>(f: impl FnOnce(&mut PmmState) -> R) -> Option<R> {
    let mut g = PMM.lock();
    let pm = g.as_mut()?;
    Some(f(pm))
}

/// 锁内适配器：每次调用临时获取 PMM 锁
///
/// 用途：页表映射器（PageTableMapper::map）的 FrameAllocator 接口
/// 需要 `&mut A`，而调用方可能并不持有 PMM 锁；此适配器把「取锁 →
/// 借出 PmmState」封装在单次调用内，天然避免锁的嵌套竞争
/// （每次调用都完整取还一次锁，不存在锁内再取锁）。
pub struct LockedPmm;

impl FrameAllocator for LockedPmm {
    fn allocate(&mut self) -> Option<Frame> {
        PMM.lock()
            .as_mut()?
            .alloc_pages(0, GfpFlags::NONE, FrameOwner::PageTable)
            .map(Frame::containing)
    }

    fn deallocate(&mut self, frame: Frame) {
        if let Some(pm) = PMM.lock().as_mut() {
            pm.dealloc_pages(frame.start, 0);
        }
    }
}

impl crate::mm::heap::slub::PageProvider for LockedPmm {
    fn alloc_pages(&mut self, order: usize) -> Option<usize> {
        PMM.lock()
            .as_mut()?
            .alloc_pages(order, GfpFlags::NONE, FrameOwner::Slab)
            .map(|phys| (phys.0 + crate::config::PHYSICAL_MEMORY_OFFSET) as usize)
    }

    fn free_pages(&mut self, order: usize, page_addr: usize) {
        let phys = PhysAddr(page_addr as u64 - crate::config::PHYSICAL_MEMORY_OFFSET);
        if let Some(pm) = PMM.lock().as_mut() {
            pm.dealloc_pages(phys, order);
        }
    }
}

// ---------------------------------------------------------------------
// 统计与监控
// ---------------------------------------------------------------------

/// 单个区域的统计快照
#[derive(Debug, Clone, Copy)]
pub struct ZoneStat {
    /// 区域类型名
    pub ty: &'static str,
    /// 起始物理页号
    pub start_pfn: usize,
    /// 结束物理页号
    pub end_pfn: usize,
    /// 总页数
    pub total: usize,
    /// 空闲页数
    pub free: usize,
    /// 使用页数
    pub used: usize,
    /// 预留页数
    pub reserved: usize,
    /// 最大分配阶
    pub max_order: usize,
    /// 当前水位档
    pub level: &'static str,
    /// 分配请求次数
    pub allocs: u64,
    /// 释放次数
    pub frees: u64,
    /// OOM 次数
    pub oom: u64,
}

/// 全局 PMM 统计快照
#[derive(Debug, Clone, Copy)]
pub struct PmmStats {
    /// 各区域统计
    pub zones: [ZoneStat; MAX_ZONES],
    /// 区域数
    pub zone_count: usize,
    /// 总页数
    pub total_pages: usize,
    /// 空闲页数
    pub free_pages: usize,
    /// 已用页数
    pub used_pages: usize,
    /// 预留页数
    pub reserved_pages: usize,
}

impl Default for PmmStats {
    fn default() -> Self {
        Self {
            zones: [ZoneStat {
                ty: "",
                start_pfn: 0,
                end_pfn: 0,
                total: 0,
                free: 0,
                used: 0,
                reserved: 0,
                max_order: 0,
                level: "",
                allocs: 0,
                frees: 0,
                oom: 0,
            }; MAX_ZONES],
            zone_count: 0,
            total_pages: 0,
            free_pages: 0,
            used_pages: 0,
            reserved_pages: 0,
        }
    }
}

/// 获取全局内存统计快照
pub fn stats() -> PmmStats {
    let pm = PMM.lock();
    let mut s = PmmStats::default();
    let Some(pm) = pm.as_ref() else {
        return s;
    };
    s.zone_count = pm.zone_count;
    s.total_pages = pm.total_pages();
    s.free_pages = pm.free_page_count();
    for (i, z) in pm.zones().iter().enumerate() {
        let p = z.pressure();
        s.used_pages += z.used_pages();
        s.reserved_pages += z.reserved_pages();
        s.zones[i] = ZoneStat {
            ty: z.ty().name(),
            start_pfn: z.start_pfn(),
            end_pfn: z.end_pfn(),
            total: z.total_pages(),
            free: z.free_pages(),
            used: z.used_pages(),
            reserved: z.reserved_pages(),
            max_order: z.max_order(),
            level: z.level().as_str(),
            allocs: p.alloc_requests,
            frees: p.free_requests,
            oom: p.oom,
        };
    }
    s
}

/// PMM 是否已初始化
pub fn is_ready() -> bool {
    PMM.lock().is_some()
}
