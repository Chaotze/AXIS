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

// 库函数模块
pub mod libcore;

// 重新导出常用类型和宏
pub use prelude::*;
