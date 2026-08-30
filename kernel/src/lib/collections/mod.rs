// ============================================================
// 堆支持数据结构库
// ============================================================
// 常用数据结构集合：存储由内核堆分配（mm 落地后），
// 容量在运行期决定。
//
// 为什么本库全部是"堆支持版本"：
// - lib 最初全部是定长版本（const 泛型容量，存储内嵌在使用方
//   结构体中）：那时内核没有动态分配器，mm 尚未完成
// - mm（kmalloc → GlobalAlloc）落地后，各结构按当年预留的路径
//   泛化出动态版本——把内嵌存储/节点池替换为堆分配，核心算法
//   无需改动。这是"先手搓、再上库"路线在数据结构层的落地
//
// 说明：无锁栈/队列（lockfree）仍采用"调用方提供节点"接口，
// 这是无锁结构内存回收（ABA/era）约束下的标准形态，节点可从
// slab/kmem_cache 池取出，与堆是否就绪无关

pub mod ring_buffer;
pub mod btree;
pub mod radix_tree;
pub mod bitmap;
pub mod lru;
pub mod lockfree;

pub use bitmap::Bitmap;
pub use btree::BTreeMap;
pub use lru::LruCache;
pub use radix_tree::RadixTree;
pub use ring_buffer::RingBuffer;

