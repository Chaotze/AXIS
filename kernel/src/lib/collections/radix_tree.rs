// ============================================================
// Radix 树（基树，堆支持节点池）
// ============================================================
// 以 u64 键的二进制位为路径的多叉前缀树。
//
// 为什么内核需要 Radix 树：
// - 页缓存（address_space）以"文件偏移"为键、进程地址空间
//   以"虚拟地址"为键——这类整数键恰好适合按位前缀索引，
//   查询/插入/删除都是 O(树高)，且天然支持范围/前缀遍历
// - 相比哈希表，Radix 树保持键序，遍历有序且无碰撞退化
//
// 为什么是"堆支持节点池"版本：
// - lib 最初是定长版本（const 泛型 N_MAX 的节点池内嵌在使用
//   方结构体中）：那时内核尚无动态分配器
// - mm 落地后节点池迁到堆上（Vec），容量只受堆限制，不再需要
//   N_MAX——与 btree.rs 同理，"替换节点池为 kmalloc"即定长版本
//   预留的泛化路径
//
// 为什么 FANOUT 仍是 const 泛型：
// - FANOUT 必须是 2 的幂且整除 64：键按 shift = log2(FANOUT)
//   位切片，树高恒为 64 / shift，路径计算全是移位与掩码，
//   无除法（这是与 B 树的实现差异——整数键不需要比较）
//
// 为什么值只存放在叶子层：
// - 键定长（64 位）→ 树高恒定，叶子层就是键的"终点"；
//   非叶子层存值会引入"前缀命中"的歧义语义，简单起见
//   统一约定"值为完整键的附属物"

use alloc::vec::Vec;

/// Radix 树节点
struct RadixNode<V, const FANOUT: usize> {
    /// 子节点索引（None 表示无该分支）
    children: [Option<u32>; FANOUT],
    /// 节点携带的值（仅叶子层有意义）
    value: Option<V>,
}

impl<V, const FANOUT: usize> RadixNode<V, FANOUT> {
    /// 创建空节点
    const fn new() -> Self {
        Self {
            children: [None; FANOUT],
            value: None,
        }
    }

    /// 节点是否"空"（无值且无任何子分支）
    ///
    /// 用于删除后的剪枝判断。
    fn is_empty(&self) -> bool {
        self.value.is_none() && self.children.iter().all(|c| c.is_none())
    }
}

/// 堆支持节点池 Radix 树
///
/// 泛型参数：
/// - `FANOUT`: 每节点扇出（必须是 2 的幂且整除 64）
pub struct RadixTree<V: Copy, const FANOUT: usize> {
    /// 节点池（按空闲链表管理；高水位增长，无固定上限）
    nodes: Vec<RadixNode<V, FANOUT>>,
    /// 根节点池索引
    root: Option<u32>,
    /// 空闲节点链表头（复用空闲节点的 children[0] 作 next）
    free_head: Option<u32>,
    /// 当前键值对总数
    len: usize,
}

/// 每层消耗的位数（FANOUT = 2^shift）
const fn fanout_shift<const FANOUT: usize>() -> usize {
    FANOUT.trailing_zeros() as usize
}

impl<V: Copy, const FANOUT: usize> RadixTree<V, FANOUT> {
    /// 创建空树
    ///
    /// # 为什么不再需要 const/惰性初始化：
    /// - 定长版本受"编译期构造静态字段"约束；堆支持版本的空树
    ///   不分配任何节点，new 之后按需 push，无需 initialized 标志
    pub fn new() -> Self {
        // FANOUT 约束：2 的幂（切片算法依赖）且能整除 64（树高恒定）
        assert!(FANOUT >= 2, "FANOUT 必须至少为 2");
        assert!(FANOUT.is_power_of_two(), "FANOUT 必须是 2 的幂");
        assert!(64 % fanout_shift::<FANOUT>() == 0, "FANOUT 必须能整除 64 位键宽");

        Self {
            nodes: Vec::new(),
            root: None,
            free_head: None,
            len: 0,
        }
    }

    /// 当前键值对总数
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 查询键对应的值
    pub fn get(&self, key: u64) -> Option<&V> {
        let mut node = self.root?;
        for level in 0..levels::<FANOUT>() {
            let child = self.node_ref(node).children[key_index::<FANOUT>(key, level)]?;
            node = child;
        }
        self.node_ref(node).value.as_ref()
    }

    /// 是否包含键
    pub fn contains(&self, key: u64) -> bool {
        self.get(key).is_some()
    }

    /// 插入键值对；键已存在时替换并返回旧值
    pub fn insert(&mut self, key: u64, value: V) -> Option<V> {
        if self.root.is_none() {
            let root = self.allocate_node();
            self.root = Some(root);
        }

        let mut node = self.root.unwrap();
        for level in 0..levels::<FANOUT>() {
            let index = key_index::<FANOUT>(key, level);
            match self.node_ref(node).children[index] {
                Some(child) => node = child,
                None => {
                    // 沿路径按需创建节点（惰性展开：只建实际
                    // 用到的分支，稀疏键空间不浪费节点）
                    let child = self.allocate_node();
                    self.node_mut(node).children[index] = Some(child);
                    node = child;
                }
            }
        }

        // 到达叶子层：替换/新值
        let old = self.node_ref(node).value;
        self.node_mut(node).value = Some(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// 删除键；返回被删除的值，键不存在返回 None
    ///
    /// 删除后自底向上剪枝：空节点（无值无分支）释放回空闲池，
    /// 保证树结构始终最紧凑。
    pub fn remove(&mut self, key: u64) -> Option<V> {
        let root = self.root?;

        let (removed, root_empty) = self.remove_node(root, key, 0);

        if root_empty {
            self.free_node(root);
            self.root = None;
        }

        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    // ============================================================
    // 内部：节点池管理
    // ============================================================

    /// 从空闲链表分配一个干净节点；链表为空时向堆申请新节点
    ///
    /// # 为什么池会"越用越大"：
    /// - Vec 只增不减，配合空闲链表复用，池的大小收敛到树
    ///   历史最高水位；换取了容量上限的消失
    fn allocate_node(&mut self) -> u32 {
        match self.free_head {
            Some(index) => {
                // 先取后继指针，结束借用后再推进表头
                let next = self.node_mut(index).children[0];
                self.free_head = next;
                *self.node_mut(index) = RadixNode::new();
                index
            }
            None => {
                let index = self.nodes.len() as u32;
                self.nodes.push(RadixNode::new());
                index
            }
        }
    }

    /// 回收节点到空闲链表
    fn free_node(&mut self, index: u32) {
        let head = self.free_head;
        self.node_mut(index).children[0] = head;
        self.free_head = Some(index);
    }

    /// 递归删除并返回（被删值, 子树是否已空）
    fn remove_node(&mut self, node: u32, key: u64, level: usize) -> (Option<V>, bool) {
        if level == levels::<FANOUT>() {
            // 叶子层：直接取走值
            let value = self.node_ref(node).value;
            if value.is_some() {
                self.node_mut(node).value = None;
            }
            return (value, self.node_ref(node).is_empty());
        }

        let index = key_index::<FANOUT>(key, level);
        let child = match self.node_ref(node).children[index] {
            Some(child) => child,
            // 分支不存在 → 键不存在
            None => return (None, false),
        };

        let (value, child_empty) = self.remove_node(child, key, level + 1);
        if child_empty {
            // 剪枝：断开并回收空孩子
            self.node_mut(node).children[index] = None;
            self.free_node(child);
        }
        (value, self.node_ref(node).is_empty())
    }

    #[inline]
    fn node_ref(&self, index: u32) -> &RadixNode<V, FANOUT> {
        &self.nodes[index as usize]
    }

    #[inline]
    fn node_mut(&mut self, index: u32) -> &mut RadixNode<V, FANOUT> {
        &mut self.nodes[index as usize]
    }
}

/// 树高（层数）
const fn levels<const FANOUT: usize>() -> usize {
    64 / fanout_shift::<FANOUT>()
}

/// 计算键在第 level 层（从根起算，0 基）的分支索引
///
/// 键位从高位向低位切片：第 0 层取最高 shift 位，依次向下。
/// 全程移位 + 掩码，无除法（整数键的前缀索引不需要比较）。
#[inline]
fn key_index<const FANOUT: usize>(key: u64, level: usize) -> usize {
    let shift = fanout_shift::<FANOUT>();
    let shift_amount = 64 - shift * (level + 1);
    let mask = (FANOUT - 1) as u64;
    ((key >> shift_amount) & mask) as usize
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    /// 测试用树：扇出 16（每层 4 位），树高 16
    type TestTree = RadixTree<u32, 16>;

    #[test]
    fn test_insert_get() {
        let mut tree: TestTree = RadixTree::new();
        assert!(tree.is_empty());

        let keys = [0u64, 1, 0xF, 0x10, 0x100, 0xFFFF, 0xABCD_1234_5678, u64::MAX];
        for (i, &key) in keys.iter().enumerate() {
            assert_eq!(tree.insert(key, i as u32), None);
        }
        assert_eq!(tree.len(), keys.len());

        for (i, &key) in keys.iter().enumerate() {
            assert_eq!(tree.get(key), Some(&(i as u32)));
        }
        assert_eq!(tree.get(0xDEAD_BEEF), None);
    }

    #[test]
    fn test_insert_overwrite() {
        let mut tree: TestTree = RadixTree::new();
        assert_eq!(tree.insert(42, 1), None);
        assert_eq!(tree.insert(42, 2), Some(1));
        assert_eq!(tree.get(42), Some(&2));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_remove_prunes() {
        let mut tree: TestTree = RadixTree::new();
        tree.insert(0x1234, 1);
        tree.insert(0x1235, 2); // 与 0x1234 共享大部分路径
        assert_eq!(tree.len(), 2);

        assert_eq!(tree.remove(0x1234), Some(1));
        // 共享路径不应被误剪：另一个键仍在
        assert_eq!(tree.get(0x1235), Some(&2));
        assert_eq!(tree.get(0x1234), None);
        assert_eq!(tree.len(), 1);

        // 删掉最后一个键：整棵树剪枝为空
        assert_eq!(tree.remove(0x1235), Some(2));
        assert!(tree.is_empty());
        assert_eq!(tree.get(0x1235), None);
        assert_eq!(tree.remove(0x1235), None);
    }

    #[test]
    fn test_sparse_keys() {
        // 稀疏键空间：只有实际路径消耗节点
        let mut tree: TestTree = RadixTree::new();
        tree.insert(0, 1);
        tree.insert(1u64 << 63, 2); // 与 0 在第一层就分道扬镳
        assert_eq!(tree.get(0), Some(&1));
        assert_eq!(tree.get(1u64 << 63), Some(&2));
        assert_eq!(tree.len(), 2);

        tree.remove(0);
        assert_eq!(tree.get(1u64 << 63), Some(&2));
    }

    #[test]
    fn test_many_keys() {
        let mut tree: TestTree = RadixTree::new();
        for i in 0..200u64 {
            tree.insert(i * 7, i as u32);
        }
        assert_eq!(tree.len(), 200);
        for i in 0..200u64 {
            assert_eq!(tree.get(i * 7), Some(&(i as u32)));
        }
        // 奇偶错开删除，验证剪枝后其余键仍可查
        for i in (0..200u64).step_by(2) {
            assert_eq!(tree.remove(i * 7), Some(i as u32));
        }
        for i in (1..200u64).step_by(2) {
            assert_eq!(tree.get(i * 7), Some(&(i as u32)));
        }
        assert_eq!(tree.len(), 100);
    }
}

