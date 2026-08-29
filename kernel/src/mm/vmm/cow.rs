// ============================================================
// 写时复制（Copy-On-Write）
// ============================================================
// 进程 fork 时，父子进程共享同一批物理页、把这些页标记为只读，
// 任一进程写入时触发 #PF，缺页处理器调用本模块「复制一页」——
// 实现“共享不复制、写时才复制”的经典内存优化。
//
// 设计要点（为什么这么做）：
// 1. 两个层面拆开：
//    - mark_cow / cow_share：地址空间层面“如何让两段虚拟地址共用
//      一页且只读”（fork 时用）
//    - break_cow：缺页层面“把某一次写入拆成独立页”（写时用）
//    这样缺页处理器只需要做“拆”，而 fork 逻辑只需要做“并”。
// 2. COW 标记用页表项的软件位（PageTableFlags::COW，bit 9）：
//    不占 VMA 额外状态，逐页可变，天然支持局部 COW。
// 3. 复制用「物理直接映射区」memcpy：本页表未映射目标页也能安全
//    读写物理内容，避免锁定/临时映射的复杂度。

use crate::arch::x86_64::memory::{Frame, Page, VirtAddr};
use crate::config::PAGE_SIZE as PS;
use crate::mm::pmm::frame::FrameOwner;
use crate::mm::pmm::{self, GfpFlags, LockedPmm};

use super::mapping::copy_from_phys;
use super::page_table::{KernelPageMapper, MappingFlags};

/// COW 相关错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CowError {
    /// 目标页未映射
    NotMapped,
    /// 内存不足（无法复制新页）
    OutOfMemory,
}

/// 把一页标记为「只读 + COW」。
///
/// flags 是 VMA 期望的最终权限（含 writable），本函数自动去掉
/// writable 并附加 COW 位——共享期间任何一方都不可写。
pub fn mark_cow(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    flags: MappingFlags,
) -> Result<(), CowError> {
    let cow_flags = MappingFlags {
        writable: false,
        cow: true,
        ..flags
    };
    let page = Page::containing(VirtAddr(vaddr as u64));
    mapper
        .update_flags(page, cow_flags, &mut LockedPmm)
        .map_err(|_| CowError::NotMapped)
}

/// 让 dst_vaddr 与 src_vaddr 共享 src 的物理页，两端都只读 + COW
///
/// fork 的一个页：子进程地址空间把该虚拟页指到父进程的物理页。
/// flags 为两端共享后的权限（函数内部强制只读 + COW）。
pub fn cow_share(
    mapper: &mut KernelPageMapper,
    src_vaddr: usize,
    dst_vaddr: usize,
    flags: MappingFlags,
) -> Result<(), CowError> {
    // 1) 源页必须已映射，取物理帧
    let phys = mapper
        .translate(VirtAddr(src_vaddr as u64))
        .ok_or(CowError::NotMapped)?;
    let frame = Frame::containing(phys);

    // 2) 源置只读 + COW
    mark_cow(mapper, src_vaddr, flags)?;

    // 3) 目标映射同一帧（只读 + COW）
    let cow_flags = MappingFlags {
        writable: false,
        cow: true,
        ..flags
    };
    let page = Page::containing(VirtAddr(dst_vaddr as u64));
    mapper
        .map_page(page, frame, cow_flags, &mut LockedPmm)
        .map_err(|_| CowError::OutOfMemory)
}

/// 拆开一处 COW：新分配物理页、拷贝内容、以可写权限重映射
///
/// 由缺页处理器在「COW 页 + 写意图」时调用。本映射从此拥有
/// 独立物理页；其他共享者仍指向旧页（保持只读），互不影响。
pub fn break_cow(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    flags: MappingFlags,
) -> Result<(), CowError> {
    let old_phys = mapper
        .translate(VirtAddr(vaddr as u64))
        .ok_or(CowError::NotMapped)?;

    let new_phys = pmm::alloc_page(GfpFlags::NONE, FrameOwner::Process)
        .ok_or(CowError::OutOfMemory)?;

    // 拷贝 4096 字节：源 → 新页（物理直接映射区中转）
    copy_from_phys(
        old_phys,
        crate::mm::vmm::page_table::phys_to_direct(new_phys).0 as *mut u8,
        PS,
    );

    let page = Page::containing(VirtAddr(vaddr as u64));
    let owned = Frame::containing(new_phys);

    // 先取消对共享页的引用：共享框架只有我们引用 + 可能其他进程
    // 引用；取消本映射不会释放它（释放由各进程 unmap 时完成）。
    mapper.unmap_page(page).map_err(|_| CowError::NotMapped)?;

    // 以可写（copied）权限映射新帧
    let final_flags = MappingFlags {
        writable: true,
        cow: false,
        ..flags
    };
    mapper
        .map_page(page, owned, final_flags, &mut LockedPmm)
        .map_err(|_| CowError::OutOfMemory)?;
    Ok(())
}

/// 判断某页是否处于 COW 状态（读取页表标志）
pub fn is_cow(mapper: &KernelPageMapper, vaddr: usize) -> bool {
    mapper
        .pte_flags(VirtAddr(vaddr as u64))
        .map(|f| f.contains(crate::arch::x86_64::paging::PageTableFlags::COW))
        .unwrap_or(false)
}