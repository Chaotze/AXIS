// ============================================================
// CPU 亲和性（cpu_affinity）
// ============================================================
// 任务可运行 CPU 集合的表示与选择（纯逻辑层）。
//
// 为什么需要亲和性：
// - 允许把任务绑定到特定 CPU（缓存亲和、NUMA 局部性、
//   隔离敏感任务），是 Linux sched_setaffinity 的内核侧
//   数据结构；也是调度器选择目标 CPU 的过滤条件
//
// 为什么复用 lib 的 Bitmap 而不是新写位集：
// - "高组件复用率、低重复冗余"：Bitmap 已经过完整
//   测试（置位/清位/查找/迭代），CpuMask 只是它的
//   语义化薄包装（newtype 避免与普通位图混用）
//
// 为什么用 const 泛型 WORDS：
// - CPU 数量编译期可知（如 16/64 核），定长数组内嵌
//   在 Thread 中零堆开销

use super::super::super::lib::collections::bitmap::Bitmap;

/// CPU 亲和掩码（1 bit 对应 1 个 CPU）
///
/// 内嵌 lib 的 Bitmap（已实现 Clone/Debug），本类型随之
/// 可按值复制
#[derive(Debug, Clone)]
pub struct CpuMask<const WORDS: usize> {
    bits: Bitmap,
}

impl<const WORDS: usize> CpuMask<WORDS> {
    /// 空掩码（不绑定任何 CPU = 非法状态，需调用方随后设置）
    pub fn none() -> Self {
        Self {
            bits: Bitmap::new(WORDS * usize::BITS as usize),
        }
    }

    /// 全 CPU 掩码（默认：可在任何 CPU 上运行）
    ///
    /// 为什么默认是全掩码：
    /// - fork 出的任务未指定亲和性时应"到处可跑"，
    ///   由负载均衡决定落点
    pub fn all() -> Self {
        let mut mask = Self::none();
        mask.bits.set_all();
        mask
    }

    /// 设置某 CPU 为可用
    pub fn set(&mut self, cpu: u32) {
        self.bits.set(cpu as usize);
    }

    /// 清除某 CPU
    pub fn clear(&mut self, cpu: u32) {
        self.bits.clear(cpu as usize);
    }

    /// 某 CPU 是否可用
    pub fn is_allowed(&self, cpu: u32) -> bool {
        self.bits.test(cpu as usize)
    }

    /// 是否一个 CPU 都不允许
    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// 可用 CPU 总数
    pub fn count(&self) -> usize {
        self.bits.count_ones()
    }

    /// 选择目标 CPU：
    /// - 优先返回 `prefer` 及其之后最近的可运行 CPU
    ///   （避免所有任务都挤到 0 号核）
    /// - 否则从 0 号核起选第一个可用 CPU
    ///
    /// # 为什么"从 prefer 向后找"而不是永远最低位：
    /// - 均衡地散开任务到各核（配合 load_balance 使用）；
    ///   prefer 通常取"上次所在核"或轮转计数器
    pub fn select_cpu(&self, prefer: Option<u32>) -> Option<u32> {
        if self.is_empty() {
            return None;
        }
        let start = prefer.unwrap_or(0) as usize;
        // 第一段：[start, WORDS*64)
        for cpu in start..WORDS * usize::BITS as usize {
            if self.bits.test(cpu) {
                return Some(cpu as u32);
            }
        }
        // 第二段：[0, start)（回绕，保证 prefer 之前也可能命中）
        for cpu in 0..start {
            if self.bits.test(cpu) {
                return Some(cpu as u32);
            }
        }
        None
    }

    /// 与另一掩码取交集（如"任务允许 ∩ 调度域允许"）
    pub fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::none();
        // Bitmap 未暴露逐字访问写回接口，逐位求交
        // （CPU 数通常 < 64，开销可忽略）
        for cpu in 0..WORDS * usize::BITS as usize {
            if self.bits.test(cpu) && other.bits.test(cpu) {
                result.bits.set(cpu);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_default_all_mask() {
        let mask: CpuMask<1> = CpuMask::all();
        assert!(!mask.is_empty());
        assert!(mask.is_allowed(0));
        assert!(mask.is_allowed(63));
        assert_eq!(mask.count(), 64);
        // 无任何偏好时选 0 号核
        assert_eq!(mask.select_cpu(None), Some(0));
    }

    #[test]
    fn test_bind_and_select() {
        let mut mask: CpuMask<1> = CpuMask::none();
        assert!(mask.is_empty());
        assert_eq!(mask.select_cpu(None), None);

        mask.set(3);
        mask.set(5);
        assert!(mask.is_allowed(3));
        assert!(!mask.is_allowed(4));
        assert_eq!(mask.count(), 2);

        // 从偏好向后找
        assert_eq!(mask.select_cpu(Some(3)), Some(3));
        assert_eq!(mask.select_cpu(Some(4)), Some(5));
        // 偏好越过末尾 → 回绕到 3
        assert_eq!(mask.select_cpu(Some(6)), Some(3));

        mask.clear(3);
        assert!(!mask.is_allowed(3));
        assert_eq!(mask.select_cpu(None), Some(5));
    }

    #[test]
    fn test_intersect() {
        let mut a: CpuMask<1> = CpuMask::none();
        a.set(1);
        a.set(2);
        a.set(3);
        let mut b: CpuMask<1> = CpuMask::none();
        b.set(2);
        b.set(3);
        b.set(4);

        let c = a.intersect(&b);
        assert!(!c.is_allowed(1));
        assert!(c.is_allowed(2));
        assert!(c.is_allowed(3));
        assert!(!c.is_allowed(4));
        assert_eq!(c.count(), 2);
    }
}
