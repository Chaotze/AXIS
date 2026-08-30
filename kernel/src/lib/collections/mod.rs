// ============================================================
// 堆支持数据结构库
// ============================================================
// 常用数据结构集合：存储由内核堆分配，
// 容量在运行期决定。
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
