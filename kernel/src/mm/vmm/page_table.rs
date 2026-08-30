// ============================================================
// 页表操作通用接口（Page Table）
// ============================================================
// 为内存管理层封装 x86_64 四级页表的基础操作。
//
// 架构复用（低冗余原则）：
// - 底层页表项/映射器实现在 arch/x86_64/paging.rs（PageTable、
//   PageTableEntry、PageTableMapper、PageTableFlags 等）
// - 物理地址/虚拟地址类型在 arch/x86_64/memory.rs
// 本模块只做三件事：
// 1. 重新导出上述底层类型（对外统一入口）
// 2. 定义「抽象权限（MappingFlags）」并转换到 x86 页标志
// 3. 提供访问“当前地址空间”映射器的便捷接口（读 CR3）

// 重新导出（vmm 上层统一从这里引用，避免散落的 import 路径不一致）
pub use crate::arch::x86_64::memory::{Frame, Page, PhysAddr, VirtAddr};
pub use crate::arch::x86_64::paging::{PageTable, PageTableEntry};

use crate::arch::x86_64::paging::{
    FrameAllocator, MapError, PageTableFlags, PageTableMapper, UnmapError,
};
use crate::config::PHYSICAL_MEMORY_OFFSET;

/// 抽象映射权限：与具体架构解耦
///
/// 为什么需要抽象层：VMA/COW/用户进程代码统一用这套权限表达，
/// 翻译成 x86_64 标志只发生在本模块，架构相关细节不外泄。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MappingFlags {
    /// 页存在（一般始终置位）
    pub present: bool,
    /// 可写
    pub writable: bool,
    /// 可执行（false 表示 NX）
    pub executable: bool,
    /// 用户态可访问
    pub user: bool,
    /// 写时复制页（软件标记，使用 AVL 位）
    pub cow: bool,
    /// 全局页（内核映射常驻，避免 TLB 抖动）
    pub global: bool,
}

impl MappingFlags {
    /// 空权限
    pub const fn none() -> Self {
        Self { present: false, writable: false, executable: false, user: false, cow: false, global: false }
    }

    /// 只读映射
    pub const fn read_only() -> Self {
        Self { present: true, writable: false, executable: false, user: false, cow: false, global: false }
    }

    /// 读写映射
    pub const fn read_write() -> Self {
        Self { present: true, writable: true, executable: false, user: false, cow: false, global: false }
    }

    /// 读执行映射
    pub const fn read_execute() -> Self {
        Self { present: true, writable: false, executable: true, user: false, cow: false, global: false }
    }

    /// 用户态读写
    pub const fn user_read_write() -> Self {
        Self { present: true, writable: true, executable: false, user: true, cow: false, global: false }
    }

    /// 标记 COW
    pub const fn to_cow(mut self) -> Self {
        self.cow = true;
        self.writable = false; // COW 页必须只读，靠缺页写时复制
        self
    }

    /// 转换成 x86_64 页表标志
    pub fn to_page_flags(self) -> PageTableFlags {
        let mut f = PageTableFlags::empty();
        if self.present {
            f = f.with(PageTableFlags::PRESENT);
        }
        if self.writable {
            f = f.with(PageTableFlags::WRITABLE);
        }
        if !self.executable {
            f = f.with(PageTableFlags::NO_EXECUTE);
        }
        if self.user {
            f = f.with(PageTableFlags::USER);
        }
        if self.cow {
            f = f.with(PageTableFlags::COW);
        }
        if self.global {
            f = f.with(PageTableFlags::GLOBAL);
        }
        f
    }

    /// 从 x86_64 页表标志还原抽象权限（查询映射时使用）
    pub fn from_page_flags(f: PageTableFlags) -> Self {
        Self {
            present: f.contains(PageTableFlags::PRESENT),
            writable: f.contains(PageTableFlags::WRITABLE),
            executable: !f.contains(PageTableFlags::NO_EXECUTE),
            user: f.contains(PageTableFlags::USER),
            cow: f.contains(PageTableFlags::COW),
            global: f.contains(PageTableFlags::GLOBAL),
        }
    }
}

/// 页表操作错误（转化自底层错误，向上提供统一错误面）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTableError {
    /// 目标页已映射
    AlreadyMapped,
    /// 内存不足（中间级页表分配失败）
    OutOfMemory,
    /// 目标页未映射
    NotMapped,
}

impl From<MapError> for PageTableError {
    fn from(e: MapError) -> Self {
        match e {
            MapError::AlreadyMapped => Self::AlreadyMapped,
            MapError::OutOfMemory => Self::OutOfMemory,
        }
    }
}

impl From<UnmapError> for PageTableError {
    fn from(e: UnmapError) -> Self {
        match e {
            UnmapError::NotMapped => Self::NotMapped,
        }
    }
}

/// 内核页表映射器（对 PageTableMapper 的高层封装）
///
/// 注意：底层的 PageTableMapper 需要 &mut 访问，且内部不并发安全，
/// 上层（vmm::VmmState）会把它放进自旋锁中整体串行化。
pub struct KernelPageMapper {
    inner: PageTableMapper,
}

impl KernelPageMapper {
    /// 获取“当前地址空间”的映射器（读 CR3 得到根页表物理地址）
    ///
    /// 为什么读 CR3 而不是使用一个固定地址：启动期 boot.asm 建立的
    /// 内核页表是“当前”唯一活动的页表；未来进程切换时，每个进程
    /// 地址空间可基于各自的 CR3 构建新映射器。
    pub fn current() -> Self {
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
        }
        // CR3 是 PML4 的物理地址（低 12 位为 PCD/PWT 等标志，需屏蔽）
        let pml4_phys = PhysAddr::new_aligned(cr3);
        Self {
            inner: unsafe { PageTableMapper::new(pml4_phys) },
        }
    }

    /// 基于给定 PML4 物理地址构建（进程地址空间用）
    ///
    /// 调用方必须保证 pml4_phys 指向一个真实有效的 PML4 页表。
    pub fn from_p4(pml4_phys: PhysAddr) -> Self {
        Self {
            inner: unsafe { PageTableMapper::new(pml4_phys) },
        }
    }

    /// 映射一页（页粒度）
    pub fn map_page<A: FrameAllocator>(
        &mut self,
        page: Page,
        frame: Frame,
        flags: MappingFlags,
        allocator: &mut A,
    ) -> Result<(), PageTableError> {
        self.inner
            .map(page, frame, flags.to_page_flags(), allocator)
            .map_err(Into::into)
    }

    /// 取消映射一页，返回被解映射的物理帧
    pub fn unmap_page(&mut self, page: Page) -> Result<Frame, PageTableError> {
        self.inner.unmap(page).map_err(Into::into)
    }

    /// 查询虚拟地址的物理地址（含页内偏移）
    pub fn translate(&self, addr: VirtAddr) -> Option<PhysAddr> {
        self.inner.translate(addr)
    }

    /// 修改一页的权限：拆表 → 改标志 → 重置表项
    ///
    /// 为什么用「读-改-写」而不是底层的直接 set：底层的 set 会覆盖
    /// 地址并强制要求新标志；这里保留原物理地址，只改标志位。
    /// 由于当前架构实现没有暴露「改标志」接口，此处通过 translate
    /// + unmap + map 的组合完成（页内容不受影响）。
    pub fn update_flags<A: FrameAllocator>(
        &mut self,
        page: Page,
        flags: MappingFlags,
        allocator: &mut A,
    ) -> Result<(), PageTableError> {
        let phys = self.inner.translate(page.start).ok_or(PageTableError::NotMapped)?;
        let frame = Frame::containing(phys);
        self.inner.unmap(page).map_err(PageTableError::from)?;
        self.inner
            .map(page, frame, flags.to_page_flags(), allocator)
            .map_err(Into::into)
    }

    /// 查询页表项标志（COW/权限判断用）
    pub fn pte_flags(&self, addr: VirtAddr) -> Option<PageTableFlags> {
        self.inner.pte_flags(addr)
    }

    /// 底层映射器（供高级操作直接用，如 COW 逐页处理）
    pub fn inner(&mut self) -> &mut PageTableMapper {
        &mut self.inner
    }
}

/// 把物理地址换算为直接映射虚拟地址（常见操作的组合）
#[inline]
pub fn phys_to_direct(phys: PhysAddr) -> VirtAddr {
    VirtAddr(phys.0 + PHYSICAL_MEMORY_OFFSET)
}