// ============================================================
// LRU 缓存（最近最少使用替换，定长）
// ============================================================
// 固定容量的键值缓存，容量满时淘汰"最久未被访问"的条目。
//
// 为什么内核需要 LRU：
// - 页缓存淘汰、目录项缓存（dcache）、连接表等场景都要
//   回答"满了踢谁"；LRU 利用时间局部性是普适的默认答案
//
// 为什么设计为定长（const 泛型 CAP/MAP_CAP）：
// - 无动态分配器：条目槽位数组 + 哈希槽位数组全部内嵌，
//   容量在编译期确定（如"256 条目的 dcache"）
// - MAP_CAP 独立成参的原因：Rust 稳定版不支持对 const 泛型
//   做算术（CAP*2）作为数组长度，且开放寻址必须保留空槽
//   终止探测，MAP_CAP 必须大于 CAP（建议 2 倍）
//
// 为什么用"哈希表 + 双向链表"双结构：
// - 哈希表负责 O(1) 定位；双向链表按"最近使用"排序，
//   head 为最新、tail 为最旧，淘汰/提升都是 O(1) 指针操作
// - 链表用槽位下标（而非指针）链接：槽位数组不移动，
//   下标永不过期，也天然规避了节点内存管理问题
//
// 为什么槽位数组是 MaybeUninit + 空闲链表：
// - 槽位"先写后读"（写入完整条目后才挂进链表/哈希表），
//   无需默认值占位，因此键值只需 Copy，不必 Default
// - 删除腾出的槽位挂入空闲链表复用，避免"只增不减"的
//   槽位泄漏
//
// 为什么键值要求 Copy：
// - 槽位按位搬移值，无需处理析构时序。内核键值通常是
//   字长标量；复杂类型可用指针/句柄间接存储

use core::mem::MaybeUninit;

/// 哈希槽位：空槽（从未使用，探测链终点）
const MAP_EMPTY: u32 = u32::MAX;
/// 哈希槽位：墓碑（条目已删，探测继续越过）
const MAP_TOMBSTONE: u32 = u32::MAX - 1;
/// 链表哨兵下标（无前驱/后继）
const NIL: usize = usize::MAX;

/// LRU 条目（数据 + 链表指针）
struct LruEntry<K, V> {
    key: K,
    value: V,
    /// 链表前驱（更"新"一侧）
    prev: usize,
    /// 链表后继（更"旧"一侧）
    next: usize,
}

/// 定长 LRU 缓存
///
/// 泛型参数：
/// - `CAP`: 条目容量
/// - `MAP_CAP`: 哈希槽位数（必须大于 CAP，建议 2 倍）
pub struct LruCache<K: Eq + Copy, V: Copy, const CAP: usize, const MAP_CAP: usize> {
    /// 条目槽位数组（槽位"先写后读"，空闲槽用 next 链成空闲表）
    entries: MaybeUninit<[LruEntry<K, V>; CAP]>,
    /// 开放寻址哈希表：键 → 条目槽位下标
    map: [u32; MAP_CAP],
    /// 链表头（最近使用）
    head: usize,
    /// 链表尾（最久未使用，淘汰对象）
    tail: usize,
    /// 空闲槽位链表头（复用空闲槽的 next 字段）
    free_slots: Option<usize>,
    /// 当前条目数
    len: usize,
}

impl<K: Eq + Copy, V: Copy, const CAP: usize, const MAP_CAP: usize> LruCache<K, V, CAP, MAP_CAP> {
    /// 创建空缓存
    ///
    /// # 为什么断言 MAP_CAP > CAP：
    /// - 开放寻址的探测循环依赖空槽终止；若哈希表满载，
    ///   探测将永无止境
    pub const fn new() -> Self {
        assert!(CAP > 0, "LRU 容量必须大于 0");
        assert!(MAP_CAP > CAP, "哈希槽位数必须大于条目容量（建议 2 倍）");
        Self {
            entries: MaybeUninit::uninit(),
            map: [MAP_EMPTY; MAP_CAP],
            head: NIL,
            tail: NIL,
            free_slots: None,
            len: 0,
        }
    }

    /// 当前条目数
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 容量
    #[inline]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// 是否包含键
    pub fn contains(&self, key: &K) -> bool {
        self.find_entry(key).is_some()
    }

    /// 查询键并标记为最近使用（LRU 提升）
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let index = self.find_entry(key)?;
        self.promote(index);
        Some(&self.entry(index).value)
    }

    /// 查询键但不改变 LRU 顺序（只读探测，如"是否存在"）
    pub fn peek(&self, key: &K) -> Option<&V> {
        let index = self.find_entry(key)?;
        Some(&self.entry(index).value)
    }

    /// 插入键值对；返回被淘汰条目的值（容量满时）
    ///
    /// 键已存在时原地更新并提升，不淘汰任何条目。
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        if let Some(index) = self.find_entry(&key) {
            // 键已存在：更新值 + 提升
            self.entry_mut(index).value = value;
            self.promote(index);
            return None;
        }

        // 为新键挑选槽位：优先复用空闲槽，否则追加或淘汰
        let index = match self.take_free_slot() {
            Some(slot) => slot,
            None if self.len < CAP => self.len,
            None => {
                // 已满：淘汰最久未使用的条目（链表尾），复用其槽位
                let evicted_index = self.tail;
                let evicted_value = self.entry(evicted_index).value;
                let evicted_key = self.entry(evicted_index).key;
                self.unlink(evicted_index);
                self.map_remove(evicted_key);
                self.write_entry(evicted_index, key, value);
                self.link_head(evicted_index);
                self.map_insert(key, evicted_index);
                return Some(evicted_value);
            }
        };

        self.write_entry(index, key, value);
        self.link_head(index);
        self.map_insert(key, index);
        self.len += 1;
        None
    }

    /// 删除键；返回被删除的值
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let index = self.find_entry(key)?;
        let value = self.entry(index).value;
        self.unlink(index);
        self.map_remove(*key);
        // 槽位挂回空闲链表供复用
        self.entry_mut(index).next = self.free_slots.unwrap_or(NIL);
        self.free_slots = Some(index);
        self.len -= 1;
        Some(value)
    }

    // ============================================================
    // 内部：槽位管理
    // ============================================================

    /// 从空闲链表取一个槽位
    fn take_free_slot(&mut self) -> Option<usize> {
        let slot = self.free_slots?;
        // 空闲链表复用槽位的 next 字段
        let next = self.entry(slot).next;
        self.free_slots = (next != NIL).then_some(next);
        Some(slot)
    }

    /// 写入完整条目（覆盖旧内容；旧内容必须已解除引用）
    fn write_entry(&mut self, index: usize, key: K, value: V) {
        let entry = self.entry_mut(index);
        entry.key = key;
        entry.value = value;
        entry.prev = NIL;
        entry.next = NIL;
    }

    #[inline]
    fn entry(&self, index: usize) -> &LruEntry<K, V> {
        unsafe { &*self.entries.as_ptr().cast::<LruEntry<K, V>>().add(index) }
    }

    #[inline]
    fn entry_mut(&mut self, index: usize) -> &mut LruEntry<K, V> {
        unsafe { &mut *self.entries.as_mut_ptr().cast::<LruEntry<K, V>>().add(index) }
    }

    // ============================================================
    // 内部：哈希表（开放寻址 + 线性探测 + 墓碑）
    // ============================================================

    /// 键的哈希值（FNV-1a，复用 lib/hash.rs 的整型键入口）
    #[inline]
    fn hash_key(key: &K) -> usize {
        // 键按字节哈希；调用方需保证键无未初始化填充字节
        // （字长标量天然满足，结构体键请使用 #[repr(C, packed)]）
        let bytes = unsafe {
            core::slice::from_raw_parts(
                key as *const K as *const u8,
                core::mem::size_of::<K>(),
            )
        };
        super::super::hash::fnv1a64(bytes) as usize
    }

    /// 查找键对应的条目槽位下标
    fn find_entry(&self, key: &K) -> Option<usize> {
        let start = Self::hash_key(key) % MAP_CAP;
        for probe in 0..MAP_CAP {
            let slot = (start + probe) % MAP_CAP;
            match self.map[slot] {
                MAP_EMPTY => return None, // 探测链终点
                MAP_TOMBSTONE => continue, // 墓碑：继续探测
                entry_index => {
                    if &self.entry(entry_index as usize).key == key {
                        return Some(entry_index as usize);
                    }
                }
            }
        }
        None
    }

    /// 插入键 → 条目下标映射（调用方保证键不存在）
    fn map_insert(&mut self, key: K, entry: usize) {
        let start = Self::hash_key(&key) % MAP_CAP;
        for probe in 0..MAP_CAP {
            let slot = (start + probe) % MAP_CAP;
            if self.map[slot] == MAP_EMPTY || self.map[slot] == MAP_TOMBSTONE {
                self.map[slot] = entry as u32;
                return;
            }
        }
        // 理论上不可达：MAP_CAP > CAP 保证有空槽
        unreachable!("哈希表已满（不变量被破坏）");
    }

    /// 从哈希表删除键（写墓碑而不是直接清空）
    ///
    /// # 为什么必须写墓碑：
    /// - 开放寻址的探测链依赖"越过被删槽位继续找"；
    ///   若直接置空，被删槽位之后的同链条目将永久失联
    fn map_remove(&mut self, key: K) {
        let start = Self::hash_key(&key) % MAP_CAP;
        for probe in 0..MAP_CAP {
            let slot = (start + probe) % MAP_CAP;
            match self.map[slot] {
                MAP_EMPTY => return,
                entry_index if self.entry(entry_index as usize).key == key => {
                    self.map[slot] = MAP_TOMBSTONE;
                    return;
                }
                _ => continue,
            }
        }
    }

    // ============================================================
    // 内部：双向链表（按最近使用排序）
    // ============================================================

    /// 把条目移到链表头（最近使用）
    fn promote(&mut self, index: usize) {
        if self.head == index {
            return; // 已在头部
        }
        self.unlink(index);
        self.link_head(index);
    }

    /// 从链表中摘除条目
    fn unlink(&mut self, index: usize) {
        let (prev, next) = (self.entry(index).prev, self.entry(index).next);
        if prev != NIL {
            self.entry_mut(prev).next = next;
        } else {
            self.head = next; // 摘的是头
        }
        if next != NIL {
            self.entry_mut(next).prev = prev;
        } else {
            self.tail = prev; // 摘的是尾
        }
        self.entry_mut(index).prev = NIL;
        self.entry_mut(index).next = NIL;
    }

    /// 把（已摘除的）条目挂到链表头
    fn link_head(&mut self, index: usize) {
        self.entry_mut(index).next = self.head;
        self.entry_mut(index).prev = NIL;
        if self.head != NIL {
            self.entry_mut(self.head).prev = index;
        } else {
            self.tail = index; // 空表：头尾合一
        }
        self.head = index;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    /// 测试用缓存：容量 3，哈希槽 8
    type TestLru = LruCache<u32, u32, 3, 8>;

    #[test]
    fn test_put_get_basic() {
        let mut cache: TestLru = LruCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.put(1, 10), None);
        assert_eq!(cache.put(2, 20), None);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.get(&2), Some(&20));
        assert_eq!(cache.get(&3), None);
        assert!(cache.contains(&1));
        assert!(!cache.contains(&3));
    }

    #[test]
    fn test_eviction_order() {
        let mut cache: TestLru = LruCache::new();
        cache.put(1, 10);
        cache.put(2, 20);
        cache.put(3, 30);
        // 访问 1：顺序变为 [1,3,2]，2 成为最旧
        assert_eq!(cache.get(&1), Some(&10));
        // 插入 4：淘汰 2
        assert_eq!(cache.put(4, 40), Some(20));
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.get(&3), Some(&30));
        assert_eq!(cache.get(&4), Some(&40));
    }

    #[test]
    fn test_put_existing_updates_and_promotes() {
        let mut cache: TestLru = LruCache::new();
        cache.put(1, 10);
        cache.put(2, 20);
        cache.put(3, 30);
        // 更新 2（应无淘汰），且 2 提升为最新
        assert_eq!(cache.put(2, 99), None);
        assert_eq!(cache.len(), 3);
        // 再插入：淘汰最旧的 1
        assert_eq!(cache.put(4, 40), Some(10));
        assert_eq!(cache.get(&2), Some(&99));
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn test_remove_and_slot_reuse() {
        let mut cache: TestLru = LruCache::new();
        cache.put(1, 10);
        cache.put(2, 20);
        assert_eq!(cache.remove(&1), Some(10));
        assert_eq!(cache.remove(&1), None);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&2), Some(&20));
        // 被删槽位被空闲链表复用
        cache.put(3, 30);
        assert_eq!(cache.get(&3), Some(&30));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_remove_reuse_cycle() {
        // 反复删除/插入，验证空闲槽复用无泄漏、哈希表一致
        let mut cache: TestLru = LruCache::new();
        for round in 0..10u32 {
            cache.put(round, round);
            assert_eq!(cache.remove(&round), Some(round));
            assert!(cache.is_empty());
        }
        for i in 0..3u32 {
            cache.put(i, i);
        }
        assert_eq!(cache.len(), 3);
        for i in 0..3u32 {
            assert_eq!(cache.get(&i), Some(&i));
        }
    }

    #[test]
    fn test_peek_does_not_promote() {
        let mut cache: TestLru = LruCache::new();
        cache.put(1, 10);
        cache.put(2, 20);
        cache.put(3, 30);
        // peek 不改变顺序：最旧的仍是 1
        assert_eq!(cache.peek(&1), Some(&10));
        assert_eq!(cache.put(4, 40), Some(10));
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn test_tombstone_probing() {
        // 小哈希表 + 多条目：必然发生碰撞与墓碑探测
        let mut cache: TestLru = LruCache::new();
        for i in 0..3u32 {
            cache.put(i, i);
        }
        cache.remove(&1); // 写墓碑
        cache.put(4, 4); // 命中/越过墓碑
        assert_eq!(cache.get(&0), Some(&0));
        assert_eq!(cache.get(&2), Some(&2));
        assert_eq!(cache.get(&4), Some(&4));
        assert_eq!(cache.get(&1), None);
    }
}
