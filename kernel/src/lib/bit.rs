// ============================================================
// 位操作工具函数
// ============================================================
// 提供以机器字（usize）为单位的位操作，以及置位迭代器。
//
// 为什么需要独立的位操作模块：
// - 位操作是内存管理（页帧位图）、进程管理（PID 分配）、
//   文件系统（块位图）等子系统的公共基础操作
// - 集中实现一次、各处复用，避免每个模块各写一套位运算，
//   既减少冗余，也便于统一修正边界问题（如位移越界）
//
// 为什么直接用 usize 作为操作单位：
// - usize 与 CPU 字长一致，一条指令即可完成读改写，
//   比逐字节/逐 bit 的操作效率高一个数量级
// - 所有使用者（Bitmap、分配器）都以机器字组织存储

/// 设置字中的第 `bit` 位（0 = 最低位）
///
/// # 参数
/// - `word`: 目标机器字
/// - `bit`: 位号，必须小于 usize 的位数（64）
///
/// # 为什么用 debug_assert 而不是 assert：
/// - 越界属于调用方的逻辑错误，发布版中省略检查以保持
///   关键路径（分配器）零开销
#[inline]
pub fn set_bit(word: &mut usize, bit: u8) {
    debug_assert!(bit < usize::BITS as u8, "位号越界");
    *word |= 1usize << bit;
}

/// 清除字中的第 `bit` 位
///
/// 为什么需要独立函数而不是让调用方自己写 `&= !(1<<bit)`：
/// - 位号越界时 `1usize << 64` 是未定义行为，集中封装后
///   只需在一处保证边界检查
#[inline]
pub fn clear_bit(word: &mut usize, bit: u8) {
    debug_assert!(bit < usize::BITS as u8, "位号越界");
    *word &= !(1usize << bit);
}

/// 翻转字中的第 `bit` 位
#[inline]
pub fn flip_bit(word: &mut usize, bit: u8) {
    debug_assert!(bit < usize::BITS as u8, "位号越界");
    *word ^= 1usize << bit;
}

/// 测试字中的第 `bit` 位是否为 1
#[inline]
pub fn test_bit(word: usize, bit: u8) -> bool {
    debug_assert!(bit < usize::BITS as u8, "位号越界");
    word & (1usize << bit) != 0
}

/// 生成低 `n` 位的掩码
///
/// # 为什么特判 n == 64：
/// - `(1usize << 64)` 是位移越界（未定义行为），
///   而 `wrapping_shl` 对 64 位位移会得到 0 而非全 1，
///   所以全宽度必须显式返回 usize::MAX
#[inline]
pub fn mask_below(n: u8) -> usize {
    debug_assert!(n <= usize::BITS as u8, "掩码宽度越界");
    if n == usize::BITS as u8 {
        usize::MAX
    } else {
        (1usize << n) - 1
    }
}

/// 查找字中最低置位（1 位）的位号
///
/// 为什么包装 trailing_zeros：
/// - 提供语义化的命名（find_first_set）与 Option 返回值，
///   调用方不必自行处理"全 0 = 64"这一特殊值
#[inline]
pub fn find_first_set(word: usize) -> Option<u8> {
    if word == 0 {
        None
    } else {
        Some(word.trailing_zeros() as u8)
    }
}

/// 在 `width` 位范围内查找最低的 0 位
///
/// 为什么需要 `width` 参数：
/// - 位图的最后一个字可能只使用了一部分（不足 64 位），
///   未使用的高位恒为 0，若不限制宽度会被误判为空闲位
#[inline]
pub fn find_first_zero(word: usize, width: u8) -> Option<u8> {
    debug_assert!(width <= usize::BITS as u8, "宽度越界");
    let usable = if width == usize::BITS as u8 {
        usize::MAX
    } else {
        mask_below(width)
    };
    // 在可用范围内取反，为 1 的位即原本为 0 的位
    let free = (!word) & usable;
    find_first_set(free)
}

/// 置位迭代器：按升序遍历一个机器字数组中所有为 1 的位
///
/// 为什么需要这个迭代器：
/// - 位图类结构最常见的查询是"枚举所有占用的项"（如遍历
///   已分配的页帧），逐字 + 逐位扫描逻辑若散落在各使用方，
///   既重复又易错
/// - 迭代器把"跳过全 0 字"和"剥离最低置位"两处优化集中实现
pub struct BitIter<'a> {
    /// 底层机器字数组（借用，迭代器不拥有数据）
    words: &'a [usize],
    /// 下一个待加载的字的下标
    word_index: usize,
    /// 当前字的剩余内容（尚未产出的置位）
    current: usize,
    /// 当前字在全局位空间中的起始位号
    ///
    /// 为什么单独记录基准位号：
    /// - 加载新字时 word_index 已自增，若用自增后的下标
    ///   计算位号会整体偏移一个机器字
    base: usize,
}

impl<'a> BitIter<'a> {
    /// 创建迭代器
    pub const fn new(words: &'a [usize]) -> Self {
        Self {
            words,
            word_index: 0,
            current: 0,
            base: 0,
        }
    }
}

impl Iterator for BitIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        loop {
            // 当前字还有置位：剥离最低置位并产出位号
            if self.current != 0 {
                let bit = self.current.trailing_zeros() as usize;
                // 清除最低置位（n & (n-1) 技巧，避免再次扫描）
                self.current &= self.current - 1;
                return Some(self.base + bit);
            }

            // 当前字耗尽，前进到下一个非零字
            let word = *self.words.get(self.word_index)?;
            self.base = self.word_index * usize::BITS as usize;
            self.word_index += 1;
            self.current = word;
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_set_clear_flip_roundtrip() {
        let mut word = 0usize;
        set_bit(&mut word, 0);
        set_bit(&mut word, 63);
        assert!(test_bit(word, 0));
        assert!(test_bit(word, 63));
        assert!(!test_bit(word, 1));

        flip_bit(&mut word, 0);
        assert!(!test_bit(word, 0));
        flip_bit(&mut word, 0);
        assert!(test_bit(word, 0));

        clear_bit(&mut word, 63);
        assert!(!test_bit(word, 63));
    }

    #[test]
    fn test_mask_below() {
        assert_eq!(mask_below(0), 0);
        assert_eq!(mask_below(1), 1);
        assert_eq!(mask_below(8), 0xFF);
        // 64 位全宽是位移越界的特例，必须返回全 1
        assert_eq!(mask_below(64), usize::MAX);
    }

    #[test]
    fn test_find_first() {
        assert_eq!(find_first_set(0), None);
        assert_eq!(find_first_set(1), Some(0));
        assert_eq!(find_first_set(1 << 63), Some(63));

        // 0b1011，宽度 4：bit 2 是第一个 0
        assert_eq!(find_first_zero(0b1011, 4), Some(2));
        // 全 1：无 0 位
        assert_eq!(find_first_zero(0b1111, 4), None);
        // 宽度限制：第 4 位虽为 0，但不在宽度内
        assert_eq!(find_first_zero(0b01111, 4), None);
    }

    #[test]
    fn test_bit_iter() {
        // 三个字：置位分布在 0、63、64、100 位
        let mut words = [0usize; 2];
        set_bit(&mut words[0], 0);
        set_bit(&mut words[0], 63);
        set_bit(&mut words[1], 0); // 第 64 位
        set_bit(&mut words[1], 36); // 第 100 位

        let bits: std::vec::Vec<usize> = BitIter::new(&words).collect();
        assert_eq!(bits, [0, 63, 64, 100]);
    }

    #[test]
    fn test_bit_iter_empty() {
        let words = [0usize; 3];
        assert_eq!(BitIter::new(&words).count(), 0);
    }
}
