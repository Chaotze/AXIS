// ============================================================
// 打印功能
// ============================================================
// 提供格式化输出到控制台的能力（print! / println! 宏的实现后端）。
//
// 结构说明：
//   底层 VGA 写入逻辑（字符绘制、滚动、光标）在 vga.rs 中实现，
//   本模块只负责"全局单例 + 加锁"，避免多核并发打印互相串扰。

use core::fmt::{self, Write};

use crate::libcore::vga::VgaWriter;
use crate::sync::Spinlock;

/// VGA 文本模式写入器（经自旋锁串行化，多核安全）
static WRITER: Spinlock<VgaWriter> = Spinlock::new(VgaWriter::new());

/// 内核打印函数
///
/// 为什么需要锁：
/// - 多个 CPU 核心可能同时调用 print
/// - VGA 缓冲区是全局共享的
/// - 不加锁会导致输出混乱
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = WRITER.lock();
    writer.write_fmt(args).unwrap();
}