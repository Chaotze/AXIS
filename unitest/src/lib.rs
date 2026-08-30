// ============================================================
// AXIS 宿主单元测试入口
// ============================================================
// 用 #[path] 把内核里的「纯算法源文件」逐个作为独立模块编入本 crate：
// - 不复制代码（低冗余）：源文件只在 kernel/src 存在一份
// - 模块层级刻意与内核保持一致：pmm / heap / vmm 三个兄弟组，
//   使源文件内部的 `super::` 相对路径在两种环境语义相同
// - 源文件若使用 `crate::` 绝对路径会在此失败——这正是设计约束：
//   纯算法模块不得依赖内核胶水，可移植性由编译器保证
//
// 路径说明：`#[path]` 相对于「内联模块所在目录」（unitest/src/<组>）
// 解析，且要求逐级目录存在；unitest/src/{pmm,heap,vmm} 目录已创建，
// ../../../kernel 即回到工作区根的 kernel 目录。

extern crate alloc;

pub mod lib {
    #[path = "../../../kernel/src/lib/bit.rs"]
    pub mod bit;
    #[path = "../../../kernel/src/lib/crc.rs"]
    pub mod crc;
    #[path = "../../../kernel/src/lib/hash.rs"]
    pub mod hash;
    #[path = "../../../kernel/src/lib/string.rs"]
    pub mod string;
    #[path = "../../../kernel/src/lib/time.rs"]
    pub mod time;
}

pub mod collections {
    // #[path = "../../../kernel/src/lib/collections/bitmap.rs"]
    // pub mod bitmap;
    #[path = "../../../kernel/src/lib/collections/btree.rs"]
    pub mod btree;
    // #[path = "../../../kernel/src/lib/collections/lru.rs"]
    // pub mod lru;
    #[path = "../../../kernel/src/lib/collections/radix_tree.rs"]
    pub mod radix_tree;
    #[path = "../../../kernel/src/lib/collections/ring_buffer.rs"]
    pub mod ring_buffer;
}

pub mod lockfree {
    // #[path = "../../../kernel/src/lib/collections/lockfree/hashmap.rs"]
    // pub mod hashmap;
    #[path = "../../../kernel/src/lib/collections/lockfree/queue.rs"]
    pub mod queue;
    #[path = "../../../kernel/src/lib/collections/lockfree/stack.rs"]
    pub mod stack;
}

pub mod pmm {
    #[path = "../../../kernel/src/mm/pmm/buddy.rs"]
    pub mod buddy;
    #[path = "../../../kernel/src/mm/pmm/frame.rs"]
    pub mod frame;
    #[path = "../../../kernel/src/mm/pmm/watermark.rs"]
    pub mod watermark;
    #[path = "../../../kernel/src/mm/pmm/zone.rs"]
    pub mod zone;
    #[path = "../../../kernel/src/mm/pmm/numa.rs"]
    pub mod numa;
}

pub mod heap {
    #[path = "../../../kernel/src/mm/heap/slub.rs"]
    pub mod slub;
    #[path = "../../../kernel/src/mm/heap/slab_cache.rs"]
    pub mod slab_cache;
    #[path = "../../../kernel/src/mm/heap/kmalloc.rs"]
    pub mod kmalloc;
}

pub mod vmm {
    #[path = "../../../kernel/src/mm/vmm/vma.rs"]
    pub mod vma;
    #[path = "../../../kernel/src/mm/vmm/swap.rs"]
    pub mod swap;
}
