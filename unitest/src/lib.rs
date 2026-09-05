// ============================================================
// AXIS 宿主单元测试入口
// ============================================================
// 用 include! 把内核里的「纯算法源文件」逐个作为独立模块编入本 crate：
// - 不复制代码（低冗余）：源文件只在 kernel/src 存在一份
// - 模块层级刻意与内核保持一致，每个源文件包进同名模块
//   （pub mod bit { include!(bit.rs) }），使源文件内部的
//   `super::` 相对路径在两种环境语义相同
//
// 为什么用 include! 而不是 #[path]：
// - 两端路径都基于 env!("CARGO_MANIFEST_DIR") 展开成绝对路径，
//   不依赖工作目录；新版 nightly 对 #[path] 增加「不得越出 crate
//   根目录」的限制，../../../kernel 这种回到工作区根的写法会直接
//   编译失败（本 crate 根 = unitest/，kernel 在其父目录）
// - include! 是纯文本展开，不受模块文件路径规则限制，语义等价；
//   但必须逐文件包进独立模块，否则多个文件的 use/局部项会互相
//   冲撞（这正是 #[path] 每个文件一个模块的理由）

extern crate alloc;

// ============================================================
// unitest::context —— 对应 kernel/src/arch/x86_64/context
// ============================================================
// 上下文帧布局的宿主测试（纯数据结构，无 arch 依赖）。

pub mod context {
    pub mod frame {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/arch/x86_64/context/frame.rs"));
    }
}

// ============================================================
// unitest::lib —— 对应 kernel/src/lib 的纯算法子集
// ============================================================
// 仅收录不依赖 arch/硬件/VGA 的模块；print/vga/debug 等
// 胶水层不在宿主测试范围内（由内核启动自测验证）。
// 目录层级与内核一致（lib/collections, lib/collections/lockfree），
// 保证源文件内部 super:: 相对路径两环境语义相同。

pub mod lib {
    pub mod bit {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/bit.rs"));
    }
    pub mod crc {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/crc.rs"));
    }
    pub mod hash {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/hash.rs"));
    }
    pub mod string {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/string.rs"));
    }
    pub mod time {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/time.rs"));
    }

    // ========================================================
    // unitest::lib::collections —— 对应 kernel/src/lib/collections
    // ========================================================
    // 堆支持数据结构库的全部宿主测试（含 lockfree）。

    pub mod collections {
        pub mod bitmap {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/bitmap.rs"));
        }
        pub mod btree {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/btree.rs"));
        }
        pub mod lru {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/lru.rs"));
        }
        pub mod radix_tree {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/radix_tree.rs"));
        }
        pub mod ring_buffer {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/ring_buffer.rs"));
        }

        // ====================================================
        // unitest::lib::collections::lockfree
        // ====================================================
        // 无锁栈/队列/哈希表的宿主测试（并发用例在宿主线程环境运行）。

        pub mod lockfree {
            pub mod hashmap {
                include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/lockfree/hashmap.rs"));
            }
            pub mod queue {
                include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/lockfree/queue.rs"));
            }
            pub mod stack {
                include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/lib/collections/lockfree/stack.rs"));
            }
        }
    }
}

// ============================================================
// unitest::heap —— 对应 kernel/src/mm/heap
// ============================================================

pub mod heap {
    pub mod slub {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/heap/slub.rs"));
    }
    pub mod slab_cache {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/heap/slab_cache.rs"));
    }
    pub mod kmalloc {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/heap/kmalloc.rs"));
    }
}

// ============================================================
// unitest::pmm —— 对应 kernel/src/mm/pmm
// ============================================================

pub mod pmm {
    pub mod buddy {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/pmm/buddy.rs"));
    }
    pub mod frame {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/pmm/frame.rs"));
    }
    pub mod watermark {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/pmm/watermark.rs"));
    }
    pub mod zone {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/pmm/zone.rs"));
    }
    pub mod numa {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/pmm/numa.rs"));
    }
}

// ============================================================
// unitest::vmm —— 对应 kernel/src/mm/vmm
// ============================================================

pub mod vmm {
    pub mod vma {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/vmm/vma.rs"));
    }
    pub mod swap {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/mm/vmm/swap.rs"));
    }
}

// ============================================================
// unitest::task —— 对应 kernel/src/task
// ============================================================
// 层级与内核一致：task 组 + scheduler 子组；模块间 super::
// 相对路径在两种环境中语义相同（crate:: 绝对路径禁用）。
// 注：scheduler/mod.rs（Scheduler 装配结构）不在此引入——
// 它依赖 cfs 的队列接口，其行为已由 cfs/process 的测试覆盖。

pub mod task {
    pub mod pcb {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/pcb.rs"));
    }
    pub mod thread {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/thread.rs"));
    }
    pub mod signal {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/signal.rs"));
    }
    pub mod resource {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/resource.rs"));
    }
    pub mod namespace {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/namespace.rs"));
    }
    pub mod cgroup {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/cgroup.rs"));
    }
    pub mod process {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/process.rs"));
    }

    pub mod scheduler {
        pub mod cfs {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/scheduler/cfs.rs"));
        }
        pub mod preemption {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/scheduler/preemption.rs"));
        }
        pub mod cpu_affinity {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/scheduler/cpu_affinity.rs"));
        }
        pub mod load_balance {
            include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/task/scheduler/load_balance.rs"));
        }
    }
}