// ============================================================
// 通用库函数和工具
// ============================================================
// 内核版"标准库"：打印、字符串、哈希、CRC、位操作、
// 错误类型、时间与调试工具，以及堆支持数据结构库。
//
// 为什么没有直接使用 core/std 的对应设施：
// - no_std 环境只有 core；而本模块承担的是"内核语义"层
//   （如 KernelError 的错误码体系、tick 时间基准），
//   与用户态标准库的语义不同

pub mod print;
pub mod string;
pub mod hash;
pub mod crc;
pub mod bit;
pub mod result;
pub mod time;
pub mod debug;
pub mod vga;
pub mod collections;

// 重新导出常用类型
pub use result::{KernelError, KernelResult};
