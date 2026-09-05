// ============================================================
// PCI DMA 管理
// ============================================================
// 为 DMA 类设备分配物理连续的缓冲区，并维护 物理地址 ↔ 虚拟地址
// 的换算。
//
// 为什么需要专门分配：设备通过总线直接读写物理内存，缓冲区必须
// 物理连续，且老式 ISA/PCI 设备只能访问低 16MB（DMA Zone）；
// 普通内核堆（SLUB）不保证物理连续性，也不保证落在 DMA 区。
//
// 实现：从 PMM 的 DMA Zone 分配页，释放时归还；虚拟地址用直接
// 映射区（phys + PHYSICAL_MEMORY_OFFSET），无需额外建页表映射。

use core::ptr;
use core::ops::{Deref, DerefMut};

use crate::arch::x86_64::memory::PhysAddr;
use crate::config::PAGE_SIZE;
use crate::mm::pmm::{self, GfpFlags, frame::FrameOwner};
use crate::prelude::KernelResult;

/// DMA 缓冲区：持有若干物理连续的页，释放时自动归还
pub struct DmaBuf {
    /// 起始物理地址
    phys: u64,
    /// 起始虚拟地址（直接映射区）
    virt: *mut u8,
    /// 长度（字节）
    size: usize,
    /// 分配阶数（2^order 页）
    order: usize,
}

// 手动实现 Send：裸指针指向直接映射的物理内存，转移所有权语义
// 由 DmaBuf 自身（Drop 归还页）保证
unsafe impl Send for DmaBuf {}

impl DmaBuf {
    /// 分配 DMA 缓冲区（物理连续，优先 DMA Zone）
    ///
    /// size 向上取整到页。为什么必须有 size 上限：DMA Zone 只有
    /// 16MB，极端分配会耗尽低地址页，影响老式设备。
    pub fn alloc(size: usize) -> KernelResult<Self> {
        if size == 0 || size > 16 * 1024 * 1024 {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }
        // 计算需要 2^order 页
        let pages = size.div_ceil(PAGE_SIZE);
        let order = (usize::BITS - 1 - pages.leading_zeros()) as usize; // ceil(log2)
        let order = if (1usize << order) < pages { order + 1 } else { order };

        let Some(phys) = pmm::alloc_pages(order, GfpFlags::DMA, FrameOwner::Kernel) else {
            return Err(crate::lib::result::KernelError::OutOfMemory);
        };

        let virt = phys.to_virt().0 as *mut u8;
        Ok(Self {
            phys: phys.0,
            virt,
            size,
            order,
        })
    }

    /// 物理地址（供设备 DMA 使用）
    pub const fn phys(&self) -> u64 {
        self.phys
    }

    /// 虚拟地址（供 CPU 访问）
    pub const fn as_ptr(&self) -> *mut u8 {
        self.virt
    }

    /// 长度
    pub const fn len(&self) -> usize {
        self.size
    }

    /// 是否为空
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// 清零缓冲区（设备就绪前避免读到脏数据）
    pub fn clear(&mut self) {
        unsafe {
            ptr::write_bytes(self.virt, 0, self.size);
        }
    }
}

impl Deref for DmaBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.virt, self.size) }
    }
}

impl DerefMut for DmaBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt, self.size) }
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        pmm::free_pages(PhysAddr(self.phys), self.order);
    }
}

/// 虚拟地址 → 物理地址（DMA 方向：CPU 侧地址转设备侧地址）
///
/// 只接受直接映射区地址；普通堆地址应先经 VirtAddr::to_phys 判断
pub const fn virt_to_phys(virt: u64) -> Option<u64> {
    match crate::arch::x86_64::memory::VirtAddr(virt).to_phys() {
        Some(p) => Some(p.0),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_reject() {
        assert!(DmaBuf::alloc(0).is_err());
        assert!(DmaBuf::alloc(16 * 1024 * 1024 + 1).is_err());
    }

    #[test]
    fn test_virt_to_phys() {
        // 直接映射区内的地址按偏移换算
        let phys = 0x1000u64;
        let virt = phys + crate::config::PHYSICAL_MEMORY_OFFSET;
        assert_eq!(virt_to_phys(virt), Some(phys));
        // 非直接映射区拒绝
        assert_eq!(virt_to_phys(0x1000), None);
    }
}