// ============================================================
// x86_64 页表管理
// ============================================================
// 实现 4 级页表的创建、修改和查询

use super::memory::{PhysAddr, VirtAddr, Frame, Page};
use core::ops::{Index, IndexMut};

/// 页表项标志位
///
/// x86_64 页表项格式（64 位）：
/// - bits 51-12: 物理地址（40 位，支持最多 1TB 物理内存）
/// - bits 11-0: 标志位
/// - bits 63-52: 保留或扩展标志
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableFlags(u64);

impl PageTableFlags {
    /// 存在位：页面是否在内存中
    pub const PRESENT: Self = Self(1 << 0);
    /// 可写位：页面是否可写
    pub const WRITABLE: Self = Self(1 << 1);
    /// 用户位：用户态是否可访问
    pub const USER: Self = Self(1 << 2);
    /// 写穿透：写操作直接写入内存（不使用缓存）
    pub const WRITE_THROUGH: Self = Self(1 << 3);
    /// 禁用缓存
    pub const NO_CACHE: Self = Self(1 << 4);
    /// 访问位：页面是否被访问过（CPU 自动设置）
    pub const ACCESSED: Self = Self(1 << 5);
    /// 脏位：页面是否被写过（CPU 自动设置）
    pub const DIRTY: Self = Self(1 << 6);
    /// 巨页：此表项映射 2MB（PD）或 1GB（PDPT）页面
    pub const HUGE_PAGE: Self = Self(1 << 7);
    /// 全局页：TLB 刷新时不清除此项（需要 CR4.PGE = 1）
    pub const GLOBAL: Self = Self(1 << 8);
    /// 禁止执行：页面不可执行（需要 EFER.NXE = 1）
    pub const NO_EXECUTE: Self = Self(1 << 63);

    /// 创建空标志
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// 检查是否包含某标志
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 添加标志
    #[inline]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// 移除标志
    #[inline]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// 页表项
///
/// 为什么需要封装：
/// - 提供类型安全的页表操作
/// - 隐藏底层位操作细节
/// - 防止创建无效的页表项
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// 创建空表项
    #[inline]
    pub const fn new() -> Self {
        Self(0)
    }

    /// 检查表项是否存在
    #[inline]
    pub const fn is_present(&self) -> bool {
        self.0 & 1 != 0
    }

    /// 获取标志位
    #[inline]
    pub const fn flags(&self) -> PageTableFlags {
        PageTableFlags(self.0 & 0xFFF)
    }

    /// 获取物理地址
    ///
    /// 为什么要屏蔽标志位：
    /// - 页表项的低 12 位是标志位，不是地址的一部分
    /// - 高位可能包含扩展标志，需要清除
    #[inline]
    pub const fn addr(&self) -> PhysAddr {
        PhysAddr(self.0 & 0x000F_FFFF_FFFF_F000)
    }

    /// 设置表项
    ///
    /// 为什么需要原子操作：
    /// - 多核系统中，其他 CPU 可能正在访问此页表
    /// - 非原子写入可能导致读到部分更新的表项
    #[inline]
    pub fn set(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        // 确保地址页对齐
        assert_eq!(addr.0 & 0xFFF, 0, "Address must be page-aligned");

        // 合并地址和标志
        let entry = addr.0 | flags.0;

        // 原子写入
        unsafe {
            core::ptr::write_volatile(&mut self.0, entry);
        }
    }

    /// 清除表项
    #[inline]
    pub fn clear(&mut self) {
        unsafe {
            core::ptr::write_volatile(&mut self.0, 0);
        }
    }

    /// 获取下级页表（如果存在）
    ///
    /// 为什么返回 Option：
    /// - 表项可能未映射（PRESENT = 0）
    /// - 表项可能是巨页（HUGE_PAGE = 1），没有下级页表
    #[inline]
    pub fn next_table(&self) -> Option<&PageTable> {
        if self.is_present() && !self.flags().contains(PageTableFlags::HUGE_PAGE) {
            let addr = self.addr().to_virt();
            Some(unsafe { &*(addr.0 as *const PageTable) })
        } else {
            None
        }
    }

    /// 获取可变的下级页表
    #[inline]
    pub fn next_table_mut(&mut self) -> Option<&mut PageTable> {
        if self.is_present() && !self.flags().contains(PageTableFlags::HUGE_PAGE) {
            let addr = self.addr().to_virt();
            Some(unsafe { &mut *(addr.0 as *mut PageTable) })
        } else {
            None
        }
    }
}

/// 页表
///
/// x86_64 页表结构：
/// - 每个页表包含 512 个表项（64 位 × 512 = 4KB）
/// - 4 级页表：PML4 → PDPT → PD → PT → 物理页
#[repr(align(4096))]
#[repr(C)]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    /// 创建空页表
    #[inline]
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); 512],
        }
    }

    /// 清零页表
    #[inline]
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }

    /// 获取表项
    #[inline]
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    /// 获取可变表项
    #[inline]
    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

impl Index<usize> for PageTable {
    type Output = PageTableEntry;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl IndexMut<usize> for PageTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

/// 页表映射器
///
/// 提供高层页表操作接口，封装底层细节
pub struct PageTableMapper {
    p4_table: &'static mut PageTable,
}

impl PageTableMapper {
    /// 创建映射器
    ///
    /// # Safety
    /// 调用者必须确保 p4_addr 指向有效的 PML4 页表
    pub unsafe fn new(p4_addr: PhysAddr) -> Self {
        unsafe {
            let p4_virt = p4_addr.to_virt();
            let p4_table = &mut *(p4_virt.0 as *mut PageTable);
            Self { p4_table }
        }
    }

    /// 映射页面
    ///
    /// 将虚拟页 page 映射到物理帧 frame
    ///
    /// 为什么需要分配器：
    /// - 可能需要创建中间级页表
    /// - 中间页表需要物理内存
    pub fn map<A>(
        &mut self,
        page: Page,
        frame: Frame,
        flags: PageTableFlags,
        allocator: &mut A,
    ) -> Result<(), MapError>
    where
        A: FrameAllocator,
    {
        let addr = page.start;

        let p4_index = addr.p4_index();
        let p3_index = addr.p3_index();
        let p2_index = addr.p2_index();
        let p1_index = addr.p1_index();

        // 获取或创建 P3
        let p3_entry = &mut self.p4_table[p4_index];
        if !p3_entry.is_present() {
            let p3_frame = allocator.allocate().ok_or(MapError::OutOfMemory)?;
            let p3_table = unsafe {
                let virt = p3_frame.start.to_virt();
                &mut *(virt.0 as *mut PageTable)
            };
            p3_table.zero();
            let p3_flags = PageTableFlags::PRESENT
                .with(PageTableFlags::WRITABLE)
                .with(PageTableFlags::USER);
            p3_entry.set(p3_frame.start, p3_flags);
        }
        let p3_table = unsafe {
            let virt = p3_entry.addr().to_virt();
            &mut *(virt.0 as *mut PageTable)
        };

        // 获取或创建 P2
        let p2_entry = &mut p3_table[p3_index];
        if !p2_entry.is_present() {
            let p2_frame = allocator.allocate().ok_or(MapError::OutOfMemory)?;
            let p2_table = unsafe {
                let virt = p2_frame.start.to_virt();
                &mut *(virt.0 as *mut PageTable)
            };
            p2_table.zero();
            let p2_flags = PageTableFlags::PRESENT
                .with(PageTableFlags::WRITABLE)
                .with(PageTableFlags::USER);
            p2_entry.set(p2_frame.start, p2_flags);
        }
        let p2_table = unsafe {
            let virt = p2_entry.addr().to_virt();
            &mut *(virt.0 as *mut PageTable)
        };

        // 获取或创建 P1
        let p1_entry = &mut p2_table[p2_index];
        if !p1_entry.is_present() {
            let p1_frame = allocator.allocate().ok_or(MapError::OutOfMemory)?;
            let p1_table = unsafe {
                let virt = p1_frame.start.to_virt();
                &mut *(virt.0 as *mut PageTable)
            };
            p1_table.zero();
            let p1_flags = PageTableFlags::PRESENT
                .with(PageTableFlags::WRITABLE)
                .with(PageTableFlags::USER);
            p1_entry.set(p1_frame.start, p1_flags);
        }
        let p1_table = unsafe {
            let virt = p1_entry.addr().to_virt();
            &mut *(virt.0 as *mut PageTable)
        };

        // 设置最终页表项
        let entry = &mut p1_table[p1_index];
        if entry.is_present() {
            return Err(MapError::AlreadyMapped);
        }

        entry.set(frame.start, flags.with(PageTableFlags::PRESENT));

        Ok(())
    }

    /// 取消映射
    pub fn unmap(&mut self, page: Page) -> Result<Frame, UnmapError> {
        let addr = page.start;

        // 遍历页表
        let p4_index = addr.p4_index();
        let p3_index = addr.p3_index();
        let p2_index = addr.p2_index();
        let p1_index = addr.p1_index();

        let p3 = self.p4_table[p4_index].next_table_mut()
            .ok_or(UnmapError::NotMapped)?;
        let p2 = p3[p3_index].next_table_mut()
            .ok_or(UnmapError::NotMapped)?;
        let p1 = p2[p2_index].next_table_mut()
            .ok_or(UnmapError::NotMapped)?;

        // 清除页表项
        let entry = &mut p1[p1_index];
        if !entry.is_present() {
            return Err(UnmapError::NotMapped);
        }

        let frame = Frame::containing(entry.addr());
        entry.clear();

        // 刷新 TLB
        // 为什么需要刷新：CPU 会缓存页表项，不刷新可能访问到旧映射
        unsafe {
            core::arch::asm!("invlpg [{}]", in(reg) addr.0, options(nostack, preserves_flags));
        }

        Ok(frame)
    }

    /// 查询映射
    pub fn translate(&self, addr: VirtAddr) -> Option<PhysAddr> {
        let p4_index = addr.p4_index();
        let p3_index = addr.p3_index();
        let p2_index = addr.p2_index();
        let p1_index = addr.p1_index();

        let p3 = self.p4_table[p4_index].next_table()?;
        let p2 = p3[p3_index].next_table()?;
        let p1 = p2[p2_index].next_table()?;
        let entry = &p1[p1_index];

        if entry.is_present() {
            Some(PhysAddr(entry.addr().0 + addr.page_offset()))
        } else {
            None
        }
    }

    /// 获取或创建下级页表
    ///
    /// 这个方法现在不再使用，保留供参考
    #[allow(dead_code)]
    fn get_or_create_next_table<'a, A>(
        &mut self,
        entry: &'a mut PageTableEntry,
        allocator: &mut A,
    ) -> Result<&'a mut PageTable, MapError>
    where
        A: FrameAllocator,
    {
        if entry.is_present() {
            Ok(entry.next_table_mut().unwrap())
        } else {
            // 分配新页表
            let frame = allocator.allocate()
                .ok_or(MapError::OutOfMemory)?;

            // 初始化为零
            let table = unsafe {
                let virt = frame.start.to_virt();
                &mut *(virt.0 as *mut PageTable)
            };
            table.zero();

            // 设置表项
            let flags = PageTableFlags::PRESENT
                .with(PageTableFlags::WRITABLE)
                .with(PageTableFlags::USER);
            entry.set(frame.start, flags);

            Ok(table)
        }
    }
}

/// 物理页帧分配器 trait
///
/// 为什么需要 trait：
/// - 页表管理不关心帧如何分配，只需要能分配和释放
/// - 可以有多种分配策略（位图、栈、伙伴系统等）
pub trait FrameAllocator {
    fn allocate(&mut self) -> Option<Frame>;
    fn deallocate(&mut self, frame: Frame);
}

/// 映射错误
#[derive(Debug)]
pub enum MapError {
    AlreadyMapped,
    OutOfMemory,
}

/// 取消映射错误
#[derive(Debug)]
pub enum UnmapError {
    NotMapped,
}
