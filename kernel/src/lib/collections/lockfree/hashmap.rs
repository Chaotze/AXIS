// ============================================================
// 无锁哈希表（堆支持、开放寻址、CAS 槽位）
// ============================================================
// u64 键 → u64 值的并发哈希映射，读写无需互斥锁。
//
// 为什么内核需要无锁哈希表：
// - 中断上下文、多核共享的"名称 → 对象"小表（如设备
//   注册表、模块符号表）；读多写少且不能睡眠的场景
//
// 为什么是"堆支持 + 开放寻址"：
// - 槽位数组在构造时按容量分配（Box<[AtomicU64]>）；
//   分配完成后数组地址恒定、槽位永不移动，并发协议全部
//   不变——无锁结构只要求"已发布的内存不动"，与分配时机无关
// - 开放寻址避免链表节点回收与并发遍历的难题；
//   链式哈希表的节点生命周期管理在无锁环境下异常复杂
//
// 并发协议（Cliff Click 式槽位状态机）：
// - 每个槽位是一个原子字，打包 [状态:8bit][键:56bit]，
//   一次 CAS 同时完成"检查 + 写入"，这是无锁的关键
// - insert：先把值写入值数组（Release），再 CAS 把槽位
//   从 EMPTY/TOMBSTONE 置为 OCCUPIED；槽位 CAS 成功即
//   线性化点
// - remove：CAS 槽位 OCCUPIED → TOMBSTONE；墓碑参与探测
//   但不参与匹配，后续插入可复用
// - get：读槽位（Acquire）命中后读值——值先于槽位发布，
//   内存序保证不会看到"新键 + 旧值"
//
// 为什么键限制为 56 位：
// - 8 位状态与 56 位键打包进一个原子字；键必须小于
//   2^56（debug 断言），内核的地址/索引键远小于此界

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use core::cell::UnsafeCell;

/// 槽位状态（打包在槽位的高 8 位）
const SLOT_EMPTY: u64 = 0;
const SLOT_OCCUPIED: u64 = 1;
const SLOT_TOMBSTONE: u64 = 2;

/// 键掩码（低 56 位）
const KEY_MASK: u64 = (1 << 56) - 1;

/// 把状态与键打包为槽位值
#[inline]
const fn pack_slot(state: u64, key: u64) -> u64 {
    (state << 56) | (key & KEY_MASK)
}

/// 堆支持无锁哈希表
pub struct LockfreeHashMap {
    /// 槽位数组：[状态|键] 原子字（构造时分配，此后地址恒定）
    slots: Box<[AtomicU64]>,
    /// 值数组：与槽位一一对应（受槽位状态机保护）
    values: UnsafeCell<Box<[u64]>>,
    /// 当前条目数
    len: AtomicUsize,
}

// 为什么可以安全地 Sync：
// - 槽位是原子字；值数组的访问受槽位状态机约束
//   （值写入先于槽位发布、读取后于槽位命中），
//   跨线程可见性由 Acquire/Release 保证
unsafe impl Sync for LockfreeHashMap {}

impl LockfreeHashMap {
    /// 以指定容量创建空表（槽位/值数组在堆上分配）
    ///
    /// # 为什么 new 取代 const 构造：
    /// - 定长版本靠 const 泛型在编译期给出槽位数组；堆支持
    ///   版本容量在运行期确定，new 一次性分配两份数组，
    ///   数组地址自此不变，并发安全的前提不受影响
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "哈希表容量必须大于 0");
        let slots = (0..capacity)
            .map(|_| AtomicU64::new(pack_slot(SLOT_EMPTY, 0)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            values: UnsafeCell::new(vec![0u64; capacity].into_boxed_slice()),
            len: AtomicUsize::new(0),
        }
    }

    /// 当前条目数
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 查询键；命中返回值的拷贝
    ///
    /// # 为什么返回值拷贝而不是引用：
    /// - 无锁环境下条目可能被并发删除，返回引用无法
    ///   保证生存期；值拷贝（u64）是并发安全的读取方式
    pub fn get(&self, key: u64) -> Option<u64> {
        debug_assert!(key <= KEY_MASK, "键超出 56 位范围");
        let start = self.hash(key);

        for probe in 0..self.slots.len() {
            let slot = (start + probe) % self.slots.len();
            let packed = self.slots[slot].load(Ordering::Acquire);
            let state = packed >> 56;
            match state {
                SLOT_EMPTY => return None, // 探测链终点
                SLOT_TOMBSTONE => continue, // 墓碑：继续探测
                SLOT_OCCUPIED if (packed & KEY_MASK) == key => {
                    // 命中：值先于槽位发布，Acquire 保证可见
                    return Some(unsafe { (*self.values.get())[slot] });
                }
                _ => continue,
            }
        }
        None
    }

    /// 是否包含键
    pub fn contains(&self, key: u64) -> bool {
        self.get(key).is_some()
    }

    /// 插入键值对；表满返回 false
    ///
    /// 键已存在时覆盖值并返回 true（不增加计数）。
    pub fn insert(&self, key: u64, value: u64) -> bool {
        debug_assert!(key <= KEY_MASK, "键超出 56 位范围");
        let start = self.hash(key);

        for probe in 0..self.slots.len() {
            let slot = (start + probe) % self.slots.len();
            let packed = self.slots[slot].load(Ordering::Acquire);
            let state = packed >> 56;

            match state {
                SLOT_OCCUPIED if (packed & KEY_MASK) == key => {
                    // 同键覆盖：值写回即可，槽位状态不变
                    unsafe { (*self.values.get())[slot] = value };
                    return true;
                }
                SLOT_EMPTY | SLOT_TOMBSTONE => {
                    // 先写值再发布槽位；CAS 成功即线性化点
                    unsafe { (*self.values.get())[slot] = value };
                    let new_packed = pack_slot(SLOT_OCCUPIED, key);
                    if self.slots[slot]
                        .compare_exchange(packed, new_packed, Ordering::Release, Ordering::Relaxed)
                        .is_ok()
                    {
                        self.len.fetch_add(1, Ordering::AcqRel);
                        return true;
                    }
                    // CAS 失败：槽位被并发修改，继续探测
                    // （本轮的探测步进会重新读同一槽位的新状态）
                }
                _ => {}
            }
        }
        false // 探测满表仍无空槽
    }

    /// 删除键；返回被删除的值
    pub fn remove(&self, key: u64) -> Option<u64> {
        debug_assert!(key <= KEY_MASK, "键超出 56 位范围");
        let start = self.hash(key);

        for probe in 0..self.slots.len() {
            let slot = (start + probe) % self.slots.len();
            let packed = self.slots[slot].load(Ordering::Acquire);
            let state = packed >> 56;

            match state {
                SLOT_EMPTY => return None,
                SLOT_OCCUPIED if (packed & KEY_MASK) == key => {
                    // 先取值，再把槽位置为墓碑；CAS 成功即删除生效
                    let value = unsafe { (*self.values.get())[slot] };
                    let tombstone = pack_slot(SLOT_TOMBSTONE, key);
                    if self.slots[slot]
                        .compare_exchange(packed, tombstone, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                    {
                        self.len.fetch_sub(1, Ordering::AcqRel);
                        return Some(value);
                    }
                    // CAS 失败：槽位已被并发修改，重读重试
                }
                _ => {}
            }
        }
        None
    }

    /// 键的起始探测位置（FNV-1a，复用 lib/hash.rs）
    #[inline]
    fn hash(&self, key: u64) -> usize {
        super::super::super::hash::hash_u64(key) as usize % self.slots.len()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::sync::Arc;
    use std::thread;

    use super::*;

    #[test]
    fn test_insert_get_remove() {
        let map = LockfreeHashMap::new(64);
        assert!(map.is_empty());

        assert!(map.insert(1, 100));
        assert!(map.insert(2, 200));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(1), Some(100));
        assert_eq!(map.get(2), Some(200));
        assert_eq!(map.get(3), None);
        assert!(map.contains(1));

        assert_eq!(map.remove(1), Some(100));
        assert_eq!(map.remove(1), None);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(1), None);
    }

    #[test]
    fn test_overwrite() {
        let map = LockfreeHashMap::new(8);
        assert!(map.insert(5, 1));
        assert!(map.insert(5, 2)); // 覆盖不增计数
        assert_eq!(map.get(5), Some(2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_tombstone_reuse() {
        // 容量小必然碰撞；删除后墓碑槽位应可复用
        let map = LockfreeHashMap::new(8);
        for i in 0..6u64 {
            assert!(map.insert(i, i * 10));
        }
        assert_eq!(map.remove(2), Some(20));
        assert_eq!(map.remove(4), Some(40));
        assert_eq!(map.len(), 4);
        // 新键复用墓碑槽位
        assert!(map.insert(100, 1000));
        assert!(map.insert(101, 1010));
        assert_eq!(map.len(), 6);
        for i in 0..6u64 {
            if i != 2 && i != 4 {
                assert_eq!(map.get(i), Some(i * 10));
            }
        }
        assert_eq!(map.get(100), Some(1000));
        assert_eq!(map.get(101), Some(1010));
    }

    #[test]
    fn test_full_table_rejects() {
        // 填满后新键插入必须失败
        let map = LockfreeHashMap::new(8);
        for i in 0..8u64 {
            assert!(map.insert(i, i));
        }
        assert!(!map.insert(100, 1));
        assert_eq!(map.len(), 8);
    }

    #[test]
    fn test_concurrent_insert_get() {
        // 两个线程各插 100 个不相交键，随后全部可见
        let map: Arc<LockfreeHashMap> = Arc::new(LockfreeHashMap::new(512));

        let mut handles = std::vec::Vec::new();
        for t in 0..2 {
            let map = Arc::clone(&map);
            handles.push(thread::spawn(move || {
                for i in 0..100u64 {
                    assert!(map.insert(t * 1000 + i, t * 1000 + i));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(map.len(), 200);
        for t in 0..2 {
            for i in 0..100u64 {
                assert_eq!(map.get(t * 1000 + i), Some(t * 1000 + i));
            }
        }
    }
}
