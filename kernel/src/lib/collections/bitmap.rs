// ============================================================
// 定长位图
// ============================================================
// 用 1 bit 表示 1 个对象占用状态的紧凑布尔数组。
//
// 为什么需要位图：
// - 伙伴系统/页帧分配器用 1 bit 记录 1 个物理页是否空闲
// - PID 分配器用 1 bit 记录 1 个进程号是否被占用
// - 文件系统用位图记录块的空闲情况
// 位图是"百万级对象状态跟踪"场景下空间效率最优的结构
//
// 为什么设计为定长（const 泛型 WORDS）：
// - 内核启动早期没有动态分配器，定长数组直接内嵌在
//   使用方的结构体中，零堆开销、零运行时初始化
// - 分配器落地后，需要动态容量时再以本结构为基础扩展，
//   核心位操作逻辑（见 bit.rs）无需改动
//
// 为什么本模块只做索引换算、位操作全部委托 bit.rs：
// - 位操作的边界处理（位移越界、宽度限制）集中在一处，
//   符合"高复用率、低重复冗余"的要求

use super::super::bit::{self, BitIter};

/// 定长位图，`WORDS` 为机器字数，总位数 = WORDS × 64
pub struct Bitmap<const WORDS: usize> {
    /// 位存储：从 words[0] 的最低位开始线性编号
    bits: [usize; WORDS],
}

impl<const WORDS: usize> Bitmap<WORDS> {
    /// 创建全零位图
    ///
    /// # 为什么是 const fn：
    /// - 位图常作为静态结构体的字段，const 构造使静态
    ///   初始化在编译期完成（.data 段直接写入，无需运行代码）
    pub const fn new() -> Self {
        Self { bits: [0; WORDS] }
    }

    /// 总位数
    #[inline]
    pub const fn len(&self) -> usize {
        WORDS * usize::BITS as usize
    }

    /// 是否一个位都没有置位
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&word| word == 0)
    }

    /// 置位
    ///
    /// # 为什么越界只做 debug_assert：
    /// - 与 bit.rs 一致，发布版保持零开销；越界属于调用方
    ///   的编程错误，由调试构建负责暴露
    #[inline]
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::set_bit(&mut self.bits[word], bit);
    }

    /// 清除位
    #[inline]
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::clear_bit(&mut self.bits[word], bit);
    }

    /// 翻转位
    #[inline]
    pub fn flip(&mut self, index: usize) {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::flip_bit(&mut self.bits[word], bit);
    }

    /// 测试位
    #[inline]
    pub fn test(&self, index: usize) -> bool {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::test_bit(self.bits[word], bit)
    }

    /// 全部置位
    pub fn set_all(&mut self) {
        for word in self.bits.iter_mut() {
            *word = usize::MAX;
        }
    }

    /// 全部清零
    pub fn clear_all(&mut self) {
        for word in self.bits.iter_mut() {
            *word = 0;
        }
    }

    /// 从 0 号位开始查找第一个空闲（0）位
    ///
    /// # 为什么是全位图范围而不是给定起点：
    /// - 全范围查找是分配器的第一需求；带起点的查找
    ///   可以在上层对切片调用同一逻辑实现
    pub fn find_first_zero(&self) -> Option<usize> {
        for (word_index, &word) in self.bits.iter().enumerate() {
            // 最后一个字可能不满 64 位，宽度按实际计算
            let width = last_word_width::<WORDS>(word_index);
            if let Some(bit) = bit::find_first_zero(word, width) {
                return Some(word_index * usize::BITS as usize + bit as usize);
            }
        }
        None
    }

    /// 从 0 号位开始查找第一个占用（1）位
    pub fn find_first_set(&self) -> Option<usize> {
        for (word_index, &word) in self.bits.iter().enumerate() {
            if let Some(bit) = bit::find_first_set(word) {
                return Some(word_index * usize::BITS as usize + bit as usize);
            }
        }
        None
    }

    /// 统计置位的总数
    pub fn count_ones(&self) -> usize {
        self.bits.iter().map(|&word| word.count_ones() as usize).sum()
    }

    /// 按升序迭代所有置位的位号（复用 bit.rs 的 BitIter）
    pub fn iter_ones(&self) -> BitIter<'_> {
        BitIter::new(&self.bits)
    }

    /// 暴露底层机器字切片（供需要直接操作内存的调用方，
    /// 如整体清零/写入设备寄存器）
    #[inline]
    pub fn words(&self) -> &[usize] {
        &self.bits
    }
}

/// 把全局位号拆分为（字下标，字内位号）
#[inline]
fn split_index(index: usize) -> (usize, u8) {
    (
        index / usize::BITS as usize,
        (index % usize::BITS as usize) as u8,
    )
}

/// 计算第 `word_index` 个字的有效宽度
///
/// # 为什么需要这个函数：
/// - 总位数 = WORDS × 64，最后一个字的高位可能超出总位数，
///   查找空闲位时必须把越界高位排除，否则会返回"幻影位"
#[inline]
fn last_word_width<const WORDS: usize>(word_index: usize) -> u8 {
    const BITS_PER_WORD: usize = usize::BITS as usize;
    if word_index + 1 == WORDS {
        // 最后一个字：宽度 = 总位数对 64 取余（整除时取满宽）
        let remaining = WORDS * BITS_PER_WORD % BITS_PER_WORD;
        if remaining == 0 {
            BITS_PER_WORD as u8
        } else {
            remaining as u8
        }
    } else {
        BITS_PER_WORD as u8
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_set_clear_flip() {
        let mut bm: Bitmap<2> = Bitmap::new();
        assert!(!bm.test(0));
        bm.set(0);
        bm.set(127);
        assert!(bm.test(0));
        assert!(bm.test(127));
        bm.flip(0);
        assert!(!bm.test(0));
        bm.clear(127);
        assert!(!bm.test(127));
    }

    #[test]
    fn test_boundary_crossing() {
        // 位 63 与 64 分属两个字，验证换算正确
        let mut bm: Bitmap<2> = Bitmap::new();
        bm.set(63);
        bm.set(64);
        assert!(bm.test(63));
        assert!(bm.test(64));
        assert_eq!(bm.count_ones(), 2);
    }

    #[test]
    fn test_find_first() {
        let mut bm: Bitmap<4> = Bitmap::new();
        // 初始全 0：第一个空闲位是 0
        assert_eq!(bm.find_first_zero(), Some(0));
        assert_eq!(bm.find_first_set(), None);

        bm.set_all();
        assert_eq!(bm.find_first_zero(), None);
        assert_eq!(bm.find_first_set(), Some(0));
        assert_eq!(bm.count_ones(), 4 * 64);

        bm.clear_all();
        bm.set(100);
        assert_eq!(bm.find_first_zero(), Some(0));
        assert_eq!(bm.find_first_set(), Some(100));
        assert!(!bm.is_empty());
        bm.clear(100);
        assert!(bm.is_empty());
    }

    #[test]
    fn test_iter_ones() {
        let mut bm: Bitmap<2> = Bitmap::new();
        bm.set(1);
        bm.set(70);
        bm.set(2);
        let ones: std::vec::Vec<usize> = bm.iter_ones().collect();
        assert_eq!(ones, [1, 2, 70]);
    }
}
