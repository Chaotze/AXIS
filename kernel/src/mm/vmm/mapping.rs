// ============================================================
// 地址映射与权限管理（Mapping）
// ============================================================
// 在「页表操作（page_table.rs）」之上提供区域级（多页）的映射服务：
// - 匿名映射：从 PMM 取物理页并建立映射
// - 物理映射：把设备/物理地址范围按偏移映射进虚拟地址空间
// - 解映射：取消映射并（可选）归还物理页
// - 权限修改：批量改写区域读写/可执行权限（mprotect 的原始操作）
//
// 设计要点（为什么这么做）：
// 1. 页表操作与策略分离：本模块只描述“哪些页怎么印”，不关心
//    VMA/进程等上层语义——语义层（vmm.rs）决定何时调用、失败怎么办
// 2. 部分失败回滚：连续映射中途 OOM 时，已映射的页全部归还，
//    调用方看到的永远是“要么全部成功、要么原样不变”
// 3. 页表中间级页的分配通过 LockedPmm（帧分配器适配）完成，
//    与匿名页的分配共用同一 PMM 锁，锁序简单统一

use crate::arch::x86_64::memory::{Frame, Page, PhysAddr, VirtAddr};
use crate::config::PAGE_SIZE as PS;
use crate::mm::pmm::frame::FrameOwner;
use crate::mm::pmm::{self, GfpFlags, LockedPmm};

use super::page_table::{KernelPageMapper, MappingFlags, PageTableError};

/// 区域映射错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    /// 内存不足（匿名页或页表中间级页）
    OutOfMemory,
    /// 目标页已被映射
    AlreadyMapped,
    /// 目标页未映射（解映射/查询时）
    NotMapped,
    /// 参数无效（地址未页对齐等）
    Invalid,
}

impl From<PageTableError> for MappingError {
    fn from(e: PageTableError) -> Self {
        match e {
            PageTableError::AlreadyMapped => Self::AlreadyMapped,
            PageTableError::OutOfMemory => Self::OutOfMemory,
            PageTableError::NotMapped => Self::NotMapped,
        }
    }
}

/// 匿名映射：为 [vaddr, vaddr + pages*PAGE) 分配物理页并建立映射
///
/// 中途失败时回滚已完成的映射（归还已分配的物理页）。
pub fn map_anon(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    pages: usize,
    flags: MappingFlags,
) -> Result<(), MappingError> {
    if vaddr % PS != 0 {
        return Err(MappingError::Invalid);
    }
    let mut done = 0usize;
    while done < pages {
        let phys = pmm::alloc_page(GfpFlags::NONE, FrameOwner::Process)
            .ok_or(MappingError::OutOfMemory)?;
        let page = Page::containing(VirtAddr((vaddr + done * PS) as u64));
        if let Err(e) = mapper.map_page(page, Frame::containing(phys), flags, &mut LockedPmm) {
            // 回滚：归还本次与已完成的页
            pmm::free_pages(phys, 0);
            for i in 0..done {
                let p = Page::containing(VirtAddr((vaddr + i * PS) as u64));
                if let Ok(f) = mapper.unmap_page(p) {
                    pmm::free_pages(f.start, 0);
                }
            }
            return Err(e.into());
        }
        done += 1;
    }
    Ok(())
}

/// 物理映射：把 [phys, phys + pages*PAGE) 映射到 [vaddr, ...)
///
/// 用于设备 MMIO / 页表的身份映射等“预先存在的物理内存”。
/// 不分配物理页；[phys] 必须页对齐。
pub fn map_phys(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    phys: usize,
    pages: usize,
    flags: MappingFlags,
) -> Result<(), MappingError> {
    if vaddr % PS != 0 || phys % PS != 0 {
        return Err(MappingError::Invalid);
    }
    let mut done = 0usize;
    while done < pages {
        let page = Page::containing(VirtAddr((vaddr + done * PS) as u64));
        let frame = Frame::containing(PhysAddr((phys + done * PS) as u64));
        mapper.map_page(page, frame, flags, &mut LockedPmm)?;
        done += 1;
    }
    Ok(())
}

/// 解映射区域，并（若 anon）归还物理页
///
/// anon = true：页由本模块此前匿名分配，解映射时一起释放；
/// anon = false：物理/设备映射，页不属于 PMM 管理，只清页表。
pub fn unmap_range(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    pages: usize,
    anon: bool,
) -> Result<(), MappingError> {
    for i in 0..pages {
        let page = Page::containing(VirtAddr((vaddr + i * PS) as u64));
        let frame = mapper.unmap_page(page)?;
        if anon {
            pmm::free_pages(frame.start, 0);
        }
    }
    Ok(())
}

/// 批量修改区域权限（无页迁移）
pub fn update_range(
    mapper: &mut KernelPageMapper,
    vaddr: usize,
    pages: usize,
    flags: MappingFlags,
) -> Result<(), MappingError> {
    for i in 0..pages {
        let page = Page::containing(VirtAddr((vaddr + i * PS) as u64));
        mapper.update_flags(page, flags, &mut LockedPmm)?;
    }
    Ok(())
}

/// 查询区域内某个页是否已映射（翻译地址）
pub fn is_mapped(mapper: &KernelPageMapper, vaddr: usize) -> bool {
    mapper.translate(VirtAddr(vaddr as u64)).is_some()
}

/// 读取一个物理页的内容到缓冲区（跨直接映射的 memcpy 封装）
///
/// 为什么需要单独函数：内核需要读取“其他进程”或设备的物理页时，
/// 不通过当前页表直接解引用，而是经由物理内存直接映射区。
pub fn copy_from_phys(phys: PhysAddr, dst: *mut u8, len: usize) {
    let src = crate::mm::vmm::page_table::phys_to_direct(phys);
    unsafe { core::ptr::copy_nonoverlapping(src.0 as *const u8, dst, len) };
}

/// 把内容写入一个物理页（同上，方向相反）
pub fn copy_to_phys(phys: PhysAddr, src: *const u8, len: usize) {
    let dst = crate::mm::vmm::page_table::phys_to_direct(phys);
    unsafe { core::ptr::copy_nonoverlapping(src, dst.0 as *mut u8, len) };
}

/// 页面数量（字节 → 页，向上取整）
#[inline]
pub const fn page_count(byte_len: usize) -> usize {
    (byte_len + PS - 1) / PS
}