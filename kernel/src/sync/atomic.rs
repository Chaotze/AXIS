// ============================================================
// 原子操作封装
// ============================================================
// 提供便捷的原子操作接口

pub use core::sync::atomic::*;

/// 原子计数器
///
/// 封装常用的原子计数操作
pub struct AtomicCounter {
    value: AtomicUsize,
}

impl AtomicCounter {
    /// 创建新的计数器
    pub const fn new(initial: usize) -> Self {
        Self {
            value: AtomicUsize::new(initial),
        }
    }

    /// 获取当前值
    #[inline]
    pub fn get(&self) -> usize {
        self.value.load(Ordering::Acquire)
    }

    /// 设置值
    #[inline]
    pub fn set(&self, value: usize) {
        self.value.store(value, Ordering::Release);
    }

    /// 增加并返回新值
    #[inline]
    pub fn increment(&self) -> usize {
        self.value.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// 减少并返回新值
    #[inline]
    pub fn decrement(&self) -> usize {
        self.value.fetch_sub(1, Ordering::AcqRel) - 1
    }

    /// 增加指定值
    #[inline]
    pub fn add(&self, value: usize) -> usize {
        self.value.fetch_add(value, Ordering::AcqRel)
    }

    /// 减少指定值
    #[inline]
    pub fn sub(&self, value: usize) -> usize {
        self.value.fetch_sub(value, Ordering::AcqRel)
    }
}
