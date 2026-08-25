// ============================================================
// AXIS 内核 Panic 处理器
// ============================================================
// 内核 panic 时的处理逻辑，提供详细的错误信息和调试支持

use core::panic::PanicInfo;
use core::fmt::Write;

/// Panic 处理器
///
/// no_std 环境必须提供自定义的 panic 处理器。
/// 当内核发生 panic 时，会调用此函数，输出错误信息并停机。
///
/// 为什么要详细输出信息：
/// - 帮助开发者快速定位问题
/// - 记录现场信息用于后续分析
/// - 防止系统在错误状态下继续运行造成数据损坏
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // 禁用中断，防止在 panic 处理中被打断
    unsafe {
        core::arch::asm!("cli");
    }

    // 使用 VGA 文本模式输出错误信息
    let mut writer = VgaWriter::new();

    // 红色背景标题
    writer.set_color(0x4f); // 红底白字
    let _ = writeln!(writer, "!!! KERNEL PANIC !!!");

    // 恢复正常颜色输出详细信息
    writer.set_color(0x0f); // 黑底亮白

    // 输出 panic 消息
    if let Some(message) = info.message().as_str() {
        let _ = writeln!(writer, "Message: {}", message);
    } else {
        let _ = writeln!(writer, "Message: {}", info.message());
    }

    // 输出 panic 位置
    if let Some(location) = info.location() {
        let _ = writeln!(writer, "Location: {}:{}:{}",
            location.file(), location.line(), location.column());
    }

    let _ = writeln!(writer, "System halted.");

    // 无限循环并停机
    // 为什么用 hlt 而不是空循环：hlt 会让 CPU 进入低功耗状态，减少发热
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// VGA 文本模式写入器
///
/// 封装 VGA 缓冲区访问，提供格式化输出能力
struct VgaWriter {
    buffer: *mut u16,
    column: usize,
    row: usize,
    color: u8,
}

impl VgaWriter {
    const WIDTH: usize = 80;
    const HEIGHT: usize = 25;
    const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;

    fn new() -> Self {
        Self {
            buffer: Self::VGA_BUFFER,
            column: 0,
            row: 0,
            color: 0x0f, // 黑底亮白
        }
    }

    fn set_color(&mut self, color: u8) {
        self.color = color;
    }

    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.column = 0;
                self.row += 1;
                if self.row >= Self::HEIGHT {
                    self.row = Self::HEIGHT - 1;
                }
            }
            byte => {
                if self.column >= Self::WIDTH {
                    self.column = 0;
                    self.row += 1;
                    if self.row >= Self::HEIGHT {
                        self.row = Self::HEIGHT - 1;
                    }
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
}

impl Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}
