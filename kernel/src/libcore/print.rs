// ============================================================
// 打印功能
// ============================================================
// 提供格式化输出到控制台的能力

use core::fmt::{self, Write};
use crate::sync::Spinlock;

/// VGA 文本模式缓冲区
static WRITER: Spinlock<VgaWriter> = Spinlock::new(VgaWriter::new());

/// VGA 文本模式写入器
pub struct VgaWriter {
    buffer: *mut u16,
    column: usize,
    row: usize,
    color: u8,
}

// 手动实现 Send 和 Sync
// 为什么安全：VGA 缓冲区是硬件内存映射，不会被编译器优化
// Spinlock 确保了互斥访问
unsafe impl Send for VgaWriter {}
unsafe impl Sync for VgaWriter {}

impl VgaWriter {
    const WIDTH: usize = 80;
    const HEIGHT: usize = 25;
    const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;

    /// 创建新的写入器
    const fn new() -> Self {
        Self {
            buffer: Self::VGA_BUFFER,
            column: 0,
            row: 0,
            color: 0, // 默认颜色，等同于白色文本黑色背景（0x07）
        }
    }

    /// 写入单个字节
    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.newline();
            }
            b'\r' => {
                self.column = 0;
            }
            byte => {
                if self.column >= Self::WIDTH {
                    self.newline();
                }

                unsafe {
                    let pos = self.row * Self::WIDTH + self.column;
                    let color_byte = (self.color as u16) << 8;
                    self.buffer.add(pos).write_volatile(color_byte | byte as u16);
                }
                self.column += 1;
            }
        }
    }

    /// 换行
    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;

        if self.row >= Self::HEIGHT {
            self.scroll();
            self.row = Self::HEIGHT - 1;
        }
    }

    /// 滚动屏幕
    ///
    /// 当到达底部时，向上滚动一行
    fn scroll(&mut self) {
        unsafe {
            // 将每一行向上移动
            for y in 1..Self::HEIGHT {
                for x in 0..Self::WIDTH {
                    let src = y * Self::WIDTH + x;
                    let dst = (y - 1) * Self::WIDTH + x;
                    let char = self.buffer.add(src).read_volatile();
                    self.buffer.add(dst).write_volatile(char);
                }
            }

            // 清空最后一行
            let blank = (self.color as u16) << 8 | b' ' as u16;
            for x in 0..Self::WIDTH {
                let pos = (Self::HEIGHT - 1) * Self::WIDTH + x;
                self.buffer.add(pos).write_volatile(blank);
            }
        }
    }

    /// 设置颜色
    #[allow(dead_code)]
    fn set_color(&mut self, color: u8) {
        self.color = color;
    }
}

impl Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

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
