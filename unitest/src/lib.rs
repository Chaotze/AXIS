// ============================================================
// AXIS 宿主单元测试入口
// ============================================================
// 用 #[path] 把内核里的「纯算法源文件」逐个作为独立模块编入本 crate：
// - 不复制代码（低冗余）：源文件只在 kernel/src 存在一份
// - 模块层级刻意与内核保持一致
//   使源文件内部的 `super::` 相对路径在两种环境语义相同

extern crate alloc;

// ============================================================
// unitest::lib —— 对应 kernel/src/lib 的纯算法子集
// ============================================================
// 仅收录不依赖 arch/硬件/VGA 的模块；print/vga/debug 等
// 胶水层不在宿主测试范围内（由内核启动自测验证）。
// 目录层级与内核一致（lib/collections, lib/collections/lockfree），
// 保证源文件内部 super:: 相对路径两环境语义相同。

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

    // ========================================================
    // unitest::lib::collections —— 对应 kernel/src/lib/collections
    // ========================================================
    // 堆支持数据结构库的全部宿主测试（含 lockfree）。

    pub mod collections {
        #[path = "../../../../kernel/src/lib/collections/bitmap.rs"]
        pub mod bitmap;

        #[path = "../../../../kernel/src/lib/collections/btree.rs"]
        pub mod btree;

        #[path = "../../../../kernel/src/lib/collections/lru.rs"]
        pub mod lru;

        #[path = "../../../../kernel/src/lib/collections/radix_tree.rs"]
        pub mod radix_tree;

        #[path = "../../../../kernel/src/lib/collections/ring_buffer.rs"]
        pub mod ring_buffer;

        // ====================================================
        // unitest::lib::collections::lockfree
        // ====================================================
        // 无锁栈/队列/哈希表的宿主测试（并发用例在宿主线程环境运行）。

        pub mod lockfree {
            #[path = "../../../../../kernel/src/lib/collections/lockfree/hashmap.rs"]
            pub mod hashmap;

            #[path = "../../../../../kernel/src/lib/collections/lockfree/queue.rs"]
            pub mod queue;

            #[path = "../../../../../kernel/src/lib/collections/lockfree/stack.rs"]
            pub mod stack;
        }
    }
}

// ============================================================
// unitest::heap —— 对应 kernel/src/mm/heap
// ============================================================

pub mod heap {
    #[path = "../../../kernel/src/mm/heap/slub.rs"]
    pub mod slub;

    #[path = "../../../kernel/src/mm/heap/slab_cache.rs"]
    pub mod slab_cache;

    #[path = "../../../kernel/src/mm/heap/kmalloc.rs"]
    pub mod kmalloc;
}

// ============================================================
// unitest::pmm —— 对应 kernel/src/mm/pmm
// ============================================================

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

// ============================================================
// unitest::vmm —— 对应 kernel/src/mm/vmm
// ============================================================

pub mod vmm {
    #[path = "../../../kernel/src/mm/vmm/vma.rs"]
    pub mod vma;

    #[path = "../../../kernel/src/mm/vmm/swap.rs"]
    pub mod swap;
}

// ============================================================
// unitest::task —— 对应 kernel/src/task
// ============================================================
// 层级与内核一致：task 组 + scheduler 子组；模块间 super::
// 相对路径在两种环境中语义相同（crate:: 绝对路径禁用）。
// 注：scheduler/mod.rs（Scheduler 装配结构）不在此引入——
// 它依赖 cfs 的队列接口，其行为已由 cfs/process 的测试覆盖。

pub mod task {
    #[path = "../../../kernel/src/task/pcb.rs"]
    pub mod pcb;

    #[path = "../../../kernel/src/task/thread.rs"]
    pub mod thread;

    #[path = "../../../kernel/src/task/signal.rs"]
    pub mod signal;

    #[path = "../../../kernel/src/task/resource.rs"]
    pub mod resource;

    #[path = "../../../kernel/src/task/namespace.rs"]
    pub mod namespace;

    #[path = "../../../kernel/src/task/cgroup.rs"]
    pub mod cgroup;

    #[path = "../../../kernel/src/task/process.rs"]
    pub mod process;

    pub mod scheduler {
        #[path = "../../../../kernel/src/task/scheduler/cfs.rs"]
        pub mod cfs;

        #[path = "../../../../kernel/src/task/scheduler/preemption.rs"]
        pub mod preemption;

        #[path = "../../../../kernel/src/task/scheduler/cpu_affinity.rs"]
        pub mod cpu_affinity;

        #[path = "../../../../kernel/src/task/scheduler/load_balance.rs"]
        pub mod load_balance;
    }
}
