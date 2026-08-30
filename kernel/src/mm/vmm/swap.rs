// ============================================================
// 交换机制（Swap）
// ============================================================
// 把暂时用不到的内存页“倒”出内存（到交换存储），需要时再换入。
//
// 本阶段说明（为什么这样设计）：
// - 块层/磁盘驱动尚未就绪，因此上层「换出目标」先用内存中的
//   模拟交换存储（MemorySwapStore）：完整走通 slot 分配、页内容
//   读写、登记/注销的全部流程，将来只需把 Store 换成长久化的
//   块设备实现（写磁盘），上层逻辑一行不改（面向接口编程）
// - SwapManager 只负责「哪个页 ↔ 哪个 slot」的登记与内容搬运，
//   不碰页表：真正把 PTE 置为 absent / 重新映射由 vmm 缺页路径做
//   （本模块保持与架构无关，可在宿主环境单测）

use alloc::vec;
use alloc::vec::Vec;

use super::super::heap::slub::PageProvider;

/// 交换存储接口（可换成内存、磁盘、压缩内存等实现）
pub trait SwapStore {
    /// 容量（slot 数）
    fn capacity(&self) -> usize;
    /// 申请一个空闲 slot
    fn alloc_slot(&mut self) -> Option<usize>;
    /// 释放 slot（内容作废）
    fn free_slot(&mut self, slot: usize);
    /// 把一个 4KB 页写进 slot
    fn write_page(&mut self, slot: usize, src: *const u8);
    /// 把一个 4KB 页从 slot 读出
    fn read_page(&mut self, slot: usize, dst: *mut u8);
}

/// 内存版交换存储：用页提供者的页作为 slot 载体
///
/// 为什么还需要页提供者：slot 要能装下 4KB 内容，本身也要占内存；
/// 通过 PageProvider 取页能让「模拟交换」也走真实分配路径，
/// 便于测试换入换出时的内存账目。
pub struct MemorySwapStore<'a> {
    provider: &'a mut dyn PageProvider,
    slots: Vec<Option<usize>>,
}

impl<'a> MemorySwapStore<'a> {
    /// 新建内存交换存储
    pub fn new(capacity: usize, provider: &'a mut dyn PageProvider) -> Self {
        Self {
            provider,
            slots: vec![None; capacity],
        }
    }

    /// 正在使用的 slot 数（统计）
    pub fn used(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

impl SwapStore for MemorySwapStore<'_> {
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn alloc_slot(&mut self) -> Option<usize> {
        // 优先复用已释放的 slot 页面；没有则新取一页
        let slot = self.slots.iter().position(|s| s.is_none())?;
        if self.slots[slot].is_none() {
            let page = self.provider.alloc_page()?;
            self.slots[slot] = Some(page);
        }
        Some(slot)
    }

    fn free_slot(&mut self, slot: usize) {
        if let Some(page) = self.slots.get_mut(slot).and_then(|s| s.take()) {
            self.provider.free_page(page);
        }
    }

    fn write_page(&mut self, slot: usize, src: *const u8) {
        if let Some(page) = self.slots[slot] {
            unsafe { core::ptr::copy_nonoverlapping(src, page as *mut u8, 4096) };
        }
    }

    fn read_page(&mut self, slot: usize, dst: *mut u8) {
        if let Some(page) = self.slots[slot] {
            unsafe { core::ptr::copy_nonoverlapping(page as *const u8, dst, 4096) };
        }
    }
}

/// 交换错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapError {
    /// 交换存储已满
    NoSlot,
    /// 该页未被登记为换出（swap_in 时）
    NotSwapped,
    /// 已登记（重复 swap_out）
    AlreadySwapped,
}

/// 换出登记表条目
#[derive(Clone, Copy)]
struct SwapEntry {
    /// 页基址（虚拟地址，页对齐），作为条目键
    page: usize,
    /// 对应 slot
    slot: u32,
    /// 是否在用
    used: bool,
}

/// 登记表容量（同时换出的页数上限；内核阶段足够）
pub const MAX_SWAPPED_PAGES: usize = 1024;

/// 交换管理器：登记「页 → slot」并搬运内容
pub struct SwapManager {
    entries: Vec<SwapEntry>,
}

impl SwapManager {
    /// 新建管理器
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 已登记换出的页数
    pub fn swapped_pages(&self) -> usize {
        self.entries.len()
    }

    /// 查找某页的登记条目（缺少则 None）
    fn find(&self, page: usize) -> Option<usize> {
        self.entries.iter().position(|e| e.used && e.page == page)
    }

    /// 换出：登记并写内容到 store
    ///
    /// src 指向待换出页的内容（调用方须保证其有效，
    /// 例如通过物理直接映射地址）
    pub fn swap_out(
        &mut self,
        store: &mut dyn SwapStore,
        page: usize,
        src: *const u8,
    ) -> Result<(), SwapError> {
        if self.find(page).is_some() {
            return Err(SwapError::AlreadySwapped);
        }
        let slot = store.alloc_slot().ok_or(SwapError::NoSlot)?;
        store.write_page(slot, src);
        // 登记表容量不足时释放 slot 以保持一致性
        if self.entries.len() >= MAX_SWAPPED_PAGES {
            store.free_slot(slot);
            return Err(SwapError::NoSlot);
        }
        self.entries.push(SwapEntry {
            page,
            slot: slot as u32,
            used: true,
        });
        Ok(())
    }

    /// 换入：读出内容到 dst，注销登记、释放 slot
    pub fn swap_in(
        &mut self,
        store: &mut dyn SwapStore,
        page: usize,
        dst: *mut u8,
    ) -> Result<(), SwapError> {
        let idx = self.find(page).ok_or(SwapError::NotSwapped)?;
        let slot = self.entries[idx].slot as usize;
        store.read_page(slot, dst);
        store.free_slot(slot);
        self.entries.remove(idx);
        Ok(())
    }

    /// 某页是否被登记为换出
    pub fn is_swapped(&self, page: usize) -> bool {
        self.find(page).is_some()
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    // 复用 slub 测试桩：super^3 = 组根（内核 mm / 宿主 crate 根）
    use super::super::super::heap::slub::FakeProvider;

    /// 栈上的 4KB 缓冲区（避免堆对齐问题）
    fn buf() -> [u8; 4096] {
        [0u8; 4096]
    }

    #[test]
    fn test_memory_store_roundtrip() {
        let mut provider = FakeProvider::new(8);
        let mut store = MemorySwapStore::new(4, &mut provider);
        assert_eq!(store.capacity(), 4);
        assert_eq!(store.used(), 0);

        let s0 = store.alloc_slot().expect("slot0");
        let s1 = store.alloc_slot().expect("slot1");
        assert_ne!(s0, s1);

        let mut data = buf();
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        store.write_page(s0, data.as_ptr());
        assert_eq!(store.used(), 2);

        let mut out = buf();
        store.read_page(s0, out.as_mut_ptr());
        assert_eq!(data, out, "写入与读出必须一致");

        // slot 复用：释放后再次分配可拿到同一 slot（页面复用）
        store.free_slot(s1);
        let s2 = store.alloc_slot().expect("reuse slot");
        assert_eq!(s2, s1);
        assert_eq!(store.used(), 2);
    }

    #[test]
    fn test_manager_out_in() {
        let mut provider = FakeProvider::new(8);
        let mut store = MemorySwapStore::new(8, &mut provider);
        let mut mgr = SwapManager::new();

        let page = 0x1000_0000usize;
        let mut data = buf();
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }

        mgr.swap_out(&mut store, page, data.as_ptr()).expect("out");
        assert!(mgr.is_swapped(page));
        assert_eq!(mgr.swapped_pages(), 1);
        // 重复换出被拒绝
        assert_eq!(
            mgr.swap_out(&mut store, page, data.as_ptr()).unwrap_err(),
            SwapError::AlreadySwapped
        );

        let mut back = buf();
        mgr.swap_in(&mut store, page, back.as_mut_ptr()).expect("in");
        assert_eq!(data, back);
        assert!(!mgr.is_swapped(page));
        assert_eq!(mgr.swapped_pages(), 0);
        // 未登记的页换入被拒绝
        assert_eq!(
            mgr.swap_in(&mut store, 0x2000, back.as_mut_ptr()).unwrap_err(),
            SwapError::NotSwapped
        );
    }

    #[test]
    fn test_store_exhaust() {
        let mut provider = FakeProvider::new(2);
        let mut store = MemorySwapStore::new(2, &mut provider);
        let mut mgr = SwapManager::new();
        let data = buf();
        assert!(mgr.swap_out(&mut store, 0x1000, data.as_ptr()).is_ok());
        assert!(mgr.swap_out(&mut store, 0x2000, data.as_ptr()).is_ok());
        // 满：无法换出
        assert_eq!(
            mgr.swap_out(&mut store, 0x3000, data.as_ptr()).unwrap_err(),
            SwapError::NoSlot
        );
        // 释放后再换
        let mut back = buf();
        mgr.swap_in(&mut store, 0x1000, back.as_mut_ptr()).unwrap();
        assert!(mgr.swap_out(&mut store, 0x3000, data.as_ptr()).is_ok());
    }
}
