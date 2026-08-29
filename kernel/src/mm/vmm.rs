// ============================================================
// 虚拟内存管理（VMM）
// ============================================================
// 内存管理层的最外层：
// - 持有「当前地址空间」的页表映射器、VMA 管理、交换登记
// - 提供 mmap / munmap / mprotect 语义（映射 + VMA + 缺页路由）
// - 提供缺页处理器入口（arch 中断的 #PF 挂钩调用）
//
// 锁序约定（全内核统一，防止死锁）：
//   VMM 锁 → PMM 锁 → HEAP 锁
// 缺页路径按此顺序取锁；任何模块不得反向取锁。

pub mod cow;
pub mod layout;
pub mod mapping;
pub mod page_table;
pub mod swap;
pub mod vma;

use crate::config::PAGE_SIZE as PS;
use crate::prelude::{KernelError, KernelResult};
use crate::sync::Spinlock;

use crate::mm::pmm::frame::FrameOwner;
use crate::mm::pmm::{alloc_page, free_pages, GfpFlags, LockedPmm};

use self::cow::{break_cow, is_cow};
use self::mapping::{map_anon, MappingError};
use self::page_table::{KernelPageMapper, Page, VirtAddr};
use self::swap::{MemorySwapStore, SwapManager};
use self::vma::{Vma, VmaFlags, VmaManager, VmaPerm};

/// 是否启用交换模拟（缺页时优先检查交换登记）
const SWAP_ENABLED: bool = true;

/// VMM 统计
#[derive(Debug, Clone, Copy, Default)]
pub struct VmmStats {
    /// 缺页总数
    pub page_faults: u64,
    /// 缺页已解决（按需分页/COW/交换）
    pub resolved: u64,
    /// 未解决缺页（内核错误）
    pub unresolved: u64,
    /// COW 拆解次数
    pub cow_breaks: u64,
    /// 按需分页成功次数
    pub demand_pages: u64,
    /// 换出次数
    pub swap_outs: u64,
    /// 换入次数
    pub swap_ins: u64,
    /// 当前登记换出的页数
    pub swapped_pages: usize,
}

/// VMM 全局状态
pub struct VmmState {
    /// 当前页表映射器（基于 CR3）
    pub mapper: KernelPageMapper,
    /// VMA 管理器
    pub vmas: VmaManager,
    /// 交换登记管理器
    pub swap: SwapManager,
    /// 内存交换存储（模拟磁盘；将来换成块设备实现）
    pub swap_store: MemorySwapStore<'static>,
    /// 统计
    pub stats: VmmStats,
}

/// VMM 全局单例
static VMM: Spinlock<Option<VmmState>> = Spinlock::new(None);

/// 把 VMA 权限换算成映射权限
fn perms_to_flags(perms: &VmaPerm) -> page_table::MappingFlags {
    page_table::MappingFlags {
        present: true,
        writable: perms.write,
        executable: perms.execute,
        user: perms.user,
        cow: false,
        global: false,
    }
}

/// 页数工具（字节 → 页，向上取整）
#[inline]
pub fn page_count(len: usize) -> usize {
    (len + PS - 1) / PS
}

/// 初始化 VMM（必须在 pmm::init、heap::init 之后调用）
pub fn init() -> KernelResult<()> {
    if VMM.lock().is_some() {
        return Err(KernelError::AlreadyExists);
    }

    let mapper = KernelPageMapper::current();

    // 交换存储：使用零状态 ZST 适配器 LockedPmm 作为页提供者。
    // LockedPmm 不持有任何数据（每次调用都临时取 PMM 锁），
    // 因此对它的多个可变引用在语义上是安全的——引用的是“无状态
    // 单例”而非实际内存，符合 ZST 的排他性豁免。
    let provider: &'static mut LockedPmm = unsafe {
        &mut *core::ptr::NonNull::<LockedPmm>::dangling().as_ptr()
    };
    let swap_store = MemorySwapStore::new(256, provider);

    *VMM.lock() = Some(VmmState {
        mapper,
        vmas: VmaManager::new(),
        swap: SwapManager::new(),
        swap_store,
        stats: VmmStats::default(),
    });
    Ok(())
}

/// VMM 是否已初始化
pub fn is_ready() -> bool {
    VMM.lock().is_some()
}

/// 在持锁状态下访问 VMM 状态
pub fn with_vmm<R>(f: impl FnOnce(&mut VmmState) -> R) -> Option<R> {
    let mut g = VMM.lock();
    let v = g.as_mut()?;
    Some(f(v))
}

/// mmap 返回错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmapError {
    /// 内存不足
    OutOfMemory,
    /// 与已有区域重叠
    Overlap,
    /// 参数无效
    Invalid,
}

impl From<MappingError> for MmapError {
    fn from(e: MappingError) -> Self {
        match e {
            MappingError::OutOfMemory => Self::OutOfMemory,
            MappingError::AlreadyMapped => Self::Overlap,
            _ => Self::Invalid,
        }
    }
}

/// 找到一块至少 size 字节的空闲虚拟地址区间
///
/// 简单策略：从 USER_BASE 起的线性游标扫描（进程 VMA 数量少、
/// 地址空间巨大，里程碑阶段无需区间树）；hint 表示优先尝试位置。
fn find_free_range(vmas: &VmaManager, hint: usize, size: usize) -> Option<usize> {
    let mut cursor = hint.max(layout::USER_BASE as usize);
    // 内核态 hint（>= KERNEL_BASE，供内核自测/驱动映射使用）向上搜到
    // 内核堆预留区为止；用户态 hint 则在用户空间范围内搜索
    let end_scan = if hint >= layout::KERNEL_BASE as usize {
        layout::KERNEL_HEAP_START as usize
    } else {
        layout::USER_STACK_BOTTOM as usize
    };
    for v in vmas.iter() {
        if v.start >= cursor + size {
            return Some(cursor);
        }
        cursor = cursor.max(v.end.max(v.start + PS));
        if cursor >= end_scan {
            return None;
        }
    }
    (cursor + size <= end_scan).then_some(cursor)
}

/// mmap 匿名区域（按需分页：只登记 VMA，页在首次访问时由缺页分配）
///
/// 为什么默认按需分页：进程中大量映射可能根本不会被访问，立即
/// 分配全部页白白浪费内存；缺页时按需一页一页给，是类 Linux 的
/// 默认行为（demand paging），也顺带验证缺页路径。
pub fn mmap_anon(
    hint: Option<usize>,
    size: usize,
    perms: VmaPerm,
    flags: VmaFlags,
) -> Result<usize, MmapError> {
    if size == 0 || size % PS != 0 {
        return Err(MmapError::Invalid);
    }
    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return Err(MmapError::Invalid);
    };

    let addr = find_free_range(&vm.vmas, hint.unwrap_or(layout::USER_BASE as usize), size)
        .ok_or(MmapError::OutOfMemory)?;

    let vma = Vma {
        start: addr,
        end: addr + size,
        perms,
        flags,
    };
    vm.vmas.insert(vma).map_err(|_| MmapError::Overlap)?;
    Ok(addr)
}

/// 立即分配并映射（供需要马上可访问内存的场景，如内核/驱动缓冲）
pub fn mmap_anon_eager(
    addr: usize,
    size: usize,
    perms: VmaPerm,
    flags: VmaFlags,
) -> Result<(), MmapError> {
    if addr % PS != 0 || size == 0 || size % PS != 0 {
        return Err(MmapError::Invalid);
    }
    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return Err(MmapError::Invalid);
    };
    let pages = size / PS;
    let mflags = perms_to_flags(&perms);
    map_anon(&mut vm.mapper, addr, pages, mflags)?;

    let vma = Vma { start: addr, end: addr + size, perms, flags };
    vm.vmas.insert(vma).map_err(|_| MmapError::Overlap)?;
    Ok(())
}

/// munmap：移除 VMA 并解除映射、归还物理页
pub fn munmap(addr: usize, size: usize) -> Result<(), MmapError> {
    if addr % PS != 0 || size == 0 {
        return Err(MmapError::Invalid);
    }
    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return Err(MmapError::Invalid);
    };
    let pages = page_count(size);
    // 逐页解除映射并归还物理页；按需分页未分配的页（尚无 PTE）
    // 属于正常情况，跳过即可
    for i in 0..pages {
        let page = Page::containing(VirtAddr((addr + i * PS) as u64));
        if vm.mapper.translate(page.start).is_some() {
            let frame = vm
                .mapper
                .unmap_page(page)
                .map_err(|_| MmapError::Invalid)?;
            free_pages(frame.start, 0);
        }
    }
    vm.vmas.remove_range(addr, addr + size.max(PS));
    Ok(())
}

/// mprotect：修改区域权限（页表 + VMA 同步更新）
pub fn mprotect(addr: usize, size: usize, perms: VmaPerm) -> Result<(), MmapError> {
    if addr % PS != 0 || size == 0 {
        return Err(MmapError::Invalid);
    }
    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return Err(MmapError::Invalid);
    };
    let mflags = perms_to_flags(&perms);
    let pages = size / PS;
    // 已按需分配的页：逐页改页表标志（未分配的页交给缺页时按新权限映射）
    for i in 0..pages {
        let page = Page::containing(VirtAddr((addr + i * PS) as u64));
        if vm.mapper.translate(page.start).is_some() {
            vm.mapper
                .update_flags(page, mflags, &mut LockedPmm)
                .map_err(|_| MmapError::Invalid)?;
        }
    }
    // VMA 记录同步更新（仅支持整段覆盖，部分重叠拒绝——里程碑约束）
    let idx = vm.vmas.find_range(addr, addr + size);
    if let Some(idx) = idx {
        let vma = vm.vmas.get(idx).copied();
        if let Some(vma) = vma {
            if vma.start == addr && vma.end == addr + size {
                if let Some(nv) = vm.vmas.find_mut(addr) {
                    nv.perms = perms;
                }
                return Ok(());
            }
        }
    }
    Err(MmapError::Invalid)
}

/// 查找包含 addr 的 VMA
pub fn find_vma(addr: usize) -> Option<Vma> {
    let v = VMM.lock();
    v.as_ref()?.vmas.find(addr).copied()
}

/// 查找覆盖 [addr, addr+size) 的 VMA
pub fn find_vma_range(addr: usize, size: usize) -> Option<Vma> {
    let v = VMM.lock();
    let vm = v.as_ref()?;
    let idx = vm.vmas.find_range(addr, addr + size)?;
    vm.vmas.get(idx).copied()
}

/// 缺页处理结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultResult {
    /// 已解决（恢复了页表/权限/内容），可返回用户态
    Resolved,
    /// 无法解决（真正的错误：非法访问/内核 bug），上层应 panic
    Unresolved,
}

/// 缺页处理器（由 arch 中断处理钩子调用）
///
/// 处理逻辑（按优先级）：
/// 1. 交换：该页被登记换出 → 换入并重映射
/// 2. COW：命中 COW 页且有写意图 → 拆开（复制新页独立可写）
/// 3. 按需分页：地址位于匿名 VMA 且页不存在 → 分配一页映射
/// 4. 其余 → Unresolved（由上层 panic 并打印现场）
///
/// error_code 约定（x86_64）：
///   bit0=Present, bit1=Write, bit2=User, bit4=InstructionFetch
pub fn handle_page_fault(fault_addr: u64, error_code: u64) -> PageFaultResult {
    let write = error_code & 0x2 != 0;
    let present = error_code & 0x1 != 0;
    let page = Page::containing(VirtAddr(fault_addr));
    let vaddr = page.start.0 as usize;

    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return PageFaultResult::Unresolved;
    };
    vm.stats.page_faults += 1;

    // 1) 交换换入：读到新分配页 → 重新映射
    if SWAP_ENABLED && vm.swap.is_swapped(vaddr) {
        let Some(tmp) = alloc_page(GfpFlags::NONE, FrameOwner::Process) else {
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        };
        let dst = crate::mm::vmm::page_table::phys_to_direct(tmp);
        if vm.swap.swap_in(&mut vm.swap_store, vaddr, dst.0 as *mut u8).is_err() {
            free_pages(tmp, 0);
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        }
        let vma = vm.vmas.find(vaddr).copied();
        let flags = vma.map(|x| perms_to_flags(&x.perms)).unwrap_or_default();
        if vm
            .mapper
            .map_page(
                page,
                crate::arch::x86_64::memory::Frame::containing(tmp),
                flags,
                &mut LockedPmm,
            )
            .is_err()
        {
            free_pages(tmp, 0);
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        }
        vm.stats.swap_ins += 1;
        vm.stats.resolved += 1;
        return PageFaultResult::Resolved;
    }

    if present && write && is_cow(&vm.mapper, vaddr) {
        // 2) COW：共享只读页上发生写缺页 → 拆开
        let Some(vma) = vm.vmas.find(vaddr).copied() else {
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        };
        let mflags = perms_to_flags(&vma.perms);
        match break_cow(&mut vm.mapper, vaddr, mflags) {
            Ok(()) => {
                vm.stats.cow_breaks += 1;
                vm.stats.resolved += 1;
                PageFaultResult::Resolved
            }
            Err(_) => {
                vm.stats.unresolved += 1;
                PageFaultResult::Unresolved
            }
        }
    } else if !present {
        // 3) 按需分页：必须在已登记的匿名 VMA 内
        let Some(vma) = vm.vmas.find(vaddr).copied() else {
            println!("[PF-DBG] no vma for {:#x}", vaddr);
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        };
        // 写意图但 VMA 不可写 → 段错误性质的错误
        if write && !vma.perms.write {
            println!("[PF-DBG] write to non-writable vma {:#x}", vaddr);
            vm.stats.unresolved += 1;
            return PageFaultResult::Unresolved;
        }
        let mflags = perms_to_flags(&vma.perms);
        match map_anon(&mut vm.mapper, vaddr, 1, mflags) {
            Ok(()) => {
                vm.stats.demand_pages += 1;
                vm.stats.resolved += 1;
                PageFaultResult::Resolved
            }
            Err(e) => {
                println!("[PF-DBG] map_anon failed: {:?} at {:#x}", e, vaddr);
                vm.stats.unresolved += 1;
                PageFaultResult::Unresolved
            }
        }
    } else {
        // 4) 已映射但权限冲突等：无法解决
        vm.stats.unresolved += 1;
        PageFaultResult::Unresolved
    }
}

/// 显式换出一页（接口就绪；由内存压力或上层策略触发）
pub fn swap_out_page(addr: usize) -> bool {
    let mut v = VMM.lock();
    let Some(vm) = v.as_mut() else {
        return false;
    };
    // 必须落在可读 VMA 且页已映射
    let Some(vma) = vm.vmas.find(addr).copied() else {
        return false;
    };
    if !vma.perms.read {
        return false;
    }
    let page = Page::containing(VirtAddr(addr as u64));
    let Some(phys) = vm.mapper.translate(page.start) else {
        return false;
    };
    let src = crate::mm::vmm::page_table::phys_to_direct(phys);
    if vm.swap.swap_out(&mut vm.swap_store, addr, src.0 as *const u8).is_err() {
        return false;
    }
    // 解除映射并归还物理页（此后该地址访问走缺页换入）
    if vm.mapper.unmap_page(page).is_ok() {
        free_pages(phys, 0);
    }
    vm.stats.swap_outs += 1;
    true
}

/// 获取 VMM 统计快照
pub fn stats() -> VmmStats {
    let v = VMM.lock();
    match v.as_ref() {
        Some(vm) => {
            let mut s = vm.stats;
            s.swapped_pages = vm.swap.swapped_pages();
            s
        }
        None => VmmStats::default(),
    }
}

/// VMA 快照（监控/调试输出用）
pub fn vma_snapshot() -> alloc::vec::Vec<(usize, usize, &'static str)> {
    let v = VMM.lock();
    let mut out = alloc::vec::Vec::new();
    if let Some(vm) = v.as_ref() {
        for x in vm.vmas.iter() {
            let kind = if x.flags.stack {
                "stack"
            } else if x.flags.heap {
                "heap"
            } else if x.flags.anonymous {
                "anon"
            } else {
                "other"
            };
            out.push((x.start, x.end, kind));
        }
    }
    out
}