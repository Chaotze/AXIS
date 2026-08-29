// ============================================================
// 页面水位标记（Watermarks）
// ============================================================
// 用「空闲页数量」把内存压力量化为 min / low / high 三档水位，
// 供分配路径判断是否需要回收内存、以及何时可以放开回收。
//
// 参照 Linux 的设计思路：
// - min：硬边界。低于 min 时正常分配应进入慢路径（回收）甚至失败，
//   因为必须为关键路径保留最后一点内存。
// - low：软边界。低于 low 时后台回收（kswapd 等）应开始工作。
// - high：安全线。高于 high 表示内存充足，无需任何回收动作。
//
// 为什么采用「空闲页数 / 128」作为基准（而非固定值）：
// - 内存越大，保留给回收机制的余量也应越大，比例化的阈值能自适应
// - 128 的比例源自 Linux 默认的 watermark_scale_factor 语义，
//   经验上足以应对多数尖峰分配

/// 水位档位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkLevel {
    /// 最低水位：低于此水位分配应报压力（甚至失败）
    Min,
    /// 低水位：低于此水位应启动后台回收
    Low,
    /// 高水位：低于此水位可停止回收
    High,
}

impl WatermarkLevel {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

/// 水位计算与判定
#[derive(Debug, Clone, Copy)]
pub struct Watermark {
    /// 区域总页数（计算阈值的基础）
    total_pages: usize,
    /// 最小水位（页）
    min: usize,
    /// 低水位（页）
    low: usize,
    /// 高水位（页）
    high: usize,
}

impl Watermark {
    /// 根据区域总页数计算三档水位
    ///
    /// 为什么阈值至少为 1：即使极小区域也必须保留至少 1 页用于
    /// 回收等关键路径，避免除零/空区域的退化情况。
    pub const fn new(total_pages: usize) -> Self {
        let min = if total_pages / 128 >= 1 { total_pages / 128 } else { 1 };
        let low = if min * 2 >= 2 { min * 2 } else { 2 };
        let high = if min * 3 >= 3 { min * 3 } else { 3 };
        Self {
            total_pages,
            min,
            low,
            high,
        }
    }

    /// 当前空闲页数是否低于指定水位
    #[inline]
    pub fn below(&self, free_pages: usize, level: WatermarkLevel) -> bool {
        match level {
            WatermarkLevel::Min => free_pages < self.min,
            WatermarkLevel::Low => free_pages < self.low,
            WatermarkLevel::High => free_pages < self.high,
        }
    }

    /// 空闲页数处于哪一档（供统计与监控输出）
    #[inline]
    pub fn level_of(&self, free_pages: usize) -> WatermarkLevel {
        if free_pages >= self.high {
            WatermarkLevel::High
        } else if free_pages >= self.low {
            WatermarkLevel::Low
        } else if free_pages >= self.min {
            WatermarkLevel::Min
        } else {
            WatermarkLevel::Min
        }
    }

    /// 最小水位（页）
    #[inline]
    pub const fn min(&self) -> usize {
        self.min
    }

    /// 低水位（页）
    #[inline]
    pub const fn low(&self) -> usize {
        self.low
    }

    /// 高水位（页）
    #[inline]
    pub const fn high(&self) -> usize {
        self.high
    }

    /// 区域总页数
    #[inline]
    pub const fn total_pages(&self) -> usize {
        self.total_pages
    }
}

/// 内存压力计数器（为监控接口提供分配质量指标）
///
/// 为什么单独放一个结构：分配/释放是高频路径，计数器用普通 usize
/// 自增即可；只有在读取统计时才会一次性合并输出，避免在路径上
/// 引入原子操作的额外开销（本内核为单核起步，多核时再原子化）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PressureCounters {
    /// 总分配请求次数
    pub alloc_requests: u64,
    /// 总释放次数
    pub free_requests: u64,
    /// 因水位不足而失败的次数（OOM 附近）
    pub water_failures: u64,
    /// 因空间耗尽而失败的次数（彻底无内存）
    pub oom: u64,
    /// 回收被触发的次数（低于 low 水位）
    pub reclaim_tripped: u64,
}

impl PressureCounters {
    /// 常量空计数器（供 const 构造）
    pub const fn empty() -> Self {
        Self {
            alloc_requests: 0,
            free_requests: 0,
            water_failures: 0,
            oom: 0,
            reclaim_tripped: 0,
        }
    }

    /// 记录一次成功分配
    #[inline]
    pub fn note_alloc(&mut self) {
        self.alloc_requests += 1;
    }

    /// 记录一次释放
    #[inline]
    pub fn note_free(&mut self) {
        self.free_requests += 1;
    }

    /// 记录一次水位失败
    #[inline]
    pub fn note_water_fail(&mut self) {
        self.water_failures += 1;
    }

    /// 记录一次 OOM
    #[inline]
    pub fn note_oom(&mut self) {
        self.oom += 1;
    }

    /// 记录一次回收触发
    #[inline]
    pub fn note_reclaim(&mut self) {
        self.reclaim_tripped += 1;
    }
}

// ---------- 宿主单元测试（通过 mmtest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    #[test]
    fn test_thresholds_monotonic() {
        for pages in [1usize, 10, 100, 1024, 32768, 1_000_000] {
            let w = Watermark::new(pages);
            assert!(w.min() >= 1);
            assert!(w.min() <= w.low());
            assert!(w.low() <= w.high());
            assert_eq!(w.total_pages(), pages);
        }
    }

    #[test]
    fn test_scale_factor() {
        // 100 页区域：min = max(100/128, 1) = 1, low = 2, high = 3
        let w = Watermark::new(100);
        assert_eq!(w.min(), 1);
        assert_eq!(w.low(), 2);
        assert_eq!(w.high(), 3);
        // 1280 页区域：min = 10
        let w2 = Watermark::new(1280);
        assert_eq!(w2.min(), 10);
        assert_eq!(w2.low(), 20);
        assert_eq!(w2.high(), 30);
    }

    #[test]
    fn test_below_and_level() {
        let w = Watermark::new(1280);
        assert!(w.below(9, WatermarkLevel::Min));
        assert!(!w.below(10, WatermarkLevel::Min));
        assert!(w.below(19, WatermarkLevel::Low));
        assert!(!w.below(20, WatermarkLevel::Low));
        assert!(w.below(29, WatermarkLevel::High));
        assert!(!w.below(30, WatermarkLevel::High));
        assert_eq!(w.level_of(100), WatermarkLevel::High);
        assert_eq!(w.level_of(25), WatermarkLevel::Low);
        assert_eq!(w.level_of(5), WatermarkLevel::Min);
    }

    #[test]
    fn test_pressure_counters() {
        let mut c = PressureCounters::default();
        c.note_alloc();
        c.note_alloc();
        c.note_free();
        c.note_oom();
        c.note_reclaim();
        assert_eq!(c.alloc_requests, 2);
        assert_eq!(c.free_requests, 1);
        assert_eq!(c.oom, 1);
        assert_eq!(c.reclaim_tripped, 1);
    }
}