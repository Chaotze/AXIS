// ============================================================
// 页帧元数据（Page Frame Metadata）
// ============================================================
// 描述每一物理页帧的用途与状态，为内存统计、监控与调试提供基础。
//
// 设计要点（为什么这么做）：
// 1. 本模块只有「类型定义」与「聚合统计」逻辑，不持有任何页帧数组
//    —— 页帧数组的存储位置由使用者（内核 pmm 胶水层）决定，
//    可以是静态数组，也可以是从物理内存顶部划分出的字节区。
//    这保证本模块既是纯逻辑、可宿主单测，又不把内存布局写死。
// 2. FrameMeta 刻意保持极小（24 字节内），因为它要为「每一页」都
//    保存一份 —— 128MB 内存就有 32768 帧，体积直接决定内存开销。
// 3. 状态用枚举而不是位标志：语义清晰、可读性强，且未来扩展
//    （如加“页缓存”归属）只需增加枚举变体，不动统计逻辑。

/// 页帧状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameState {
    /// 空闲（可分配）
    Free,
    /// 已分配并属于某个所有者
    Used(FrameOwner),
    /// 预留（不可分配，如内核映像、MMIO、ACPI 区域）
    Reserved,
}

/// 页帧所有者（Used 时有效）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOwner {
    /// 内核代码/数据/堆等一般用途
    Kernel,
    /// 页表页（四级页表的任一中间级）
    PageTable,
    /// SLUB / Slab 缓存页（堆对象载体）
    Slab,
    /// 进程地址空间（用户页、栈、堆等）
    Process,
    /// 设备 DMA / MMIO
    Device,
    /// 交换存储备用页
    Swap,
    /// 未知或未归类
    Unknown,
}

impl Default for FrameOwner {
    fn default() -> Self {
        Self::Unknown
    }
}

/// 页帧元数据
///
/// 为什么把 owner 放进 state 而不是单独字段：
/// - Free/Reserved 状态下 owner 无意义，放进枚举避免无效组合
/// - 匹配一个枚举字段即可完成全部状态判断，代码更简洁
#[derive(Debug, Clone, Copy)]
pub struct FrameMeta {
    /// 帧状态（含所有者信息）
    pub state: FrameState,
    /// 该帧所在的分配阶（伙伴系统 order，0 表示单页）
    pub order: u8,
}

impl FrameMeta {
    /// 新建空闲帧元数据
    #[inline]
    pub const fn free() -> Self {
        Self {
            state: FrameState::Free,
            order: 0,
        }
    }

    /// 新建已分配帧元数据
    #[inline]
    pub const fn used(owner: FrameOwner, order: u8) -> Self {
        Self {
            state: FrameState::Used(owner),
            order,
        }
    }

    /// 新建预留帧元数据
    #[inline]
    pub const fn reserved() -> Self {
        Self {
            state: FrameState::Reserved,
            order: 0,
        }
    }

    /// 是否空闲
    #[inline]
    pub fn is_free(&self) -> bool {
        matches!(self.state, FrameState::Free)
    }

    /// 获取所属物（仅 Used 帧有意义）
    #[inline]
    pub fn owner(&self) -> Option<FrameOwner> {
        match self.state {
            FrameState::Used(o) => Some(o),
            _ => None,
        }
    }
}

/// 帧统计摘要（供“内存统计与监控接口”使用）
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameSummary {
    /// 总帧数
    pub total: usize,
    /// 空闲帧数
    pub free: usize,
    /// 已分配帧数
    pub used: usize,
    /// 预留帧数
    pub reserved: usize,
    /// 按所有者分类的帧数
    pub by_owner: [usize; FRAME_OWNER_COUNT],
}

/// 所有者种类数（用于定长统计数组）
pub const FRAME_OWNER_COUNT: usize = 7;

/// 将所有者映射为统计数组下标
#[inline]
pub fn owner_index(owner: FrameOwner) -> usize {
    match owner {
        FrameOwner::Kernel => 0,
        FrameOwner::PageTable => 1,
        FrameOwner::Slab => 2,
        FrameOwner::Process => 3,
        FrameOwner::Device => 4,
        FrameOwner::Swap => 5,
        FrameOwner::Unknown => 6,
    }
}

impl FrameSummary {
    /// 从帧元数据切片汇总统计
    ///
    /// 为什么单独提供汇总函数而不是内嵌在分配器里：
    /// 监控接口可以按需（如每 10 秒）扫描一遍，避免每次分配/释放
    /// 都记账的开销；两种策略可以并存。
    pub fn summarize(metas: &[FrameMeta]) -> Self {
        let mut s = Self::default();
        s.total = metas.len();
        for m in metas {
            match m.state {
                FrameState::Free => s.free += 1,
                FrameState::Used(o) => {
                    s.used += 1;
                    s.by_owner[owner_index(o)] += 1;
                }
                FrameState::Reserved => s.reserved += 1,
            }
        }
        s
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use std::prelude::v1::*;

    #[test]
    fn test_summarize_counts() {
        let metas = vec![
            FrameMeta::free(),
            FrameMeta::used(FrameOwner::Kernel, 0),
            FrameMeta::used(FrameOwner::Slab, 1),
            FrameMeta::reserved(),
        ];
        let s = FrameSummary::summarize(&metas);
        assert_eq!(s.total, 4);
        assert_eq!(s.free, 1);
        assert_eq!(s.used, 2);
        assert_eq!(s.reserved, 1);
        assert_eq!(s.by_owner[owner_index(FrameOwner::Kernel)], 1);
        assert_eq!(s.by_owner[owner_index(FrameOwner::Slab)], 1);

        assert!(metas[0].is_free());
        assert_eq!(metas[1].owner(), Some(FrameOwner::Kernel));
    }
}
