// ============================================================
// B 树（堆支持节点池）
// ============================================================
// 实现经典 B 树映射：有序键值对、所有叶子同深度、节点键数
// 介于 [MIN_KEYS, K_MAX] 之间（根例外）。
//
// 为什么内核需要 B 树：
// - 文件系统目录索引、内存数据库式结构等需要"有序 + 高效
//   范围查询 + 稳定树高"的场景；树高 O(log N) 保证最坏
//   情况下的查询步数，这对内核的可预测性很重要
//
// 为什么是"堆支持节点池"版本：
// - lib 最初是定长版本（const 泛型 K_MAX/C_MAX/N_MAX，节点池
//   内嵌在使用方结构体中）：那时内核尚无动态分配器
// - mm 落地后节点池迁到堆上（Vec），容量只受堆限制，不再需要
//   N_MAX——"替换节点池为 kmalloc 即可"正是当初定长版本预留的
//   泛化路径；K_MAX/C_MAX 仍为 const 泛型（节点形状编译期确定）
//
// 为什么节点仍用 u32 下标而非指针：
// - Vec 重分配会搬移节点但不会改变下标，树内持久引用的下标
//   永不失效，且单次访问都重新借用（借用在单条语句内结束），
//   从定长数组池迁移过来后不变式原封不动
//
// 为什么 const 泛型参数是 K_MAX / C_MAX 而非"阶数 t"：
// - Rust 稳定版不支持对 const 泛型做算术（2t-1）作为数组
//   长度，因此显式传入"每节点最大键数/最大子节点数"，
//   并通过编译期断言强制 C_MAX == K_MAX + 1、K_MAX 为奇数
//
// 为什么键值要求 Copy：
// - 节点槽位是未初始化内存（MaybeUninit），Copy 约束让
//   键值在移位/分裂/合并时可以直接按位搬移，无需处理
//   析构时序；内核的键值几乎都是字长标量（地址、索引、
//   指针），非 Copy 值可用指针/句柄间接存储
//
// 为什么内部实现用 &mut self 贯穿递归而不是 &self + 裸指针：
// - 单线程拥有权贯穿全部递归路径，天然无别名，安全性
//   显著优于"&self 返回 &mut"的常见内核写法

use alloc::vec::Vec;
use core::mem::MaybeUninit;

/// B 树节点
struct BTreeNode<K: Ord, V, const K_MAX: usize, const C_MAX: usize> {
    /// 键数组（仅前 key_count 个槽位有效）
    keys: MaybeUninit<[K; K_MAX]>,
    /// 值数组（与键一一对应）
    vals: MaybeUninit<[V; K_MAX]>,
    /// 子节点数组（内部节点有效；空闲节点复用 [0] 作空闲链 next）
    children: [Option<u32>; C_MAX],
    /// 当前键数
    key_count: usize,
    /// 是否为叶子
    is_leaf: bool,
}

impl<K: Ord, V, const K_MAX: usize, const C_MAX: usize> BTreeNode<K, V, K_MAX, C_MAX> {
    /// 创建空节点
    const fn new(is_leaf: bool) -> Self {
        Self {
            keys: MaybeUninit::uninit(),
            vals: MaybeUninit::uninit(),
            children: [None; C_MAX],
            key_count: 0,
            is_leaf,
        }
    }
}

/// 堆支持节点池 B 树映射
///
/// 泛型参数：
/// - `K_MAX`: 每节点最大键数（必须为奇数，≥3）
/// - `C_MAX`: 每节点最大子节点数（必须等于 K_MAX + 1）
pub struct BTreeMap<K: Ord + Copy, V: Copy, const K_MAX: usize, const C_MAX: usize> {
    /// 节点池（按空闲链表管理；高水位增长，无固定上限）
    nodes: Vec<BTreeNode<K, V, K_MAX, C_MAX>>,
    /// 根节点池下标
    root: Option<u32>,
    /// 空闲节点链表头（复用空闲节点的 children[0] 作 next）
    free_head: Option<u32>,
    /// 键值对总数
    len: usize,
}

/// 节点最少键数（阶数 t 的 t-1；K_MAX = 2t-1）
const fn min_keys<const K_MAX: usize>() -> usize {
    K_MAX / 2
}

impl<K: Ord + Copy, V: Copy, const K_MAX: usize, const C_MAX: usize>
    BTreeMap<K, V, K_MAX, C_MAX>
{
    /// 创建空树
    ///
    /// # 为什么不再需要 const/惰性初始化：
    /// - 定长版本受"编译期构造静态字段"约束，节点池必须
    ///   惰性初始化；堆支持版本的空树不分配任何节点，
    ///   new 之后按需 push，无需 initialized 标志
    pub fn new() -> Self {
        // 运行期校验 const 泛型参数的约束关系
        assert!(K_MAX >= 3, "K_MAX 必须至少为 3（阶数 t >= 2）");
        assert!(K_MAX % 2 == 1, "K_MAX 必须为奇数，合并操作才能不溢出");
        assert!(C_MAX == K_MAX + 1, "C_MAX 必须等于 K_MAX + 1");

        Self {
            nodes: Vec::new(),
            root: None,
            free_head: None,
            len: 0,
        }
    }

    /// 键值对总数
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
    pub fn get(&self, key: &K) -> Option<&V> {
        let mut node = self.root?;
        loop {
            // 线性查找第一个 >= key 的键位置
            // 为什么用线性查找：K_MAX 通常很小（几个到几十个），
            // 线性查找缓存友好且实现简单，与二分查找差异可忽略
            let count = self.key_count(node);
            let mut i = 0;
            while i < count && self.key_at(node, i) < *key {
                i += 1;
            }

            if i < count && self.key_at(node, i) == *key {
                return Some(self.val_ref(node, i));
            }
            if self.is_leaf(node) {
                return None;
            }
            node = self.child_at(node, i)?;
        }
    }

    /// 是否包含键
    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// 插入键值对；键已存在时替换并返回旧值
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.root {
            None => {
                // 空树：新建根节点
                let root = self.allocate_node();
                self.root = Some(root);
                self.write_key_val(root, 0, key, value);
                self.set_key_count(root, 1);
                self.len += 1;
                None
            }
            Some(root) => {
                // 根满：先分裂根（树长高一层），这是 B 树
                // "只从根向下生长"不变式的标准做法
                if self.key_count(root) == K_MAX {
                    let new_root = self.allocate_node();
                    self.set_child(new_root, 0, Some(root));
                    self.set_leaf(new_root, false);
                    self.set_key_count(new_root, 0);
                    self.root = Some(new_root);
                    self.split_child(new_root, 0);
                }
                let old = self.insert_nonfull(self.root.unwrap(), key, value);
                if old.is_none() {
                    self.len += 1;
                }
                old
            }
        }
    }

    /// 删除键；返回被删除的值，键不存在返回 None
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let root = self.root?;

        let removed = self.delete_from(root, *key);

        // 根被借空：释放根并把唯一的孩子提升为新根（树变矮）
        if let Some(r) = self.root {
            if self.key_count(r) == 0 {
                let only_child = self.child_at(r, 0);
                self.free_node(r);
                self.root = only_child;
            }
        }

        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    /// 按中序遍历所有键值对（键升序）
    ///
    /// 为什么用闭包遍历而不是返回迭代器：
    /// - 递归遍历需要显式栈或深度递归；闭包形式让调用方
    ///   自行决定收集/处理方式，遍历本身零分配
    pub fn visit(&self, f: &mut impl FnMut(&K, &V)) {
        if let Some(root) = self.root {
            self.visit_node(root, f);
        }
    }

    // ============================================================
    // 内部：节点池管理
    // ============================================================

    /// 从空闲链表分配一个节点；链表为空时向堆申请新节点
    ///
    /// # 为什么池会"越用越大"：
    /// - Vec 只增不减，配合空闲链表复用，池的大小收敛到树
    ///   历史最高水位；这是"动态池"与"定长池"的成本差异——
    ///   换取了容量上限的消失
    fn allocate_node(&mut self) -> u32 {
        match self.free_head {
            Some(index) => {
                // 先取后继指针，结束借用后再推进表头
                let next = self.node_mut(index).children[0];
                self.free_head = next;
                *self.node_mut(index) = BTreeNode::new(true);
                index
            }
            None => {
                let index = self.nodes.len() as u32;
                self.nodes.push(BTreeNode::new(true));
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

    // ============================================================
    // 内部：节点字段访问
    // ============================================================

    #[inline]
    fn node_ref(&self, index: u32) -> &BTreeNode<K, V, K_MAX, C_MAX> {
        &self.nodes[index as usize]
    }

    #[inline]
    fn node_mut(&mut self, index: u32) -> &mut BTreeNode<K, V, K_MAX, C_MAX> {
        &mut self.nodes[index as usize]
    }

    #[inline]
    fn key_count(&self, node: u32) -> usize {
        self.node_ref(node).key_count
    }

    #[inline]
    fn set_key_count(&mut self, node: u32, count: usize) {
        self.node_mut(node).key_count = count;
    }

    #[inline]
    fn is_leaf(&self, node: u32) -> bool {
        self.node_ref(node).is_leaf
    }

    #[inline]
    fn set_leaf(&mut self, node: u32, leaf: bool) {
        self.node_mut(node).is_leaf = leaf;
    }

    #[inline]
    fn key_at(&self, node: u32, i: usize) -> K {
        unsafe { (self.node_ref(node).keys.as_ptr() as *const K).add(i).read() }
    }

    #[inline]
    fn val_at(&self, node: u32, i: usize) -> V {
        unsafe { (self.node_ref(node).vals.as_ptr() as *const V).add(i).read() }
    }

    #[inline]
    fn val_ref(&self, node: u32, i: usize) -> &V {
        unsafe { &*(self.node_ref(node).vals.as_ptr() as *const V).add(i) }
    }

    #[inline]
    fn write_key_val(&mut self, node: u32, i: usize, key: K, value: V) {
        let n = self.node_mut(node);
        unsafe {
            (n.keys.as_mut_ptr() as *mut K).add(i).write(key);
            (n.vals.as_mut_ptr() as *mut V).add(i).write(value);
        }
    }

    #[inline]
    fn child_at(&self, node: u32, i: usize) -> Option<u32> {
        self.node_ref(node).children[i]
    }

    #[inline]
    fn set_child(&mut self, node: u32, i: usize, child: Option<u32>) {
        self.node_mut(node).children[i] = child;
    }

    // ============================================================
    // 内部：插入路径
    // ============================================================

    /// 向非满节点插入（键已存在时替换值）
    ///
    /// 返回 Some(旧值) 表示替换，None 表示新插入。
    fn insert_nonfull(&mut self, node: u32, key: K, value: V) -> Option<V> {
        let count = self.key_count(node);
        let mut i = 0;
        while i < count && self.key_at(node, i) < key {
            i += 1;
        }

        // 键已存在：替换值
        if i < count && self.key_at(node, i) == key {
            let old = self.val_at(node, i);
            self.write_key_val(node, i, key, value);
            return Some(old);
        }

        if self.is_leaf(node) {
            // 叶子：腾出位置并插入
            for j in (i..count).rev() {
                let k = self.key_at(node, j);
                let v = self.val_at(node, j);
                self.write_key_val(node, j + 1, k, v);
            }
            self.write_key_val(node, i, key, value);
            self.set_key_count(node, count + 1);
            return None;
        }

        // 内部节点：下降前若目标孩子已满则先分裂，
        // 保证递归插入永远不会遇到满节点
        let child = self.child_at(node, i).unwrap();
        if self.key_count(child) == K_MAX {
            self.split_child(node, i);
            // 分裂后父节点多了中间键，比较决定走左还是右孩子
            if key > self.key_at(node, i) {
                i += 1;
            }
        }
        self.insert_nonfull(self.child_at(node, i).unwrap(), key, value)
    }

    /// 分裂父节点第 i 个满孩子
    ///
    /// 满孩子（K_MAX 键）被拆成两个各 MIN_KEYS 键的节点，
    /// 中间键提升到父节点。这是 B 树高度增长的唯一途径。
    fn split_child(&mut self, parent: u32, i: usize) {
        let child = self.child_at(parent, i).unwrap();
        let sibling = self.allocate_node();
        let mid = min_keys::<K_MAX>(); // 中间键下标（K_MAX = 2*mid+1）

        // 兄弟承接右半部分键值
        self.set_leaf(sibling, self.is_leaf(child));
        let right_half = K_MAX - mid - 1;
        for j in 0..right_half {
            let k = self.key_at(child, mid + 1 + j);
            let v = self.val_at(child, mid + 1 + j);
            self.write_key_val(sibling, j, k, v);
        }
        self.set_key_count(sibling, right_half);

        // 内部节点还要搬移右半部分孩子
        if !self.is_leaf(child) {
            for j in 0..=right_half {
                let c = self.child_at(child, mid + 1 + j);
                self.set_child(sibling, j, c);
            }
        }
        self.set_key_count(child, mid);

        // 父节点腾位置：键 i.. 右移，孩子 i+1.. 右移
        let parent_count = self.key_count(parent);
        for j in (i..parent_count).rev() {
            let k = self.key_at(parent, j);
            let v = self.val_at(parent, j);
            self.write_key_val(parent, j + 1, k, v);
        }
        for j in (i + 1..=parent_count).rev() {
            let c = self.child_at(parent, j);
            self.set_child(parent, j + 1, c);
        }

        // 中间键提升
        let mid_key = self.key_at(child, mid);
        let mid_val = self.val_at(child, mid);
        self.write_key_val(parent, i, mid_key, mid_val);
        self.set_child(parent, i + 1, Some(sibling));
        self.set_key_count(parent, parent_count + 1);
    }

    // ============================================================
    // 内部：删除路径
    // ============================================================

    /// 从节点递归删除键
    ///
    /// 删除路径的关键不变式：进入递归时，当前节点（除根）
    /// 的键数严格大于 MIN_KEYS，保证删除后不会下溢。
    fn delete_from(&mut self, node: u32, key: K) -> Option<V> {
        let count = self.key_count(node);
        let mut i = 0;
        while i < count && self.key_at(node, i) < key {
            i += 1;
        }

        // 情况 1：键在本节点
        if i < count && self.key_at(node, i) == key {
            if self.is_leaf(node) {
                // 叶子：直接移除（后续元素左移覆盖）
                let removed = self.val_at(node, i);
                for j in i..count - 1 {
                    let k = self.key_at(node, j + 1);
                    let v = self.val_at(node, j + 1);
                    self.write_key_val(node, j, k, v);
                }
                self.set_key_count(node, count - 1);
                return Some(removed);
            }

            // 内部节点：用前驱/后继替换，或在合并后继续删除
            let original = self.val_at(node, i);
            let left = self.child_at(node, i).unwrap();
            let right = self.child_at(node, i + 1).unwrap();

            if self.key_count(left) > min_keys::<K_MAX>() {
                // 2a：左子树键充足，用前驱（左子树最大键）替换
                let (pk, pv) = self.extract_max(left);
                self.write_key_val(node, i, pk, pv);
                return Some(original);
            } else if self.key_count(right) > min_keys::<K_MAX>() {
                // 2b：右子树键充足，用后继（右子树最小键）替换
                let (sk, sv) = self.extract_min(right);
                self.write_key_val(node, i, sk, sv);
                return Some(original);
            } else {
                // 2c：两侧都只有 MIN 键，合并两孩子后继续删除
                self.merge_children(node, i);
                let merged = self.child_at(node, i).unwrap();
                return self.delete_from(merged, key);
            }
        }

        // 键不在本节点
        if self.is_leaf(node) {
            return None; // 树中不存在
        }

        // 情况 3：下降前保证目标孩子键数 > MIN_KEYS
        let mut child_index = i;
        let child = self.child_at(node, child_index).unwrap();
        if self.key_count(child) == min_keys::<K_MAX>() {
            if child_index > 0 {
                let left_sib = self.child_at(node, child_index - 1).unwrap();
                if self.key_count(left_sib) > min_keys::<K_MAX>() {
                    // 3a：向左兄弟借
                    self.borrow_from_left(node, child_index);
                } else {
                    // 3b：与左兄弟合并，改从合并节点下降
                    self.merge_children(node, child_index - 1);
                    child_index -= 1;
                }
            } else {
                let right_sib = self.child_at(node, child_index + 1).unwrap();
                if self.key_count(right_sib) > min_keys::<K_MAX>() {
                    // 3a'：向右兄弟借
                    self.borrow_from_right(node, child_index);
                } else {
                    // 3b'：与右兄弟合并
                    self.merge_children(node, child_index);
                }
            }
        }
        self.delete_from(self.child_at(node, child_index).unwrap(), key)
    }

    /// 提取子树的最大键值（供"前驱替换"使用）
    ///
    /// 沿最右孩子下降，同样维持"进入节点键数 > MIN_KEYS"不变式。
    fn extract_max(&mut self, node: u32) -> (K, V) {
        let count = self.key_count(node);
        if self.is_leaf(node) {
            let k = self.key_at(node, count - 1);
            let v = self.val_at(node, count - 1);
            self.set_key_count(node, count - 1);
            return (k, v);
        }

        // 最右路径：下降前修复最右孩子（只有左兄弟可借/并）
        let mut child = self.child_at(node, count).unwrap();
        if self.key_count(child) == min_keys::<K_MAX>() {
            let left_sib = self.child_at(node, count - 1).unwrap();
            if self.key_count(left_sib) > min_keys::<K_MAX>() {
                self.borrow_from_left(node, count);
            } else {
                self.merge_children(node, count - 1);
            }
            // 借位不移动节点（孩子仍在索引 count）；
            // 合并后父节点少一键，合并结果落在索引 count-1。
            // 用 key_count 归一后取 min 即可同时覆盖两种情况。
            child = self.child_at(node, count.min(self.key_count(node))).unwrap();
        }
        self.extract_max(child)
    }

    /// 提取子树的最小键值（供"后继替换"使用）
    fn extract_min(&mut self, node: u32) -> (K, V) {
        let count = self.key_count(node);
        if self.is_leaf(node) {
            let k = self.key_at(node, 0);
            let v = self.val_at(node, 0);
            for j in 0..count - 1 {
                let k2 = self.key_at(node, j + 1);
                let v2 = self.val_at(node, j + 1);
                self.write_key_val(node, j, k2, v2);
            }
            self.set_key_count(node, count - 1);
            return (k, v);
        }

        // 最左路径：下降前修复最左孩子（只有右兄弟可借/并）
        let mut child = self.child_at(node, 0).unwrap();
        if self.key_count(child) == min_keys::<K_MAX>() {
            let right_sib = self.child_at(node, 1).unwrap();
            if self.key_count(right_sib) > min_keys::<K_MAX>() {
                self.borrow_from_right(node, 0);
            } else {
                self.merge_children(node, 0);
            }
            child = self.child_at(node, 0).unwrap();
        }
        self.extract_min(child)
    }

    /// 从右向左：把父键下沉到欠额孩子，左兄弟最大键上提
    fn borrow_from_left(&mut self, parent: u32, child_index: usize) {
        let child = self.child_at(parent, child_index).unwrap();
        let left = self.child_at(parent, child_index - 1).unwrap();
        let child_count = self.key_count(child);
        let left_count = self.key_count(left);

        // 孩子键值整体右移一格
        for j in (0..child_count).rev() {
            let k = self.key_at(child, j);
            let v = self.val_at(child, j);
            self.write_key_val(child, j + 1, k, v);
        }
        // 内部节点：孩子指针也右移，左兄弟末孩子过继到最前
        if !self.is_leaf(child) {
            for j in (0..=child_count).rev() {
                let c = self.child_at(child, j);
                self.set_child(child, j + 1, c);
            }
            let moved = self.child_at(left, left_count);
            self.set_child(child, 0, moved);
        }
        // 父键下沉到孩子最前
        let pk = self.key_at(parent, child_index - 1);
        let pv = self.val_at(parent, child_index - 1);
        self.write_key_val(child, 0, pk, pv);
        self.set_key_count(child, child_count + 1);

        // 左兄弟最大键上提到父
        let lk = self.key_at(left, left_count - 1);
        let lv = self.val_at(left, left_count - 1);
        self.write_key_val(parent, child_index - 1, lk, lv);
        self.set_key_count(left, left_count - 1);
    }

    /// 从左向右：把父键下沉到欠额孩子，右兄弟最小键上提
    fn borrow_from_right(&mut self, parent: u32, child_index: usize) {
        let child = self.child_at(parent, child_index).unwrap();
        let right = self.child_at(parent, child_index + 1).unwrap();
        let child_count = self.key_count(child);
        let right_count = self.key_count(right);

        // 父键下沉到孩子末尾
        let pk = self.key_at(parent, child_index);
        let pv = self.val_at(parent, child_index);
        self.write_key_val(child, child_count, pk, pv);
        self.set_key_count(child, child_count + 1);
        // 内部节点：右兄弟首孩子过继到孩子末尾
        if !self.is_leaf(child) {
            let moved = self.child_at(right, 0);
            self.set_child(child, child_count + 1, moved);
        }

        // 右兄弟最小键上提到父
        let rk = self.key_at(right, 0);
        let rv = self.val_at(right, 0);
        self.write_key_val(parent, child_index, rk, rv);
        // 右兄弟键值左移一格（覆盖被提走的键）
        for j in 0..right_count - 1 {
            let k = self.key_at(right, j + 1);
            let v = self.val_at(right, j + 1);
            self.write_key_val(right, j, k, v);
        }
        // 内部节点：右兄弟孩子左移一格
        if !self.is_leaf(child) {
            for j in 0..right_count {
                let c = self.child_at(right, j + 1);
                self.set_child(right, j, c);
            }
        }
        self.set_key_count(right, right_count - 1);
    }

    /// 合并父节点第 i 个与第 i+1 个孩子（父键 i 下沉为中间键）
    ///
    /// 前提：两个孩子都只有 MIN_KEYS 键；
    /// 合并后键数 = MIN + 1 + MIN = K_MAX（恰好不溢出）。
    fn merge_children(&mut self, parent: u32, i: usize) {
        let left = self.child_at(parent, i).unwrap();
        let right = self.child_at(parent, i + 1).unwrap();
        let left_count = self.key_count(left);
        let right_count = self.key_count(right);
        let parent_count = self.key_count(parent);

        // 父键下沉到左孩子末尾，成为中间键
        let pk = self.key_at(parent, i);
        let pv = self.val_at(parent, i);
        self.write_key_val(left, left_count, pk, pv);
        // 右孩子键值追加
        for j in 0..right_count {
            let k = self.key_at(right, j);
            let v = self.val_at(right, j);
            self.write_key_val(left, left_count + 1 + j, k, v);
        }
        // 内部节点：右孩子指针一并搬移
        if !self.is_leaf(left) {
            for j in 0..=right_count {
                let c = self.child_at(right, j);
                self.set_child(left, left_count + 1 + j, c);
            }
        }
        self.set_key_count(left, left_count + 1 + right_count);
        self.free_node(right);

        // 父节点：键 i 左移，孩子 i+1 左移
        for j in i..parent_count - 1 {
            let k = self.key_at(parent, j + 1);
            let v = self.val_at(parent, j + 1);
            self.write_key_val(parent, j, k, v);
        }
        for j in i + 1..parent_count {
            let c = self.child_at(parent, j + 1);
            self.set_child(parent, j, c);
        }
        self.set_child(parent, parent_count, None);
        self.set_key_count(parent, parent_count - 1);
    }

    // ============================================================
    // 内部：遍历
    // ============================================================

    /// 中序递归遍历（递归深度 = 树高 = O(log N)，堆栈有界）
    fn visit_node(&self, node: u32, f: &mut impl FnMut(&K, &V)) {
        let count = self.key_count(node);
        for i in 0..count {
            if !self.is_leaf(node) {
                if let Some(child) = self.child_at(node, i) {
                    self.visit_node(child, f);
                }
            }
            let k = self.key_at(node, i);
            let v = self.val_at(node, i);
            f(&k, &v);
        }
        if !self.is_leaf(node) {
            if let Some(child) = self.child_at(node, count) {
                self.visit_node(child, f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    /// 测试用树：阶数 t=2（2-3-4 树），K_MAX=3、C_MAX=4
    type TestTree = BTreeMap<u32, u32, 3, 4>;

    /// B 树结构不变量校验器：
    /// - 键严格递增且在 (min, max) 开区间内
    /// - 非根节点键数 >= MIN_KEYS
    /// - 所有叶子深度一致
    fn validate_tree<const K_MAX: usize, const C_MAX: usize>(tree: &BTreeMap<u32, u32, K_MAX, C_MAX>) {
        let mut leaf_depth: Option<usize> = None;

        fn walk<const K_MAX: usize, const C_MAX: usize>(
            tree: &BTreeMap<u32, u32, K_MAX, C_MAX>,
            node: u32,
            depth: usize,
            min: Option<u32>,
            max: Option<u32>,
            is_root: bool,
            leaf_depth: &mut Option<usize>,
        ) -> usize {
            let count = tree.key_count(node);
            if !is_root {
                assert!(count >= min_keys::<K_MAX>(), "非根节点键数下溢");
            }
            assert!(count <= K_MAX, "节点键数上溢");

            let mut prev: Option<u32> = None;
            for i in 0..count {
                let key = tree.key_at(node, i);
                if let Some(p) = prev {
                    assert!(p < key, "键必须严格递增");
                }
                prev = Some(key);
                if let Some(m) = min {
                    assert!(key > m, "键越出下界");
                }
                if let Some(m) = max {
                    assert!(key < m, "键越出上界");
                }
            }

            if tree.is_leaf(node) {
                if let Some(d) = *leaf_depth {
                    assert_eq!(d, depth, "叶子深度不一致");
                } else {
                    *leaf_depth = Some(depth);
                }
                return count;
            }

            // 内部节点：孩子数 = 键数 + 1，递归校验每棵子树
            assert!(count >= 1, "内部节点至少 1 个键");
            let mut total = 0;
            for i in 0..=count {
                let child = tree.child_at(node, i).expect("内部节点孩子缺失");
                let child_min = if i == 0 { min } else { Some(tree.key_at(node, i - 1)) };
                let child_max = if i == count { max } else { Some(tree.key_at(node, i)) };
                total += walk(tree, child, depth + 1, child_min, child_max, false, leaf_depth);
            }
            total + count
        }

        if let Some(root) = tree.root {
            let total = walk(tree, root, 0, None, None, true, &mut leaf_depth);
            assert_eq!(total, tree.len, "遍历计数与 len 不一致");
        } else {
            assert_eq!(tree.len, 0);
        }
    }

    #[test]
    fn test_insert_and_get() {
        let mut tree: TestTree = BTreeMap::new();
        assert!(tree.is_empty());

        for i in 0..16u32 {
            assert_eq!(tree.insert(i, i * 10), None);
        }
        assert_eq!(tree.len(), 16);
        assert!(!tree.is_empty());

        for i in 0..16u32 {
            assert_eq!(tree.get(&i), Some(&(i * 10)));
        }
        assert_eq!(tree.get(&16), None);
        assert!(tree.contains(&7));
        assert!(!tree.contains(&99));

        validate_tree(&tree);
    }

    #[test]
    fn test_insert_overwrite() {
        let mut tree: TestTree = BTreeMap::new();
        assert_eq!(tree.insert(5, 50), None);
        assert_eq!(tree.insert(5, 99), Some(50));
        assert_eq!(tree.get(&5), Some(&99));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_visit_sorted() {
        let mut tree: TestTree = BTreeMap::new();
        // 乱序插入
        for &k in &[7u32, 3, 15, 1, 9, 13, 5, 11, 2, 4, 6, 8, 10, 12, 14, 0] {
            tree.insert(k, k);
        }
        let mut keys: std::vec::Vec<u32> = std::vec::Vec::new();
        tree.visit(&mut |k, _| keys.push(*k));
        assert_eq!(keys, (0..16u32).collect::<std::vec::Vec<u32>>());
    }

    #[test]
    fn test_delete_all_paths() {
        let mut tree: TestTree = BTreeMap::new();
        for i in 0..32u32 {
            tree.insert(i, i);
        }
        validate_tree(&tree);

        // 先删偶数（触发借位/合并），每步校验结构
        for i in (0..32u32).step_by(2) {
            assert_eq!(tree.remove(&i), Some(i), "删除 {} 失败", i);
            validate_tree(&tree);
        }
        assert_eq!(tree.len(), 16);

        // 再删奇数（树逐步变矮直至为空）
        for i in (1..32u32).step_by(2) {
            assert_eq!(tree.remove(&i), Some(i), "删除 {} 失败", i);
            validate_tree(&tree);
        }
        assert!(tree.is_empty());
        assert_eq!(tree.remove(&0), None);
    }

    #[test]
    fn test_delete_descending_order() {
        // 降序删除与升序删除路径不同（更多走最右/最左路径）
        let mut tree: TestTree = BTreeMap::new();
        for i in 0..24u32 {
            tree.insert(i, i);
        }
        for i in (0..24u32).rev() {
            assert_eq!(tree.remove(&i), Some(i));
            validate_tree(&tree);
        }
        assert!(tree.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut tree: TestTree = BTreeMap::new();
        tree.insert(1, 1);
        tree.insert(3, 3);
        assert_eq!(tree.remove(&2), None);
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn test_tree_grows_and_shrinks() {
        // 足够多的键迫使树长高（2-3-4 树在 4、16 键处增长）
        let mut tree: TestTree = BTreeMap::new();
        for i in 0..100u32 {
            tree.insert(i, i);
        }
        validate_tree(&tree);
        for i in 0..100u32 {
            assert_eq!(tree.get(&i), Some(&i));
        }
        for i in 0..100u32 {
            assert_eq!(tree.remove(&i), Some(i));
        }
        assert!(tree.is_empty());
    }
}

