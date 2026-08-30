// ============================================================
// 地址工具（Address Utilities）
// ============================================================
// 物理/虚拟地址转换与内存区描述。为避免重复定义，物理地址、虚拟
// 地址、页/帧等基础类型直接复用 arch/x86_64/memory.rs 的实现，
// 本模块只补充「内存区描述」这类跨模块共用的工具。
//
// 设计要点（为什么这么做）：
// - 单一事实来源：PhysAddr/VirtAddr 的换算逻辑只存在于 arch 层，
//   本模块若再写一遍会引入两套可能不一致的转换（低冗余原则）
// - MemoryRegion 供引导内存映射描述使用：从 bootloader/固件拿到
//   的可用内存列表，统一表示为「起始地址 + 结束地址 + 类型」

pub use crate::arch::x86_64::memory::{Frame, Page, PhysAddr, VirtAddr};
pub use crate::config::PAGE_SIZE;

/// 内存区类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// 可用内存（可纳入伙伴系统）
    Usable,
    /// 预留内存（内核映像、MMIO、ACPI 表等）
    Reserved,
    /// 设备内存（不可作为通用页分配）
    Mmio,
}

/// 一段连续内存区 [start, end)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    /// 起始物理地址
    pub start: u64,
    /// 结束物理地址（不含）
    pub end: u64,
    /// 区域类型
    pub ty: MemoryRegionType,
}

impl MemoryRegion {
    /// 创建内存区（自动纠正 start > end 的脏数据）
    pub const fn new(start: u64, end: u64, ty: MemoryRegionType) -> Self {
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        Self { start: s, end: e, ty }
    }

    /// 区长度（字节）
    #[inline]
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// 区域内页帧数量（按页大小对齐后的页数）
    #[inline]
    pub fn page_count(&self, page_size: u64) -> u64 {
        let s = align_up(self.start, page_size);
        if s >= self.end {
            0
        } else {
            (self.end - s) / page_size
        }
    }

    /// 与另一区域求交集（不重叠则返回空区）
    pub const fn intersect(&self, other: &MemoryRegion) -> MemoryRegion {
        let start = if self.start > other.start { self.start } else { other.start };
        let end = if self.end < other.end { self.end } else { other.end };
        MemoryRegion { start, end, ty: self.ty }
    }
}

/// 向上对齐到 page_size 的整数倍
#[inline]
pub const fn align_up(addr: u64, page_size: u64) -> u64 {
    if page_size == 0 {
        return addr;
    }
    let mask = page_size - 1;
    (addr + mask) & !mask
}

/// 向下对齐到 page_size 的整数倍
#[inline]
pub const fn align_down(addr: u64, page_size: u64) -> u64 {
    if page_size == 0 {
        return addr;
    }
    addr & !(page_size - 1)
}

/// 从物理地址取页号
#[inline]
pub const fn phys_to_pfn(addr: u64, page_size: u64) -> u64 {
    addr / page_size
}

/// 从页号取物理地址
#[inline]
pub const fn pfn_to_phys(pfn: u64, page_size: u64) -> u64 {
    pfn * page_size
}
