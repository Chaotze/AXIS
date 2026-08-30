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
    // 先格式到栈缓冲，再统一输出：
    // 1. 保证一次调用输出“完整一行/一段”，避免多核交错
    // 2. 同时镜像到调试串口（COM1，QEMU -serial stdio 可见），便于
    //    脚本化/无显示环境中采集内核日志（临时调试设施，后续可移除）
    let mut buf = [0u8; 512];
    let mut wr = BufWriter(&mut buf, 0);
    let _ = wr.write_fmt(args);
    let len = wr.1;

    let mut writer = WRITER.lock();
    for &b in &buf[..len] {
        if b == b'\n' {
            writer.write_byte(b'\r');
        }
        writer.write_byte(b);
    }

    // 调试：镜像到 COM1（16550 UART），QEMU 下配合 -serial stdio 使用。
    // 为什么用 in/out 指令而不是内存读：x86 上端口 I/O 只能通过
    // IN/OUT 指令完成，对 0x3FD 的普通内存读并不等价（低位物理内
    // 存已取消映射，会直接触发缺页）。fire-and-forget：QEMU 的 16550
    // 发送保持寄存器几乎总是空闲，省去 LSR 轮询的开销。
    for &b in &buf[..len] {
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") 0x3F8u16,
                in("al") if b == b'\n' { b'\r' } else { b },
                options(nostack, preserves_flags)
            );
        }
    }
}

/// 栈上格式化缓冲写入器
struct BufWriter<'a>(&'a mut [u8], usize);

impl fmt::Write for BufWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let room = self.0.len().saturating_sub(self.1);
        let n = bytes.len().min(room);
        self.0[self.1..self.1 + n].copy_from_slice(&bytes[..n]);
        self.1 += n;
        Ok(())
    }
}