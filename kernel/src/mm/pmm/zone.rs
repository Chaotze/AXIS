// ============================================================
// 内存区域（Zone）管理
// ============================================================
// 把物理内存按地址范围划分为若干区域（DMA / Normal / 未来 High），
// 每个区域独立跑一个伙伴系统 + 水位标记，形成「区域 → 伙伴 → 页」的
// 分层。这借鉴了 Linux 的 ZONE 设计：
//
// 为什么要分区域：
// - 老式 ISA 设备只能做 16MB 以下内存的 DMA，因此必须保留一块
//   低地址区域（DMA Zone），驱动分配时显式要求这块区域的页
// - 区域隔离还能避免「低地址内存被长期占用而耗尽」——高内存需求
//   从 Normal 区分配，不会挤占 DMA 区的稀缺低址页
// - 每个区域有独立水位，便于按区域观察内存压力
//
// 本模块是纯逻辑：区域的页帧范围（start_pfn/end_pfn）、页大小由
// 调用方给出，区域只负责在该范围内分配/释放，不涉及物理内存映射。

use super::buddy::BuddyAllocator;
use super::watermark::{PressureCounters, Watermark, WatermarkLevel};

/// 区域类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    /// DMA 区：物理地址 < DMA_LIMIT，供老式 DMA 设备使用
    Dma,
    /// Normal 区：常规内存，内核与进程页的主要来源
    Normal,
    /// High 区（预留）：x86_32 时代的高端内存区，x86_64 下无意义
    High,
}

impl ZoneType {
    /// 区域名（调试与统计输出）
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dma => "DMA",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }
}

/// 分配标志（gfp_flags 的精简子集）
///
/// 为什么用位集合结构而不是 bool 参数：
/// - 调用方可以组合语义（如「DMA 区 + 允许回收」）
/// - 未来扩展（GFP_ATOMIC、GFP_NOFS…）只需新增常量位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GfpFlags(u8);

impl GfpFlags {
    /// 空标志：普通内核分配
    pub const NONE: Self = Self(0);
    /// 需要在 DMA 区分配（驱动专用）
    pub const DMA: Self = Self(1 << 0);
    /// 允许触发回收（慢路径；本里程碑仅记录计数，不真正回收）
    pub const MAY_RECLAIM: Self = Self(1 << 1);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// 内存区域
///
/// 区域负责把「伙伴系统返回的叶块偏移」换算成「物理页号 pfn」，
/// 并向调用方隐藏伙伴系统的实现细节。
pub struct Zone {
    /// 区域类型
    ty: ZoneType,
    /// 区域起始物理页号（含）
    start_pfn: usize,
    /// 区域结束物理页号（不含）
    end_pfn: usize,
    /// 伙伴系统
    buddy: BuddyAllocator,
    /// 水位
    watermark: Watermark,
    /// 压力计数器
    pressure: PressureCounters,
    /// 是否已初始化完成
    ready: bool,
}

impl Zone {
    /// 未初始化占位（用于内核仿静态占位）
    pub const fn uninit() -> Self {
        Self {
            ty: ZoneType::Normal,
            start_pfn: 0,
            end_pfn: 0,
            buddy: BuddyAllocator::uninit(),
            watermark: Watermark::new(0),
            pressure: PressureCounters::empty(),
            ready: false,
        }
    }

    /// 从（start_pfn, end_pfn）区间与一段页面字节区构建区域
    ///
    /// # Safety
    /// - `arena` 必须来自 [`super::buddy::BuddyAllocator::needed_bytes`]
    ///   计算所得的、8 字节对齐的独立字节区
    /// - 调用方须保证区域页数与字节区容量匹配
    pub unsafe fn from_arena(
        ty: ZoneType,
        start_pfn: usize,
        end_pfn: usize,
        arena: &'static mut [u8],
    ) -> Self {
        let pages = end_pfn - start_pfn;
        let buddy = unsafe { BuddyAllocator::from_bytes(pages, arena) };
        // page_size 现阶段恒为 4096（x86_64 标准页），pfn 换算在
        // 胶水层完成；若未来引入大页支持，再在此处显式记录
        Self {
            ty,
            start_pfn,
            end_pfn,
            buddy,
            watermark: Watermark::new(pages),
            pressure: PressureCounters::empty(),
            ready: false,
        }
    }

    /// 预留页（必须在该区域 [finalize] 之前调用）
    pub fn reserve(&mut self, start_pfn: usize, pages: usize) {
        debug_assert!(start_pfn >= self.start_pfn);
        debug_assert!(start_pfn + pages <= self.end_pfn);
        let leaf = start_pfn - self.start_pfn;
        // 只支持叶块级预留（页粒度）；上层映像预留一般也只需要页粒度
        for i in 0..pages {
            self.buddy.mark_reserved(0, leaf + i);
        }
    }

    /// 完成伙伴系统挂链
    pub fn finalize(&mut self) {
        self.buddy.finalize();
        self.ready = true;
    }

    /// 区域类型
    #[inline]
    pub const fn ty(&self) -> ZoneType {
        self.ty
    }

    /// 区域起始物理页号
    #[inline]
    pub const fn start_pfn(&self) -> usize {
        self.start_pfn
    }

    /// 区域结束物理页号（不含）
    #[inline]
    pub const fn end_pfn(&self) -> usize {
        self.end_pfn
    }

    /// 区域页数
    #[inline]
    pub const fn total_pages(&self) -> usize {
        self.end_pfn - self.start_pfn
    }

    /// 空闲页数
    #[inline]
    pub fn free_pages(&self) -> usize {
        self.buddy.free_pages()
    }

    /// 已分配页数
    #[inline]
    pub fn used_pages(&self) -> usize {
        self.buddy.used_pages()
    }

    /// 预留页数
    #[inline]
    pub fn reserved_pages(&self) -> usize {
        self.buddy.reserved_pages()
    }

    /// 最大分配阶
    #[inline]
    pub fn max_order(&self) -> usize {
        self.buddy.max_order()
    }

    /// 水位
    #[inline]
    pub const fn watermark(&self) -> &Watermark {
        &self.watermark
    }

    /// 压力计数器（只读引用，供统计输出）
    #[inline]
    pub const fn pressure(&self) -> &PressureCounters {
        &self.pressure
    }

    /// 压力计数器（可变引用，供分配路径记账）
    #[inline]
    pub fn pressure_mut(&mut self) -> &mut PressureCounters {
        &mut self.pressure
    }

    /// 是否已初始化
    #[inline]
    pub const fn is_ready(&self) -> bool {
        self.ready
    }

    /// 尝试分配 2^order 页，返回物理页号（失败返回 None）
    ///
    /// 分配策略：
    /// 1. 先检查水位：低于 min 时按「硬分配」处理——本里程碑
    ///    不实现后台回收，故低于 min 仍然尽力分配，但记录压力计数，
    ///    让监控接口能观察到水位告警
    /// 2. 委托伙伴系统取块，成功把叶块偏移换算为 pfn
    pub fn alloc(&mut self, order: usize, flags: GfpFlags) -> Option<usize> {
        if !self.ready {
            self.pressure.note_oom();
            return None;
        }
        self.pressure.note_alloc();

        let free = self.buddy.free_pages();
        if self.watermark.below(free, WatermarkLevel::Min) {
            self.pressure.note_water_fail();
            if !flags.contains(GfpFlags::MAY_RECLAIM) {
                // 无回收能力且低于 min：允许失败，保证关键路径余量
                self.pressure.note_oom();
                return None;
            }
            self.pressure.note_reclaim();
        }

        let leaf = match self.buddy.alloc(order) {
            Some(l) => l,
            None => {
                // 伙伴系统彻底耗尽：累计一次 OOM（供监控）
                self.pressure.note_oom();
                return None;
            }
        };
        let pfn = self.start_pfn + leaf;
        Some(pfn)
    }

    /// 释放 2^order 页（pfn 必须来自本区域）
    pub fn free(&mut self, pfn: usize, order: usize) {
        debug_assert!(pfn >= self.start_pfn && pfn + order_pages(order) <= self.end_pfn);
        let leaf = pfn - self.start_pfn;
        self.buddy.free(order, leaf);
        self.pressure.note_free();
    }

    /// 该区域当前水位档位（统计输出用）
    #[inline]
    pub fn level(&self) -> WatermarkLevel {
        self.watermark.level_of(self.buddy.free_pages())
    }
}

use super::buddy::order_pages;

// ---------- 宿主单元测试（通过 mmtest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use std::prelude::v1::*;

    fn make_zone(ty: ZoneType, start_pfn: usize, pages: usize) -> Zone {
        let need = BuddyAllocator::needed_bytes(pages);
        let mut arena = vec![0u8; need + 8];
        let p = arena.as_mut_ptr() as usize;
        let aligned = (p + 7) & !7;
        let bytes =
            unsafe { core::slice::from_raw_parts_mut(aligned as *mut u8, need) };
        core::mem::forget(arena);
        let mut z = unsafe { Zone::from_arena(ty, start_pfn, start_pfn + pages, bytes) };
        z.finalize();
        z
    }

    #[test]
    fn test_alloc_free_roundtrip() {
        let mut z = make_zone(ZoneType::Normal, 100, 64);
        assert_eq!(z.total_pages(), 64);
        assert_eq!(z.free_pages(), 64);

        let pfn = z.alloc(0, GfpFlags::NONE).expect("alloc");
        assert_eq!(pfn, 100, "应从区域起始页开始分配");
        assert_eq!(z.free_pages(), 63);
        z.free(pfn, 0);
        assert_eq!(z.free_pages(), 64);
        assert_eq!(z.used_pages(), 0);
    }

    #[test]
    fn test_dma_zone_low_address() {
        // DMA 区位于低地址（如 < 16MB），验证 pfn 落在区域内
        let mut z = make_zone(ZoneType::Dma, 0, 512);
        let dma_limit = 4096; // 16MB / 4KB
        let first = z.alloc(3, GfpFlags::DMA).expect("dma alloc");
        assert!(first < dma_limit);
        assert_eq!(z.ty(), ZoneType::Dma);
        assert_eq!(z.ty().name(), "DMA");
    }

    #[test]
    fn test_exhaust_zone() {
        let mut z = make_zone(ZoneType::Normal, 0, 8);
        let mut pfns = vec![];
        while let Some(p) = z.alloc(0, GfpFlags::NONE) {
            pfns.push(p);
        }
        assert_eq!(pfns.len(), 8);
        assert_eq!(z.alloc(1, GfpFlags::NONE), None);
        for p in pfns {
            z.free(p, 0);
        }
        assert_eq!(z.free_pages(), 8);
    }

    #[test]
    fn test_watermark_fail_path() {
        let mut z = make_zone(ZoneType::Normal, 0, 1280);
        // 塞满大部分内存（到 min 以下），验证 MAY_RECLAIM 路径计数
        let mut pfns = vec![];
        while let Some(p) = z.alloc(0, GfpFlags::MAY_RECLAIM) {
            pfns.push(p);
        }
        assert!(z.pressure().reclaim_tripped > 0);
        assert!(z.pressure().water_failures > 0);
        // 清理
        for p in pfns {
            z.free(p, 0);
        }
        assert_eq!(z.free_pages(), 1280);
    }

    #[test]
    fn test_reserve_in_zone() {
        let mut z = make_zone(ZoneType::Normal, 0, 64);
        z.reserve(8, 8);
        z.finalize();
        assert_eq!(z.reserved_pages(), 8);
        let mut got = vec![];
        while let Some(p) = z.alloc(0, GfpFlags::NONE) {
            got.push(p);
        }
        assert_eq!(got.len(), 56);
        for p in got {
            assert!(!(8..16).contains(&p));
        }
    }
}