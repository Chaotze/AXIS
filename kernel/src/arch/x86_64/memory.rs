// ============================================================
// x86_64 内存布局和地址转换
// ============================================================
// 提供内存地址相关的常量和工具函数

use crate::config::{PHYSICAL_MEMORY_OFFSET, PAGE_SIZE};

/// 物理地址
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    /// 创建物理地址
    ///
    /// 为什么需要对齐检查：
    /// - 页表项要求地址按页对齐
    /// - 某些硬件寄存器要求地址对齐
    #[inline]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// 创建页对齐的物理地址
    #[inline]
    pub const fn new_aligned(addr: u64) -> Self {
        Self(addr & !((PAGE_SIZE as u64) - 1))
    }

    /// 转换为虚拟地址（通过物理内存映射区）
    ///
    /// 为什么需要物理内存映射：
    /// - 内核需要访问所有物理内存（如修改页表、访问设备内存）
    /// - 直接映射避免频繁的临时映射开销
    #[inline]
    pub const fn to_virt(self) -> VirtAddr {
        VirtAddr(self.0 + PHYSICAL_MEMORY_OFFSET)
    }

    /// 获取页号
    #[inline]
    pub const fn page_number(self) -> u64 {
        self.0 / PAGE_SIZE as u64
    }

    /// 页内偏移
    #[inline]
    pub const fn page_offset(self) -> u64 {
        self.0 & ((PAGE_SIZE as u64) - 1)
    }
}

/// 虚拟地址
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// 创建虚拟地址
    ///
    /// x86_64 规范地址：
    /// - 只有低 48 位有效（当前实现）
    /// - 第 47 位必须符号扩展到高 16 位
    /// - 即：0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF（用户空间）
    /// -   0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF（内核空间）
    ///
    /// 为什么需要规范地址：
    /// - CPU 在页表转换时会检查地址是否规范
    /// - 非规范地址会触发 #GP 异常
    #[inline]
    pub const fn new(addr: u64) -> Self {
        Self(Self::canonicalize(addr))
    }

    /// 规范化地址
    #[inline]
    const fn canonicalize(addr: u64) -> u64 {
        // 符号扩展第 47 位
        ((addr << 16) as i64 >> 16) as u64
    }

    /// 创建页对齐的虚拟地址
    #[inline]
    pub const fn new_aligned(addr: u64) -> Self {
        Self::new(addr & !((PAGE_SIZE as u64) - 1))
    }

    /// 转换为物理地址（如果在物理内存映射区）
    ///
    /// 为什么可能失败：
    /// - 不是所有虚拟地址都是直接映射的物理内存
    /// - 需要检查地址是否在映射区范围内
    #[inline]
    pub const fn to_phys(self) -> Option<PhysAddr> {
        if self.0 >= PHYSICAL_MEMORY_OFFSET {
            Some(PhysAddr(self.0 - PHYSICAL_MEMORY_OFFSET))
        } else {
            None
        }
    }

    /// 获取页号
    #[inline]
    pub const fn page_number(self) -> u64 {
        self.0 / PAGE_SIZE as u64
    }

    /// 页内偏移
    #[inline]
    pub const fn page_offset(self) -> u64 {
        self.0 & ((PAGE_SIZE as u64) - 1)
    }

    /// 获取页表索引
    ///
    /// x86_64 4 级页表：
    /// - PML4: bits 47-39 (9 bits)
    /// - PDPT: bits 38-30 (9 bits)
    /// - PD:   bits 29-21 (9 bits)
    /// - PT:   bits 20-12 (9 bits)
    /// - Offset: bits 11-0 (12 bits)
    #[inline]
    pub const fn p4_index(self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    #[inline]
    pub const fn p3_index(self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    #[inline]
    pub const fn p2_index(self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    #[inline]
    pub const fn p1_index(self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }
}

/// 页面
#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub start: VirtAddr,
}

impl Page {
    /// 包含给定地址的页面
    #[inline]
    pub const fn containing(addr: VirtAddr) -> Self {
        Self {
            start: VirtAddr::new_aligned(addr.0),
        }
    }

    /// 页面范围
    #[inline]
    pub const fn range(start: VirtAddr, end: VirtAddr) -> PageRange {
        PageRange {
            start: Self::containing(start),
            end: Self::containing(end),
        }
    }
}

/// 页面范围
#[derive(Debug, Clone, Copy)]
pub struct PageRange {
    pub start: Page,
    pub end: Page,
}

impl PageRange {
    /// 页面数量
    #[inline]
    pub const fn count(&self) -> usize {
        ((self.end.start.0 - self.start.start.0) / PAGE_SIZE as u64) as usize
    }
}

/// 物理页帧
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub start: PhysAddr,
}

impl Frame {
    /// 包含给定地址的页帧
    #[inline]
    pub const fn containing(addr: PhysAddr) -> Self {
        Self {
            start: PhysAddr::new_aligned(addr.0),
        }
    }
}
