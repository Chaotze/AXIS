// ============================================================
// 位图（堆支持）
// ============================================================
// 用 1 bit 表示 1 个对象占用状态的紧凑布尔数组。
//
// 为什么需要位图：
// - 伙伴系统/页帧分配器用 1 bit 记录 1 个物理页是否空闲
// - PID 分配器用 1 bit 记录 1 个进程号是否被占用
// - 文件系统用位图记录块的空闲情况
// 位图是"百万级对象状态跟踪"场景下空间效率最优的结构
//
// 为什么本模块只做索引换算、位操作全部委托 bit.rs：
// - 位操作的边界处理（位移越界、宽度限制）集中在一处，
//   符合"高复用率、低重复冗余"的要求

use alloc::vec;
use alloc::vec::Vec;

use super::super::bit::{self, BitIter};

/// 定长位图，`WORDS` 为机器字数，总位数 = WORDS × 64
///
/// Copy/Clone：存储就是 usize 数组，按位复制语义正确，
/// 使位图可作为结构体字段参与按值复制（如 CpuMask、
/// 任务槽位表），避免不必要的引用计数
#[derive(Debug, Clone, Copy)]
pub struct Bitmap<const WORDS: usize> {
    /// 位存储：从 words[0] 的最低位开始线性编号
    words: Vec<usize>,
    /// 总位数（可能不满最后一个机器字）
    bit_len: usize,
}

impl Bitmap {
    /// 以指定总位数创建全零位图（字数组在堆上分配）
    ///
    /// # 为什么不再需要 const 构造：
    /// - 定长版本靠 const fn 在编译期写入 .data 段，配合静态字段
    ///   使用；堆支持版本容量在运行期才确定，天然由 new 分配
    pub fn new(bits: usize) -> Self {
        let words = bits.div_ceil(usize::BITS as usize);
        Self {
            words: vec![0; words],
            bit_len: bits,
        }
    }

    /// 总位数
    #[inline]
    pub const fn len(&self) -> usize {
        self.bit_len
    }

    /// 是否一个位都没有置位
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&word| word == 0)
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
        bit::set_bit(&mut self.words[word], bit);
    }

    /// 清除位
    #[inline]
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::clear_bit(&mut self.words[word], bit);
    }

    /// 翻转位
    #[inline]
    pub fn flip(&mut self, index: usize) {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::flip_bit(&mut self.words[word], bit);
    }

    /// 测试位
    #[inline]
    pub fn test(&self, index: usize) -> bool {
        debug_assert!(index < self.len(), "位图索引越界");
        let (word, bit) = split_index(index);
        bit::test_bit(self.words[word], bit)
    }

    /// 全部置位
    pub fn set_all(&mut self) {
        for word in self.words.iter_mut() {
            *word = usize::MAX;
        }
    }

    /// 全部清零
    pub fn clear_all(&mut self) {
        for word in self.words.iter_mut() {
            *word = 0;
        }
    }

    /// 从 0 号位开始查找第一个空闲（0）位
    ///
    /// # 为什么是全位图范围而不是给定起点：
    /// - 全范围查找是分配器的第一需求；带起点的查找
    ///   可以在上层对切片调用同一逻辑实现
    pub fn find_first_zero(&self) -> Option<usize> {
        for (word_index, &word) in self.words.iter().enumerate() {
            // 最后一个字可能不满 64 位，宽度按实际计算
            let width = self.last_word_width(word_index);
            if let Some(bit) = bit::find_first_zero(word, width) {
                return Some(word_index * usize::BITS as usize + bit as usize);
            }
        }
        None
    }

    /// 从 0 号位开始查找第一个占用（1）位
    pub fn find_first_set(&self) -> Option<usize> {
        for (word_index, &word) in self.words.iter().enumerate() {
            if let Some(bit) = bit::find_first_set(word) {
                return Some(word_index * usize::BITS as usize + bit as usize);
            }
        }
        None
    }

    /// 统计置位的总数
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|&word| word.count_ones() as usize).sum()
    }

    /// 按升序迭代所有置位的位号（复用 bit.rs 的 BitIter）
    pub fn iter_ones(&self) -> BitIter<'_> {
        BitIter::new(&self.words)
    }

    /// 计算第 `word_index` 个字的有效宽度
    ///
    /// # 为什么需要这个函数：
    /// - 总位数可能不满整个机器字，最后一个字的高位会超出总位数，
    ///   查找空闲位时必须把越界高位排除，否则会返回"幻影位"
    #[inline]
    fn last_word_width(&self, word_index: usize) -> u8 {
        const BITS_PER_WORD: usize = usize::BITS as usize;
        if word_index + 1 == self.words.len() {
            // 最后一个字：宽度 = 总位数对 64 取余（整除时取满宽）
            let remaining = self.bit_len % BITS_PER_WORD;
            if remaining == 0 {
                BITS_PER_WORD as u8
            } else {
                remaining as u8
            }
        } else {
            BITS_PER_WORD as u8
        }
    }

    /// 暴露底层机器字切片（供需要直接操作内存的调用方，
    /// 如整体清零/写入设备寄存器）
    #[inline]
    pub fn words(&self) -> &[usize] {
        &self.words
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

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_set_clear_flip() {
        let mut bm = Bitmap::new(128);
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
        let mut bm = Bitmap::new(128);
        bm.set(63);
        bm.set(64);
        assert!(bm.test(63));
        assert!(bm.test(64));
        assert_eq!(bm.count_ones(), 2);
    }

    #[test]
    fn test_find_first() {
        let mut bm = Bitmap::new(256);
        // 初始全 0：第一个空闲位是 0
        assert_eq!(bm.find_first_zero(), Some(0));
        assert_eq!(bm.find_first_set(), None);

        bm.set_all();
        assert_eq!(bm.find_first_zero(), None);
        assert_eq!(bm.find_first_set(), Some(0));
        assert_eq!(bm.count_ones(), 256);

        bm.clear_all();
        bm.set(100);
        assert_eq!(bm.find_first_zero(), Some(0));
        assert_eq!(bm.find_first_set(), Some(100));
        assert!(!bm.is_empty());
        bm.clear(100);
        assert!(bm.is_empty());
    }

    #[test]
    fn test_partial_last_word() {
        // 总位数不满一个字：末位以外的幻影位不能被视为空闲
        let mut bm = Bitmap::new(70);
        assert_eq!(bm.len(), 70);
        bm.set_all();
        assert_eq!(bm.find_first_zero(), None);
        // 最后一位（69）可置位/清除
        bm.clear(69);
        assert_eq!(bm.find_first_zero(), Some(69));
    }

    #[test]
    fn test_iter_ones() {
        let mut bm = Bitmap::new(128);
        bm.set(1);
        bm.set(70);
        bm.set(2);
        let ones: std::vec::Vec<usize> = bm.iter_ones().collect();
        assert_eq!(ones, [1, 2, 70]);
    }
}
