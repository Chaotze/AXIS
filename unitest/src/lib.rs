// ============================================================
// AXIS 宿主单元测试入口
// ============================================================
// 用 #[path] 把内核里的「纯算法源文件」逐个作为独立模块编入本 crate：
// - 不复制代码（低冗余）：源文件只在 kernel/src 存在一份
// - 模块层级刻意与内核保持一致：unitest/src/{lib,pmm,heap,vmm}
//   四组目录对应 kernel/src/{lib,mm/pmm,mm/heap,mm/vmm}，
//   使源文件内部的 `super::` 相对路径在两种环境语义相同
//   （如 collections/bitmap.rs 的 super::super::bit 与 lockfree/
//   hashmap.rs 的 super::super::super::hash 均可解析）
// - 源文件若使用 `crate::` 绝对路径会在此失败——这正是设计约束：
//   纯算法模块不得依赖内核胶水，可移植性由编译器保证
//
// 路径说明：`#[path]` 相对于「模块所在目录」解析，且要求逐级目录
// 存在；unitest/src/<组>/ 下均为 mod.rs 实体文件（git 可跟踪），
// ../../../kernel 即回到工作区根的 kernel 目录。

extern crate alloc;

#[path = "lib/mod.rs"]
pub mod lib;

#[path = "pmm/mod.rs"]
pub mod pmm;

#[path = "heap/mod.rs"]
pub mod heap;

#[path = "vmm/mod.rs"]
pub mod vmm;
