// ============================================================
// VGA 文本模式支持
// ============================================================
// 提供对 VGA 文本缓冲区（物理 0xB8000）的底层写入能力。
//
// 为什么单独成模块而不是写在 print.rs 里：
//   - println!（print.rs：持自旋锁输出）与 panic 处理器（panic.rs：关中断
//     裸写）都需要一个 VGA 写入器；此前 print.rs / panic.rs / main.rs 各
//     维护一份几乎相同的实现，且 VGA 地址是写死的 0xB8000——恒等映射删除
//     后地址必须改为物理内存映射区地址，三处漂移的副本正是重复冗余的典型
//   - 本模块只关心「如何写 VGA」：不加锁、不挂钩子，由调用方决定并发策略

use core::fmt::{self, Write};

use crate::config::VGA_TEXT_BUFFER;

/// VGA 文本模式尺寸（宽 80 × 高 25 字符）
pub const VGA_WIDTH: usize = 80;
pub const VGA_HEIGHT: usize = 25;

/// VGA 文本模式写入器（无锁，对硬件缓冲区的最底层访问）
///
/// # 并发约定
/// 本类型不做任何同步；多核下并发访问需由调用方保证互斥：
/// - 常规打印经 print.rs 的 Spinlock 串行化
/// - panic 路径中断已禁用，单核独占
pub struct VgaWriter {
    buffer: *mut u16,
    column: usize,
    row: usize,
    color: u8,
}

// 手动实现 Send / Sync：
// 为什么安全：buffer 指向硬件 MMIO 内存而非普通堆对象，
// 其读写互斥由调用方（自旋锁 / 关中断）保证，不受编译器优化影响
unsafe impl Send for VgaWriter {}
unsafe impl Sync for VgaWriter {}

impl VgaWriter {
    /// 创建写入器
    ///
    /// 默认黑底亮白（0x0F），保证任何输出都可见；
    /// 历史实现曾误用 0 作为默认色（属性 0x00 = 黑底黑字，输出不可见）
    pub const fn new() -> Self {
        Self {
            buffer: VGA_TEXT_BUFFER as *mut u16,
            column: 0,
            row: 0,
            color: 0x0F,
        }
    }

    /// 设置颜色属性（低 4 位前景、高 4 位背景/闪烁）
    pub fn set_color(&mut self, color: u8) {
        self.color = color;
    }

    /// 清空整个屏幕，光标回到左上角
    ///
    /// 为什么用 write_volatile：硬件 MMIO 的写入副作用（显示）不能
    /// 被编译器当作普通内存访问优化掉（合并、重排），必须按易失性写
    pub fn clear_screen(&mut self) {
        let blank = ((self.color as u16) << 8) | (b' ' as u16);
        for i in 0..(VGA_WIDTH * VGA_HEIGHT) {
            unsafe {
                self.buffer.add(i).write_volatile(blank);
            }
        }
        self.column = 0;
        self.row = 0;
    }

    /// 写入单个字节（按字符逐字节处理，处理 \n 与 \r）
    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.column = 0,
            byte => {
                if self.column >= VGA_WIDTH {
                    self.newline();
                }

                let pos = self.row * VGA_WIDTH + self.column;
                let color_byte = (self.color as u16) << 8;
                unsafe {
                    self.buffer
                        .add(pos)
                        .write_volatile(color_byte | byte as u16);
                }
                self.column += 1;
            }
        }
    }

    /// 换行：光标移到下一行开头，到底部则向上滚动
    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;

        if self.row >= VGA_HEIGHT {
            self.scroll();
            self.row = VGA_HEIGHT - 1;
        }
    }

    /// 向上滚动一行（屏幕内容逐行上移，末行留空）
    fn scroll(&mut self) {
        for y in 1..VGA_HEIGHT {
            for x in 0..VGA_WIDTH {
                let src = y * VGA_WIDTH + x;
                let dst = (y - 1) * VGA_WIDTH + x;
                let ch = unsafe { self.buffer.add(src).read_volatile() };
                unsafe {
                    self.buffer.add(dst).write_volatile(ch);
                }
            }
        }

        // 清空最后一行
        let blank = ((self.color as u16) << 8) | (b' ' as u16);
        for x in 0..VGA_WIDTH {
            let pos = (VGA_HEIGHT - 1) * VGA_WIDTH + x;
            unsafe {
                self.buffer.add(pos).write_volatile(blank);
            }
        }
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

/// 清空整个屏幕（模块级便捷函数，供无需持有写入器的调用方使用）
pub fn clear_screen() {
    let mut writer = VgaWriter::new();
    writer.clear_screen();
}