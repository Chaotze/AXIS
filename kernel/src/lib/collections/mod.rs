// ============================================================
// 定长数据结构库
// ============================================================
// 无动态分配环境下的常用数据结构集合。
//
// 为什么全部设计为定长（const 泛型容量）：
// - 内核启动早期没有动态分配器（kmalloc 属阶段 3），
//   数据结构必须把存储内嵌在使用方结构体中
// - 分配器落地后，各结构可在此定长实现的基础上
//   泛化出动态版本（替换节点池为 kmalloc 即可），
//   核心算法无需改动——这是"先手搓、再上库"路线
//   在数据结构层的具体体现

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
