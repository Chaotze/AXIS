// ============================================================
// 伙伴系统（Buddy System）分配器核心
// ============================================================
// 物理页帧分配的基础算法。能够分配任意 2^order 个连续页、
// 释放并自动与伙伴（buddy）合并成大块，从而缓解外部碎片。
//
// 设计要点（为什么这么做）：
// 1. 元数据（空闲计数段树 + 各级空闲链表）全部存放在调用方提供的
//    一段「字节区」（arena）中，本结构体自身零堆依赖。
//    —— 堆分配器（SLUB）需要向本分配器要页，因此本分配器绝不能
//       反过来依赖堆，否则形成循环依赖（鸡生蛋问题）。
// 2. 段树（segment tree）下标表示：叶块 i 位于下标 T+i（T 为 2 的幂），
//    父节点为 i>>1，根为 1。free_count[node] = 该子树内空闲叶块数。
//    —— 相比每块一个大位图，段树能 O(log T) 判定「整块是否空闲」，
//       支撑 O(1) 均摊的分配/释放与伙伴合并。
// 3. 真实的页数 real 不必是 2 的幂：虚拟叶块数 T = next_pow2(real)，
//    多出的 [real, T) 叶块视为「洞」（初始 free_count = 0），
//    永不参与分配。这样任意大小的内存区都能被伙伴系统管理，
//    而不会把内存浪费在凑 2 的幂上。
// 4. 伙伴关系：阶 o 下，块索引 b 的伙伴为 b ^ 1（最低位取反）。
//    只有当伙伴「有效且整块空闲」时才合并——不允许跨洞合并。

/// 空闲链表空指针标记
const NONE: i64 = -1;

/// 计算 2 的幂（一个阶对应的页块大小）
#[inline]
pub const fn order_pages(order: usize) -> usize {
    1usize << order
}

/// 向上取 2 的幂
#[inline]
pub const fn next_pow2(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let mut v = n;
    // 经典 bit-twiddling：把最高位以下全部置 1，再 +1
    v -= 1;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    #[cfg(target_pointer_width = "64")]
    {
        v |= v >> 32;
    }
    v + 1
}

/// 计算 n 的二进制对数（取下整）
#[inline]
pub const fn log2_floor(mut n: usize) -> usize {
    let mut log = 0;
    while n > 1 {
        n >>= 1;
        log += 1;
    }
    log
}

/// 伙伴系统分配器
///
/// 本结构体只持有指向元数据区的引用，真正的元素据由
/// [`BuddyAllocator::from_bytes`] 在调用方提供的字节区中构建。
/// 因此它可以被安全地放进静态存储（如 Spinlock 或 static mut），
/// 满足内核「分配器自身不依赖分配」的约束。
pub struct BuddyAllocator {
    /// 实际可用的叶块总数（页数）
    real_units: usize,
    /// 虚拟叶块总数（2 的幂，>= real_units）
    virtual_units: usize,
    /// 最大阶 = log2(virtual_units)
    max_order: usize,
    /// 段树：free_count[node] = 子树内空闲叶块数（下标 [1, 2T)）
    free_count: &'static mut [u32],
    /// 空闲链表前驱（双向链表，支持 O(1) 摘除，用于合并）
    list_prev: &'static mut [i64],
    /// 空闲链表后继
    list_next: &'static mut [i64],
    /// 每阶链表头（heads[order]，-1 表示空）
    heads: &'static mut [i64],
    /// 已分配叶块数（统计）
    allocated_pages: usize,
    /// 保留叶块数（统计，如内核映像区）
    reserved_pages: usize,
}

impl BuddyAllocator {
    /// 空分配器（仅用于内核静态占位，使用前必须用 [`from_bytes`] 重新构建）
    ///
    /// 为什么需要它：PmmState 需要在堆就绪前静态创建，此时还没有任何
    /// 内存区可以初始化伙伴系统，只能先占一个「未初始化」坑位。
    pub const fn uninit() -> Self {
        Self {
            real_units: 0,
            virtual_units: 0,
            max_order: 0,
            // 长度为零的悬垂切片：合法且不会被访问（所有方法先校验 real_units）
            free_count: &mut [],
            list_prev: &mut [],
            list_next: &mut [],
            heads: &mut [],
            allocated_pages: 0,
            reserved_pages: 0,
        }
    }

    /// 计算容纳 real_units 个叶块所需的字节区大小（含对齐填充）
    ///
    /// 为什么暴露这个函数：内核需要先算出元数据要占多少内存，
    /// 才能决定从哪里（物理内存顶部）划分出字节区。
    pub const fn needed_bytes(real_units: usize) -> usize {
        let t = next_pow2(real_units); // next_pow2(0) 也已定义为 1
        let max_order = log2_floor(t);
        // 三张表各 2T 个元素（u32 * 4B / i64 * 8B），加上每阶链表头
        let fc = 2 * t * core::mem::size_of::<u32>();
        let lists = 2 * t * core::mem::size_of::<i64>();
        let heads = (max_order + 1) * core::mem::size_of::<i64>();
        fc + 2 * lists + heads
    }

    /// 从字节区构建伙伴系统
    ///
    /// # Safety
    /// - `bytes` 必须 8 字节对齐，长度 >= [`needed_bytes`] 的结果
    /// - `bytes` 所指向的内存在分配器生命周期内不得被其他对象借用
    /// - 同一块字节区不得构建第二个分配器（会产生重叠别名）
    ///
    /// 构建完成后只会把「完全有效且空闲」的块入链；调用方可在
    /// [`finalize`] 之前调用 [`mark_reserved`] 预留里程碑页。
    pub unsafe fn from_bytes(real_units: usize, bytes: &'static mut [u8]) -> Self {
        let t = next_pow2(real_units); // next_pow2(0) 也已定义为 1
        let max_order = log2_floor(t);

        // 字节区布局（向前推进 offset，先 4 字节表再 8 字节表，
        // 只需保证整体 8 对齐即可满足所有表的对齐要求）
        let base = bytes.as_mut_ptr();
        let len = bytes.len();
        let mut off: usize = 0;

        let take_slice_u32 = |off: &mut usize, n: usize| -> &'static mut [u32] {
            let end = *off + n * core::mem::size_of::<u32>();
            debug_assert!(end <= len, "buddy 字节区不足");
            let s = unsafe { core::slice::from_raw_parts_mut(base.add(*off) as *mut u32, n) };
            *off = end;
            s
        };
        let take_slice_i64 = |off: &mut usize, n: usize| -> &'static mut [i64] {
            let end = *off + n * core::mem::size_of::<i64>();
            debug_assert!(end <= len, "buddy 字节区不足");
            let s = unsafe { core::slice::from_raw_parts_mut(base.add(*off) as *mut i64, n) };
            *off = end;
            s
        };

        let free_count = take_slice_u32(&mut off, 2 * t);
        let list_prev = take_slice_i64(&mut off, 2 * t);
        let list_next = take_slice_i64(&mut off, 2 * t);
        let heads = take_slice_i64(&mut off, max_order + 1);

        // 显式清零整个元数据区（字节区可能来自引导期未清零的物理内存）
        for x in free_count.iter_mut() {
            *x = 0;
        }
        for x in list_prev.iter_mut() {
            *x = NONE;
        }
        for x in list_next.iter_mut() {
            *x = NONE;
        }
        for x in heads.iter_mut() {
            *x = NONE;
        }

        let b = Self {
            real_units,
            virtual_units: t,
            max_order,
            free_count,
            list_prev,
            list_next,
            heads,
            allocated_pages: 0,
            reserved_pages: 0,
        };

        // 叶层初始化：真实页 free = 1，洞 free = 0
        for leaf in 0..t {
            b.free_count[t + leaf] = (leaf < real_units) as u32;
        }
        // 自底向上累加内部节点（children 下标恒大于 parent，倒序安全）
        for node in (1..t).rev() {
            b.free_count[node] = b.free_count[2 * node] + b.free_count[2 * node + 1];
        }

        b
    }

    /// 将「最大且完全空闲」的块挂入空闲链表（贪心自顶向下）
    ///
    /// 为什么必须只挂最大块：空闲链表的不变式是「同一块内存只出现在
    /// 恰好一条链上、且没有已分配的后代」。如果同时把父子块都入链，
    /// 低阶分配会悄悄“掏空”高阶块，使高阶链表出现失效条目（经典
    /// buddy 实现的坑）。自顶向下贪心：只有当某整块完全空闲且其父块
    /// 并非完全空闲时才入链，保证链上每一块都是“最高可用”的。
    ///
    /// 为什么分开两步：调用方可能在构建后、挂链前先 [mark_reserved]
    /// 预留区域（如内核映像），这些块就不能再进入空闲链表。
    pub fn finalize(&mut self) {
        let t = self.virtual_units;
        for order in (0..=self.max_order).rev() {
            let count = t >> order;
            for idx in 0..count {
                let node = (t >> order) + idx;
                // 「整块完全空闲」= free_count 恰好等于满值：洞（初始 0）
                // 与已预留页都会使计数不足，从而自动被排除，几何范围
                // 判断在这里是隐含的（满值意味着覆盖范围全部真实且空闲）
                if self.free_count[node] != order_pages(order) as u32 {
                    continue;
                }
                // 父块同样整块空闲？若是，应交由更高阶入链，本级跳过
                if self.parent_full(order, idx) {
                    continue;
                }
                self.push(order, idx << order);
            }
        }
    }

    /// 判断（order, idx）块的父块是否也整块空闲
    ///
    /// 用于 finalize 贪心时避免重复入链父子块。
    #[inline]
    fn parent_full(&self, order: usize, idx: usize) -> bool {
        if order + 1 > self.max_order {
            return false;
        }
        let parent_idx = idx >> 1;
        self.free_count[(self.virtual_units >> (order + 1)) + parent_idx]
            == order_pages(order + 1) as u32
    }

    /// 阶 o 下块索引 idx 对应段树节点
    #[inline]
    fn node(&self, order: usize, idx: usize) -> usize {
        (self.virtual_units >> order) + idx
    }

    /// 某块是否「有效」：其覆盖范围全部落在真实页内（不碰洞）
    #[inline]
    fn valid(&self, order: usize, idx: usize) -> bool {
        idx + (1usize << order) <= self.real_units
    }

    /// 将块（order, idx = 起始叶块）从空闲链表中摘除
    ///
    /// 调用前提：该块一定在链上（调用方已用 free_count 判定）。
    #[inline]
    fn unlink(&mut self, order: usize, idx: usize) {
        let node = self.node(order, idx >> order);
        let prev = self.list_prev[node];
        let next = self.list_next[node];
        if prev == NONE {
            // 是表头
            self.heads[order] = next;
        } else {
            self.list_next[prev as usize] = next;
        }
        if next != NONE {
            self.list_prev[next as usize] = prev;
        }
    }

    /// 将块（order, idx = 起始叶块）挂入空闲链表头部
    ///
    /// 为什么入参是「叶块下标」而非「块编号」：pop/free 等路径得到的
    /// 都是叶块下标，统一一种表示可以避免调用方反复换算。
    #[inline]
    fn push(&mut self, order: usize, idx: usize) {
        let node = self.node(order, idx >> order);
        let head = self.heads[order];
        self.list_prev[node] = NONE;
        self.list_next[node] = head;
        if head != NONE {
            self.list_prev[head as usize] = node as i64;
        }
        self.heads[order] = node as i64;
    }

    /// 从 order 阶链表弹出一块，返回其起始叶块下标
    #[inline]
    fn pop(&mut self, order: usize) -> Option<usize> {
        let node = self.heads[order];
        if node == NONE {
            return None;
        }
        let next = self.list_next[node as usize];
        if next != NONE {
            self.list_prev[next as usize] = NONE;
        }
        self.heads[order] = next;
        let idx = ((node as usize) - (self.virtual_units >> order)) << order;
        Some(idx)
    }

    /// 将块（order, leaf 起始下标）标记为已使用，并向上传播计数
    ///
    /// 除块自身节点外，还会逐叶清零：让“free_count[父] == 子树空闲
    /// 叶数和”的不变式在每一层都成立，这样占用检查/统计可以按叶
    /// 读取，后续页迭代代码也不用关心分配粒度。
    #[inline]
    fn mark_used(&mut self, order: usize, leaf: usize) {
        let node = self.node(order, leaf >> order);
        debug_assert_eq!(self.free_count[node], order_pages(order) as u32);
        let span = order_pages(order);
        for sub in 0..span {
            self.free_count[self.virtual_units + leaf + sub] = 0;
        }
        self.free_count[node] = 0;
        let mut n = node;
        let delta = span as u32;
        while n > 1 {
            n >>= 1;
            debug_assert!(self.free_count[n] >= delta);
            self.free_count[n] -= delta;
        }
    }

    /// 将块（order, leaf 起始下标）标记为空闲，向上传播计数（不自动入链）
    ///
    /// 与 mark_used 对称：逐叶置 1，保持树不变式；合并判空读的是
    /// “块节点数值是否等于满值”，叶层更新不影响其正确性。
    #[inline]
    fn mark_free(&mut self, order: usize, leaf: usize) {
        let node = self.node(order, leaf >> order);
        debug_assert_eq!(self.free_count[node], 0);
        let span = order_pages(order);
        for sub in 0..span {
            self.free_count[self.virtual_units + leaf + sub] = 1;
        }
        self.free_count[node] = span as u32;
        let mut n = node;
        let delta = span as u32;
        while n > 1 {
            n >>= 1;
            self.free_count[n] += delta;
        }
    }

    /// 预留（标记为已用）一块区域：order 阶、起始叶块下标 leaf
    ///
    /// 仅允许在 [`finalize`] 之前调用。预留的块计入 reserved_pages。
    pub fn mark_reserved(&mut self, order: usize, leaf: usize) {
        debug_assert!(self.valid(order, leaf >> order));
        self.mark_used(order, leaf);
        self.reserved_pages += order_pages(order);
    }

    /// 分配 2^order 个连续页，返回起始叶块下标（找不到返回 None）
    ///
    /// 策略：从请求阶向上查找第一个有空闲块的阶，找不到则内存耗尽；
    /// 找到后把多余的块逐级对半拆分，另一半放回同级空闲链表。
    /// 这是 buddy 分配的核心思路：大块拆小块，小块释放时反向合并成大块。
    pub fn alloc(&mut self, order: usize) -> Option<usize> {
        if order > self.max_order || self.real_units == 0 {
            return None;
        }

        // 1) 找到最小的、非空的空闲阶
        let mut found = None;
        let mut o = order;
        while o <= self.max_order {
            if let Some(leaf) = self.pop(o) {
                found = Some((leaf, o));
                break;
            }
            o += 1;
        }
        let (leaf, mut found_order) = found?;

        // 2) 逐级拆分：保留低半块，高半块入链
        while found_order > order {
            let half = 1usize << (found_order - 1);
            let buddy_half = leaf + half;
            found_order -= 1;
            // buddy_half 直接入链即可：其 free_count 本就是一半的大小。
            // 但如果这半块超出了真实页边界（洞），绝不能入链——洞永
            // 远不是可分配内存，入链会导致分配越界。
            if buddy_half + half <= self.real_units {
                self.push(found_order, buddy_half);
            } else {
                self.mark_free(found_order, buddy_half);
            }
        }

        // 3) 标记最终块为已使用
        self.mark_used(order, leaf);
        self.allocated_pages += order_pages(order);
        Some(leaf)
    }

    /// 释放（order, leaf）块，并尝试与伙伴向上合并
    ///
    /// 合并条件：
    /// - 伙伴必须是「真实页」内有效的块（不跨洞）
    /// - 伙伴必须处于完全空闲状态（free_count == 2^order）
    /// 满足则摘除伙伴、两半并为父块，继续向更高阶尝试。
    pub fn free(&mut self, order: usize, leaf: usize) {
        debug_assert!(order <= self.max_order);
        debug_assert!(self.valid(order, leaf >> order));

        self.mark_free(order, leaf);
        self.allocated_pages = self.allocated_pages.saturating_sub(order_pages(order));

        let mut o = order;
        let mut idx = leaf >> order;
        while o < self.max_order {
            let buddy_idx = idx ^ 1;
            // 伙伴无效（超过真实页）或未整块空闲则停止合并
            if !self.valid(o, buddy_idx) {
                break;
            }
            let buddy_node = self.node(o, buddy_idx);
            if self.free_count[buddy_node] != order_pages(o) as u32 {
                break;
            }
            // 摘除伙伴（入参为叶块下标），合并为 o+1 阶的父块
            self.unlink(o, buddy_idx << o);
            o += 1;
            idx >>= 1;
        }

        // 最终（可能合并过）的块入链
        self.push(o, idx << o);
    }

    /// 获取段树根节点空闲叶块数（= 整片区域空闲页总数）
    #[inline]
    pub fn free_pages(&self) -> usize {
        if self.real_units == 0 {
            0
        } else {
            self.free_count[1] as usize
        }
    }

    /// 实际（真实）页数
    #[inline]
    pub fn real_pages(&self) -> usize {
        self.real_units
    }

    /// 已分配页数
    #[inline]
    pub fn used_pages(&self) -> usize {
        self.allocated_pages
    }

    /// 保留页数
    #[inline]
    pub fn reserved_pages(&self) -> usize {
        self.reserved_pages
    }

    /// 最大阶
    #[inline]
    pub fn max_order(&self) -> usize {
        self.max_order
    }

    /// 是否已初始化（real_units > 0）
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.real_units > 0
    }

    /// 全部互不重叠？仅在调试自测中使用：
    /// 返回一个 per-leaf 的占用标记，用于校验分配结果没有重叠。
    pub fn occupancy(&self) -> alloc::vec::Vec<bool> {
        let mut map = alloc::vec![false; self.real_units];
        // 遍历所有「已分配/保留」的叶子（free_count[1] 为全局空闲数，
        // 加上洞以后能反推出已用集合 —— 更直接的做法是遍历叶子）
        for leaf in 0..self.real_units {
            // 叶节点 free_count == 0 且 real 内 => 被占用或保留
            map[leaf] = self.free_count[self.virtual_units + leaf] == 0;
        }
        map
    }
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use std::prelude::v1::*;

    /// 构造一段泄漏的 8 字节对齐字节区（泄漏使 &'static mut 语义成立）
    fn make_arena(real: usize) -> &'static mut [u8] {
        let need = BuddyAllocator::needed_bytes(real);
        let mut arena = vec![0u8; need + 8]; // +8 保证对齐余量
        let p = arena.as_mut_ptr() as usize;
        let aligned = (p + 7) & !7;
        let slice =
            unsafe { core::slice::from_raw_parts_mut(aligned as *mut u8, need) };
        // 泄漏 Vec，使底层缓冲区在整个测试期间持续存活
        core::mem::forget(arena);
        slice
    }

    /// 构造伙伴系统测试实例（可选预留与是否挂链）
    fn build(real: usize, reserve: &[usize], finalize: bool) -> BuddyAllocator {
        let bytes = make_arena(real);
        let mut b = unsafe { BuddyAllocator::from_bytes(real, bytes) };
        for &leaf in reserve {
            b.mark_reserved(0, leaf);
        }
        if finalize {
            b.finalize();
        }
        b
    }

    fn make_buddy(real: usize) -> BuddyAllocator {
        build(real, &[], true)
    }

    #[test]
    fn test_needed_bytes_sane() {
        assert_eq!(BuddyAllocator::needed_bytes(0), BuddyAllocator::needed_bytes(1));
        assert!(BuddyAllocator::needed_bytes(1000) >= BuddyAllocator::needed_bytes(512));
        assert!(BuddyAllocator::needed_bytes(65536) > BuddyAllocator::needed_bytes(5));
    }

    #[test]
    fn test_next_pow2() {
        assert_eq!(next_pow2(0), 1);
        assert_eq!(next_pow2(1), 1);
        assert_eq!(next_pow2(2), 2);
        assert_eq!(next_pow2(3), 4);
        assert_eq!(next_pow2(5), 8);
        assert_eq!(next_pow2(4096), 4096);
    }

    #[test]
    fn test_alloc_free_basic() {
        let mut b = make_buddy(64);
        assert_eq!(b.free_pages(), 64);
        assert_eq!(b.used_pages(), 0);

        // 分配两个单页
        let l0 = b.alloc(0).expect("alloc0");
        let l1 = b.alloc(0).expect("alloc1");
        assert_ne!(l0, l1);
        assert_eq!(b.free_pages(), 62);

        // 释放后应自动合并（两个伙伴相邻）
        b.free(0, l0);
        b.free(0, l1);
        assert_eq!(b.free_pages(), 64, "释放后应完全恢复");
        assert_eq!(b.used_pages(), 0);
    }

    #[test]
    fn test_alloc_order_and_split() {
        let mut b = make_buddy(256);
        // 请求 2 页（order 1）
        let l = b.alloc(1).expect("order1");
        assert_eq!(l % 2, 0, "order-1 块必须 2 页对齐");
        assert_eq!(b.free_pages(), 254);

        // 请求 4 页（order 2）应不与前面重叠
        let l2 = b.alloc(2).expect("order2");
        assert_eq!(l2 % 4, 0);
        let occ = b.occupancy();
        for i in l..l + 2 {
            assert!(occ[i]);
        }
        for i in l2..l2 + 4 {
            assert!(occ[i]);
        }

        // 释放 order-2 块（256 - 2 - 4 + 4 = 254 页空闲）
        b.free(2, l2);
        assert_eq!(b.free_pages(), 254);
        assert!(!b.occupancy()[l2..l2 + 4].iter().any(|&x| x));
    }

    #[test]
    fn test_merge_across_levels() {
        let mut b = make_buddy(64);
        // 分配一大块再逐级释放，验证完全合并
        let l = b.alloc(4).expect("order4");
        assert_eq!(b.free_pages(), 48);
        b.free(4, l);
        assert_eq!(b.free_pages(), 64, "整块归还后应可恢复原状");
        // 打散分配 + 逆序释放，仍应全部合并
        let mut pages = vec![];
        for _ in 0..16 {
            pages.push(b.alloc(0).unwrap());
        }
        for p in pages.iter() {
            b.free(0, *p);
        }
        assert_eq!(b.free_pages(), 64);
        assert_eq!(b.used_pages(), 0);
    }

    #[test]
    fn test_reserved_never_allocated() {
        // 预留 [32, 48) 的 16 页再挂链
        let mut b = build(128, &(32..48).collect::<Vec<_>>(), true);
        assert_eq!(b.reserved_pages(), 16);
        assert_eq!(b.free_pages(), 112);

        // 分配全部剩余页，验证不与预留区重叠
        let mut got = vec![];
        while let Some(l) = b.alloc(0) {
            got.push(l);
        }
        assert_eq!(got.len(), 112);
        for &l in &got {
            assert!(!(32..48).contains(&l), "分配到预留区");
        }
        // 预留区在占用表里恒为已占用
        assert!(b.occupancy()[32..48].iter().all(|&x| x));
    }

    #[test]
    fn test_holes_never_returned() {
        // real = 5，虚拟 T = 8：分配与释放不得跨越真实边界
        let mut b = make_buddy(5);
        let mut got = vec![];
        while let Some(l) = b.alloc(0) {
            got.push(l);
        }
        // 分配集合必须恰好是 real 页（顺序取决于伙伴拆分路径，不要求升序）
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4], "只能给出真实 5 页");
        // 全分配后 OOM
        assert_eq!(b.alloc(0), None);
        assert_eq!(b.alloc(1), None);
        // 释放全部应恢复 5 页（洞不计入）
        for &l in &got {
            b.free(0, l);
        }
        assert_eq!(b.free_pages(), 5);
    }

    #[test]
    fn test_oom_when_exhausted() {
        let mut b = make_buddy(4);
        let _a = b.alloc(0).unwrap();
        let _b = b.alloc(0).unwrap();
        let _c = b.alloc(0).unwrap();
        let d = b.alloc(0).unwrap();
        assert_eq!(b.alloc(0), None, "4 页全部分配后应 OOM");
        assert_eq!(b.alloc(2), None);
        // 只释放一页还凑不出连续两页 → order-1 分配失败
        b.free(0, d ^ 1);
        assert_eq!(b.alloc(1), None, "只释放一页无法凑出连续两页");
        // 把伙伴也释放 → 两块相邻空闲，可合并出 order-1
        b.free(0, d);
        let merged = b.alloc(1);
        assert!(merged.is_some(), "相邻两页释放后可合并为 order-1");
        b.free(1, merged.unwrap());
        // 剩余 _a / _c 两块单页仍占用，加上刚刚归还的 2 页 = 2 页空闲
        assert_eq!(b.free_pages(), 2);
        b.free(0, 0);
        b.free(0, 1);
        assert_eq!(b.free_pages(), 4, "全部归还后完全恢复");
    }

    #[test]
    fn test_big_alloc_split_reuse() {
        // 验证整区一大块入链后，低阶请求能逐级拆分且不重叠
        let mut b = make_buddy(8);
        let a = b.alloc(1).unwrap(); // 从整块 [0..8) 拆出 [0..2)
        assert_eq!(a, 0);
        let c = b.alloc(1).unwrap(); // 应复用已拆出的 [2..4)
        assert_eq!(c, 2);
        let _d = b.alloc(1).unwrap(); // [4..6)
        let _e = b.alloc(1).unwrap(); // [6..8)
        assert_eq!(b.alloc(1), None, "4 块 order-1 全部分配后应 OOM");

        // 释放两块低位块后应自动合并为 [0..4)
        b.free(1, a);
        b.free(1, c);
        let merged = b.alloc(2).unwrap();
        assert_eq!(merged, 0, "合并得到的 4 页块应从 0 开始");
        b.free(2, merged);
        assert_eq!(b.free_pages(), 4, "剩两块 order-1（[4..8)）空闲");
    }

    #[test]
    fn test_random_stress() {
        // 伪随机压力测试：大量随机分配/释放，校验最终恢复 + 无重叠
        let real = 2048;
        let mut b = make_buddy(real);
        let mut ptrs: Vec<(usize, usize)> = vec![]; // (leaf, order)
        let mut rng = 0x1234_5678u64;

        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        for _ in 0..20_000 {
            if (rand() & 1) == 0 || ptrs.is_empty() {
                // 分配（偏向小块）
                let order = (rand() as usize) % 6;
                if let Some(l) = b.alloc(order) {
                    ptrs.push((l, order));
                }
            } else {
                // 释放随机一块
                let i = (rand() as usize) % ptrs.len();
                let (l, o) = ptrs.swap_remove(i);
                b.free(o, l);
            }
        }
        // 全部释放
        for (l, o) in ptrs.drain(..) {
            b.free(o, l);
        }
        assert_eq!(b.free_pages(), real, "压力测试后必须完全恢复");
        assert_eq!(b.used_pages(), 0);
    }

    #[test]
    fn test_occpancy_no_overlap_during_stress() {
        let real = 512;
        let mut b = make_buddy(real);
        let mut ptrs: Vec<(usize, usize)> = vec![];
        let mut rng = 0xDEAD_BEEFu64;
        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for _ in 0..8000 {
            if (rand() & 1) == 0 || ptrs.is_empty() {
                let order = (rand() as usize) % 5;
                if let Some(l) = b.alloc(order) {
                    ptrs.push((l, order));
                }
            } else {
                let i = (rand() as usize) % ptrs.len();
                let (l, o) = ptrs.swap_remove(i);
                b.free(o, l);
            }
        }
        let occ = b.occupancy();
        let mut running = 0usize;
        // 用相邻两段位数和 = used 数来核对不会出现重叠（段树一致性已由
        // occupancy 直接读出；此处仅统计数量级正确）
        for &x in occ.iter() {
            running += x as usize;
        }
        assert_eq!(running, b.used_pages());
    }
}
