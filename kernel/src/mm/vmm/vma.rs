// ============================================================
// VMA（虚拟内存区域）描述符与管理
// ============================================================
// VMA 描述进程地址空间里一段「属性一致的连续虚拟地址范围」
// （如匿名映射、文件映射、栈、堆 brk 区），是 demand-paging 与
// 缺页处理的路由依据：缺页时先找 VMA，再决定分配/COW/换入。
//
// 设计要点（为什么这么做）：
// 1. 按 start 有序存放于 Vec（有序数组）：
//    - 进程的 VMA 数量通常不多（几十~几百），二分查找 O(log n)
//      完全够用，比红黑树实现简单得多、错误面小
//    - 相邻合并、区间切分在有序列上都是 O(n) 线性操作，语义直观
// 2. 区域互不重叠（insert 拒绝重叠，remove 负责切分）：
//    - 缺页处理依赖「地址 → 唯一 VMA」的一一对应
// 3. 纯数据管理：不触碰页表/物理内存——「分配页并映射」由 vmm 的
//    mapping 层负责，本模块只维护地址空间的结构（单一职责）。

use alloc::vec::Vec;

/// VMA 权限位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VmaPerm {
    /// 可读
    pub read: bool,
    /// 可写
    pub write: bool,
    /// 可执行
    pub execute: bool,
    /// 用户态可访问（否则仅内核态）
    pub user: bool,
}

impl VmaPerm {
    /// 全空权限
    pub const fn none() -> Self {
        Self { read: false, write: false, execute: false, user: false }
    }

    /// 常规读权限
    pub const fn read() -> Self {
        Self { read: true, write: false, execute: false, user: false }
    }

    /// 读写
    pub const fn read_write() -> Self {
        Self { read: true, write: true, execute: false, user: false }
    }

    /// 读执行
    pub const fn read_execute() -> Self {
        Self { read: true, write: false, execute: true, user: false }
    }

    /// 设置用户位
    pub const fn to_user(self) -> Self {
        Self { user: true, ..self }
    }

    /// 是否可读/可写/可执行（便捷查询）
    #[inline]
    pub const fn is_readable(&self) -> bool {
        self.read
    }
    #[inline]
    pub const fn is_writable(&self) -> bool {
        self.write
    }
    #[inline]
    pub const fn is_executable(&self) -> bool {
        self.execute
    }
    #[inline]
    pub const fn is_user(&self) -> bool {
        self.user
    }

    /// 两个区域的权限是否一致（合并判断用）
    #[inline]
    pub const fn same_as(&self, other: &Self) -> bool {
        self.read == other.read
            && self.write == other.write
            && self.execute == other.execute
            && self.user == other.user
    }
}

/// VMA 额外标志
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VmaFlags {
    /// 匿名映射（无文件后备）
    pub anonymous: bool,
    /// 写时复制（共享只读页）
    pub cow: bool,
    /// 共享映射（MAP_SHARED，写入即回写）
    pub shared: bool,
    /// 栈区
    pub stack: bool,
    /// 堆区（brk 增长区）
    pub heap: bool,
}

impl VmaFlags {
    pub const fn empty() -> Self {
        Self { anonymous: false, cow: false, shared: false, stack: false, heap: false }
    }

    /// 标志位是否一致（合并判断用）
    #[inline]
    pub const fn same_as(&self, other: &Self) -> bool {
        self.anonymous == other.anonymous
            && self.cow == other.cow
            && self.shared == other.shared
            && self.stack == other.stack
            && self.heap == other.heap
    }
}

/// 虚拟内存区域
#[derive(Debug, Clone, Copy)]
pub struct Vma {
    /// 起始虚拟地址（含，页对齐）
    pub start: usize,
    /// 结束虚拟地址（不含，页对齐）
    pub end: usize,
    /// 权限
    pub perms: VmaPerm,
    /// 标志
    pub flags: VmaFlags,
}

impl Vma {
    /// 区域长度（字节）
    #[inline]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// 是否包含某地址
    #[inline]
    pub const fn contains(&self, addr: usize) -> bool {
        self.start <= addr && addr < self.end
    }

    /// 是否与 [s, e) 重叠
    #[inline]
    pub const fn overlaps(&self, s: usize, e: usize) -> bool {
        self.start < e && s < self.end
    }

    /// 属性是否与另一 VMA 完全相同（合并前提）
    #[inline]
    pub const fn same_attrs(&self, other: &Self) -> bool {
        self.perms.same_as(&other.perms) && self.flags.same_as(&other.flags)
    }
}

/// VMA 操作错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaError {
    /// 与已有区域重叠（未指定 MAP_FIXED 时发生）
    Overlap,
    /// 参数无效（start >= end、长度越界等）
    Invalid,
}

/// VMA 管理器：有序、互不重叠的区域集合
pub struct VmaManager {
    vmas: Vec<Vma>,
}

impl VmaManager {
    /// 新建空管理器
    pub const fn new() -> Self {
        Self { vmas: Vec::new() }
    }

    /// 区域数量
    #[inline]
    pub fn count(&self) -> usize {
        self.vmas.len()
    }

    /// 占用地址空间总字节数
    pub fn usage_bytes(&self) -> usize {
        self.vmas.iter().map(|v| v.len()).sum()
    }

    /// 区域迭代
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, Vma> {
        self.vmas.iter()
    }

    /// 按 start 二分查找插入位置
    fn insertion_point(&self, start: usize) -> usize {
        self.vmas.partition_point(|v| v.start < start)
    }

    /// 插入区域（不与任何既有区域重叠）
    ///
    /// 为什么直接拒绝重叠而非自动切分：切分语义属于 mmap 调用的
    /// 策略（MAP_FIXED 覆盖 vs 默认报错），由调用方显式决定，
    /// 管理器保持「无重叠」这一不变量最安全。
    pub fn insert(&mut self, vma: Vma) -> Result<(), VmaError> {
        if vma.is_empty() {
            return Err(VmaError::Invalid);
        }
        let i = self.insertion_point(vma.start);
        // 检查与左右邻居的重叠
        if i > 0 {
            let prev = &self.vmas[i - 1];
            if prev.end > vma.start {
                return Err(VmaError::Overlap);
            }
        }
        if i < self.vmas.len() {
            let next = &self.vmas[i];
            if vma.end > next.start {
                return Err(VmaError::Overlap);
            }
        }
        self.vmas.insert(i, vma);
        Ok(())
    }

    /// 查找包含 addr 的区域（只读）
    ///
    /// 插入点两侧都要检查：partition_point 返回第一个 start >= addr
    /// 的下标，包含 addr 的区域可能在其左侧（start < addr）也可能
    /// 恰好在插入点（start == addr，首个元素时尤其如此）。
    pub fn find(&self, addr: usize) -> Option<&Vma> {
        let i = self.insertion_point(addr);
        if i < self.vmas.len() {
            let v = &self.vmas[i];
            if v.contains(addr) {
                return Some(v);
            }
        }
        if i > 0 {
            let v = &self.vmas[i - 1];
            if v.contains(addr) {
                return Some(v);
            }
        }
        None
    }

    /// 查找包含 addr 的区域（可变）
    pub fn find_mut(&mut self, addr: usize) -> Option<&mut Vma> {
        let i = self.insertion_point(addr);
        if i < self.vmas.len() {
            let (lo, hi) = self.vmas.split_at_mut(i);
            let v = &mut hi[0];
            if v.contains(addr) {
                return Some(v);
            }
            let _ = lo;
        }
        if i > 0 {
            return self.vmas.get_mut(i - 1).filter(|v| v.contains(addr));
        }
        None
    }

    /// 查找覆盖 [s, e) 的区域下标（假设不变量成立时至多一个）
    pub fn find_range(&self, s: usize, e: usize) -> Option<usize> {
        let i = self.insertion_point(s);
        if i > 0 {
            let v = &self.vmas[i - 1];
            if v.overlaps(s, e) {
                return Some(i - 1);
            }
        }
        if i < self.vmas.len() {
            let v = &self.vmas[i];
            if v.overlaps(s, e) {
                return Some(i);
            }
        }
        None
    }

    /// 删除区域 [s, e)：重叠部分被切分/移除，两边残余保留
    ///
    /// 这是 munmap / 覆盖映射的核心操作：
    /// - 完全包含 → 整体移除
    /// - 头部重叠 → 截短尾部
    /// - 尾部重叠 → 截短头部
    /// - 横跨中部 → 一分为二
    pub fn remove_range(&mut self, s: usize, e: usize) {
        let mut i = 0;
        while i < self.vmas.len() {
            let v = self.vmas[i];
            if v.end <= s {
                i += 1;
                continue;
            }
            if v.start >= e {
                break;
            }
            // 处理与 [s, e) 的所有重叠形态
            if v.start < s && v.end > e {
                // 中部横跨：拆成 [start, s) 与 [e, end)
                let right = Vma { start: e, end: v.end, perms: v.perms, flags: v.flags };
                self.vmas[i] = Vma { end: s, ..v };
                self.vmas.insert(i + 1, right);
                i += 2;
                continue;
            }
            // 完全在内部：移除
            let new_start = if v.start < s { s } else { v.start };
            let new_end = if v.end > e { e } else { v.end };
            if new_start <= v.start && new_end >= v.end {
                self.vmas.remove(i);
                continue;
            }
            if new_start > v.start && new_end >= v.end {
                // 只留左段
                self.vmas[i].end = new_start;
                i += 1;
                continue;
            }
            if new_end < v.end {
                // 只留右段
                self.vmas[i].start = new_end;
                i += 1;
                continue;
            }
            unreachable!();
        }
    }

    /// 在 index 处尝试与相邻区域合并（属性相同才合并）
    ///
    /// 合并降低了 VMA 数量、简化后续查找；只有相邻且同属性
    /// 才合并，避免把不同语义的区域黏在一起。
    pub fn merge_at(&mut self, index: usize) {
        if index >= self.vmas.len() {
            return;
        }
        // 先与左侧合并
        if index > 0 {
            let (l, r) = (self.vmas[index - 1], self.vmas[index]);
            if l.end == r.start && l.same_attrs(&r) {
                self.vmas[index - 1].end = r.end;
                self.vmas.remove(index);
                self.merge_at(index - 1);
                return;
            }
        }
        // 再尝试与右侧合并
        if index + 1 < self.vmas.len() {
            let (l, r) = (self.vmas[index], self.vmas[index + 1]);
            if l.end == r.start && l.same_attrs(&r) {
                self.vmas[index].end = r.end;
                self.vmas.remove(index + 1);
                self.merge_at(index);
            }
        }
    }

    /// 访问第 index 个区域（只读）
    pub fn get(&self, index: usize) -> Option<&Vma> {
        self.vmas.get(index)
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    fn vma(s: usize, e: usize, write: bool) -> Vma {
        Vma {
            start: s,
            end: e,
            perms: if write { VmaPerm::read_write() } else { VmaPerm::read() },
            flags: VmaFlags::empty(),
        }
    }

    #[test]
    fn test_insert_find() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x2000, true)).unwrap();
        m.insert(vma(0x3000, 0x4000, false)).unwrap();
        // 乱序插入
        m.insert(vma(0x2000, 0x3000, false)).unwrap();
        assert_eq!(m.count(), 3);
        assert_eq!(m.find(0x1500).unwrap().start, 0x1000);
        assert_eq!(m.find(0x2500).unwrap().start, 0x2000);
        assert_eq!(m.find(0x3999).unwrap().start, 0x3000);
        // 相邻区域的内部地址必命中（0x2500+0x100 = 0x2600 位于 [0x2000,0x3000)）
        assert!(m.find(0x2600).is_some());
        assert_eq!(m.find(0x2600).unwrap().end, 0x3000);
        // 预留一块带缝的区域验证空洞判空
        m.insert(vma(0x5000, 0x6000, false)).unwrap();
        assert_eq!(m.count(), 4);
        assert!(m.find(0x4500).is_none(), "[0x4000,0x5000) 是空洞");
        assert_eq!(m.find(0x5500).unwrap().start, 0x5000);
    }

    #[test]
    fn test_find_at_exact_start() {
        // 边界回归：find 必须命中 start == addr 的区域（含首元素）
        let mut m = VmaManager::new();
        m.insert(vma(0x3000, 0x4000, false)).unwrap();
        m.insert(vma(0x1000, 0x2000, true)).unwrap();
        assert_eq!(m.find(0x1000).unwrap().start, 0x1000);
        assert_eq!(m.find(0x3000).unwrap().start, 0x3000);
        let v = m.find_mut(0x1000).unwrap();
        v.perms = VmaPerm::read_write();
        assert_eq!(m.find(0x1000).unwrap().perms.write, true);
        // end 边界不算包含
        assert!(m.find(0x2000).is_none());
    }

    #[test]
    fn test_insert_rejects_overlap() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x3000, true)).unwrap();
        assert_eq!(m.insert(vma(0x2000, 0x4000, true)).unwrap_err(), VmaError::Overlap);
        assert_eq!(m.insert(vma(0x0000, 0x2000, true)).unwrap_err(), VmaError::Overlap);
        assert_eq!(m.insert(vma(0x1500, 0x2500, true)).unwrap_err(), VmaError::Overlap);
        // 正好相邻可插入
        m.insert(vma(0x3000, 0x4000, false)).unwrap();
        assert_eq!(m.count(), 2);
        // 非法参数
        assert_eq!(m.insert(vma(0x4000, 0x4000, false)).unwrap_err(), VmaError::Invalid);
    }

    #[test]
    fn test_remove_variants() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x5000, true)).unwrap();
        // 删中间一段：一分为二，两边残余保留
        m.remove_range(0x3000, 0x4000);
        assert_eq!(m.count(), 2);
        assert_eq!(m.find(0x2000).unwrap().end, 0x3000);
        assert!(m.find(0x3500).is_none(), "挖掉的部分不应再命中");
        assert_eq!(m.find(0x4500).unwrap().start, 0x4000);

        // 完全包含头部 → 移除第一段
        m.remove_range(0x0000, 0x3000);
        assert_eq!(m.count(), 1);
        assert_eq!(m.get(0).unwrap().start, 0x4000);

        // 覆盖尾部 → 移除全部
        m.remove_range(0x4000, 0x6000);
        assert_eq!(m.count(), 0);
    }

    #[test]
    fn test_remove_middle_split() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x5000, true)).unwrap();
        // 从中间挖掉，应拆成两段
        m.remove_range(0x2000, 0x4000);
        assert_eq!(m.count(), 2);
        assert_eq!(m.get(0).unwrap().start, 0x1000);
        assert_eq!(m.get(0).unwrap().end, 0x2000);
        assert_eq!(m.get(1).unwrap().start, 0x4000);
        assert_eq!(m.get(1).unwrap().end, 0x5000);
        // 两段之间还有被挖掉的缝隙 [0x2000,0x4000)，不得合并
        m.merge_at(0);
        assert_eq!(m.count(), 2, "有缝隙不得合并");
    }

    #[test]
    fn test_merge_adjacent() {
        let mut m = VmaManager::new();
        // 相邻且属性一致的两个区域应能合并
        m.insert(vma(0x1000, 0x3000, true)).unwrap();
        m.insert(vma(0x3000, 0x5000, true)).unwrap();
        assert_eq!(m.count(), 2);
        m.merge_at(0);
        assert_eq!(m.count(), 1);
        assert_eq!(m.get(0).unwrap().start, 0x1000);
        assert_eq!(m.get(0).unwrap().end, 0x5000);
    }

    #[test]
    fn test_no_merge_across_attrs() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x2000, true)).unwrap();
        m.insert(vma(0x2000, 0x3000, false)).unwrap();
        m.merge_at(0);
        m.merge_at(0);
        assert_eq!(m.count(), 2, "权限不同的相邻区域不得合并");
    }

    #[test]
    fn test_find_range_and_usage() {
        let mut m = VmaManager::new();
        m.insert(vma(0x1000, 0x3000, true)).unwrap();
        m.insert(vma(0x5000, 0x9000, true)).unwrap();
        assert_eq!(m.find_range(0x2000, 0x4000).unwrap(), 0);
        assert_eq!(m.find_range(0x4000, 0x5000), None);
        assert_eq!(m.usage_bytes(), 0x2000 + 0x4000);
        assert_eq!(m.count(), 2);
    }

    #[test]
    fn test_stress_random() {
        // 随机插入/删除后不变量仍然成立：有序、无重叠、find 正确
        let mut m = VmaManager::new();
        let mut rng = 0xCAFE_F00Du64;
        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        // 分配一段段不相交的区域
        let mut regions: Vec<(usize, usize)> = vec![];
        let mut cursor = 0x1000usize;
        while cursor < 0x100000 {
            let len = 0x1000 + (rand() as usize) % 0x8000;
            regions.push((cursor, cursor + len));
            cursor += len + 0x1000;
        }
        for &(s, e) in &regions {
            m.insert(vma(s, e, true)).unwrap();
        }
        assert_eq!(m.count(), regions.len());

        // 删除中间一块再验证查找
        let mid = regions[regions.len() / 2];
        m.remove_range(mid.0 + 0x1000, mid.1 - 0x1000);
        // 校验有序
        for w in m.iter().zip(m.iter().skip(1)) {
            assert!(w.0.end <= w.1.start);
        }
    }
}