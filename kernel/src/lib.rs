// ============================================================
// AXIS 内核库入口
// ============================================================
// 提供内核公共模块和基础设施

#![no_std]

// 宏必须最先声明
#[macro_use]
pub mod macros;

// 公共模块
pub mod config;
pub mod prelude;

// 架构相关代码
pub mod arch;

// 同步原语
pub mod sync;

// 库函数模块（与 arch.md 的 kernel/src/lib/ 规划一致：
// 打印、字符串、哈希、CRC、位操作、错误类型、时间、调试
// 以及定长 collections 数据结构）
//
// 为什么需要 #[path] 显式指向 lib/mod.rs：
// - 本 crate 的根文件恰好也叫 lib.rs，`pub mod lib` 会让
//   Rust 在 src/lib.rs 与 src/lib/mod.rs 之间产生歧义
//   （E0761）；#[path] 显式消歧是官方推荐做法
#[path = "lib/mod.rs"]
pub mod lib;

// 重新导出常用类型和宏
pub use prelude::*;
