// ============================================================
// 基础库函数
// ============================================================
// 提供内核通用的工具函数和数据结构

pub mod print;
pub mod string;
pub mod result;
pub mod time;
pub mod debug;

// 重新导出常用类型
pub use result::{KernelResult, KernelError};
