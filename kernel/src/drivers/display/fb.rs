// ============================================================
// 帧缓冲（Framebuffer）
// ============================================================
// 通用线性帧缓冲抽象：一段连续显存 + 分辨率/行距/位深描述，
// 提供像素级绘图原语（画点、矩形填充、清屏）。
//
// 为什么单独成模块：VGA 文本模式（lib/vga.rs 负责）、UEFI GOP、
// VESA 帧缓冲都是「往显存写像素」的变体；统一抽象后 gop/vesafb
// 驱动只需提供帧缓冲描述，绘图逻辑共用一份。
//
// 纯逻辑设计：Framebuffer 只操作内存（base 指针），不触碰寄存器，
// 可在宿主环境用普通字节数组验证绘图正确性。

use core::ptr;

/// 像素颜色格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32 位：每像素 4 字节，通道顺序 BGRA（低位蓝）
    Bgra32,
    /// 32 位：每像素 4 字节，通道顺序 RGBA
    Rgba32,
    /// 24 位：每像素 3 字节，通道顺序 RGB
    Rgb24,
    /// 16 位：RGB565（高 5 红、中 6 绿、低 5 蓝）
    Rgb565,
}

/// 线性帧缓冲描述
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// 显存起始虚拟地址（经物理内存映射区）
    pub base: u64,
    /// 宽度（像素）
    pub width: usize,
    /// 高度（像素）
    pub height: usize,
    /// 行距（每行起始到下一行起始的字节数）
    pub pitch: usize,
    /// 像素格式
    pub format: PixelFormat,
}

impl FramebufferInfo {
    /// 每像素字节数
    pub const fn bytes_per_pixel(&self) -> usize {
        match self.format {
            PixelFormat::Bgra32 | PixelFormat::Rgba32 => 4,
            PixelFormat::Rgb24 => 3,
            PixelFormat::Rgb565 => 2,
        }
    }

    /// 像素位置对应的显存偏移（纯函数）
    pub const fn offset(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(y * self.pitch + x * self.bytes_per_pixel())
    }
}

/// 帧缓冲绘图对象
///
/// # 并发约定
/// 本类型不自行加锁；多核并发绘图由调用方保证互斥
/// （与 VgaWriter 相同的约定）。
pub struct Framebuffer {
    /// 显存起始指针
    base: *mut u8,
    /// 帧缓冲描述
    info: FramebufferInfo,
}

// 手动实现 Send/Sync：指向显存的裸指针，互斥由调用方保证
unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// 从物理地址 + 描述创建帧缓冲（地址需已映射）
    ///
    /// # Safety
    /// base 必须指向有效且已映射的显存
    pub unsafe fn new(base: *mut u8, info: FramebufferInfo) -> Self {
        Self { base, info }
    }

    /// 从已映射的虚拟地址创建
    ///
    /// # Safety
    /// 同上；调用方保证地址有效
    pub unsafe fn from_virt_addr(virt: u64, info: FramebufferInfo) -> Self {
        Self { base: virt as *mut u8, info }
    }

    /// 帧缓冲描述
    pub const fn info(&self) -> FramebufferInfo {
        self.info
    }

    /// 宽度
    pub const fn width(&self) -> usize {
        self.info.width
    }

    /// 高度
    pub const fn height(&self) -> usize {
        self.info.height
    }

    /// 在 (x, y) 画一个像素
    ///
    /// color 统一用 0xRRGGBB 表示，按格式展开写入。
    pub fn put_pixel(&self, x: usize, y: usize, color: u32) {
        let Some(off) = self.info.offset(x, y) else { return };
        let p = unsafe { self.base.add(off) };
        unsafe {
            match self.info.format {
                PixelFormat::Bgra32 => {
                    ptr::write_volatile(p, (color & 0xFF) as u8);
                    ptr::write_volatile(p.add(1), ((color >> 8) & 0xFF) as u8);
                    ptr::write_volatile(p.add(2), ((color >> 16) & 0xFF) as u8);
                    ptr::write_volatile(p.add(3), 0xFF);
                }
                PixelFormat::Rgba32 => {
                    ptr::write_volatile(p, ((color >> 16) & 0xFF) as u8);
                    ptr::write_volatile(p.add(1), ((color >> 8) & 0xFF) as u8);
                    ptr::write_volatile(p.add(2), (color & 0xFF) as u8);
                    ptr::write_volatile(p.add(3), 0xFF);
                }
                PixelFormat::Rgb24 => {
                    ptr::write_volatile(p, ((color >> 16) & 0xFF) as u8);
                    ptr::write_volatile(p.add(1), ((color >> 8) & 0xFF) as u8);
                    ptr::write_volatile(p.add(2), (color & 0xFF) as u8);
                }
                PixelFormat::Rgb565 => {
                    let r5 = ((color >> 19) & 0x1F) as u16;
                    let g6 = ((color >> 10) & 0x3F) as u16;
                    let b5 = ((color >> 3) & 0x1F) as u16;
                    let pixel = (r5 << 11) | (g6 << 5) | b5;
                    ptr::write_volatile(p as *mut u16, pixel);
                }
            }
        }
    }

    /// 填充一个矩形区域
    pub fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x2 = x.saturating_add(w).min(self.info.width);
        let y2 = y.saturating_add(h).min(self.info.height);
        for py in y..y2 {
            for px in x..x2 {
                self.put_pixel(px, py, color);
            }
        }
    }

    /// 清屏（填充指定颜色，默认黑色）
    pub fn clear(&self, color: u32) {
        self.fill_rect(0, 0, self.info.width, self.info.height, color);
    }
}

/// VGA 文本模式帧缓冲兼容层
///
/// 把 80×25 的 16 位文本单元视作一种「帧缓冲」，复用统一绘图
/// 接口的调用约定；底层由 lib/vga.rs 的 VgaWriter 提供。
/// 为什么保留：文本模式是当前唯一可用的显示输出，也是 VGA
/// 图形模式/线性帧缓冲就绪前的过渡实现。
pub mod vga_text {
    /// VGA 文本缓冲区单元：低字节字符，高字节属性
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Cell {
        pub ch: u8,
        pub fg: u8,
        pub bg: u8,
    }

    impl Cell {
        /// 包装成 16 位单元（屏幕上的实际存储格式）
        pub const fn to_u16(&self) -> u16 {
            ((self.bg as u16) << 12) | ((self.fg as u16) << 8) | self.ch as u16
        }

        /// 从 16 位单元解包
        pub const fn from_u16(v: u16) -> Self {
            Self {
                ch: (v & 0xFF) as u8,
                fg: ((v >> 8) & 0x0F) as u8,
                bg: ((v >> 12) & 0x0F) as u8,
            }
        }
    }

    /// 16 色标准 VGA 调色板（低 4 位）
    impl Cell {
        pub const BLACK: u8 = 0;
        pub const BLUE: u8 = 1;
        pub const GREEN: u8 = 2;
        pub const CYAN: u8 = 3;
        pub const RED: u8 = 4;
        pub const MAGENTA: u8 = 5;
        pub const BROWN: u8 = 6;
        pub const LIGHT_GRAY: u8 = 7;
        pub const DARK_GRAY: u8 = 8;
        pub const LIGHT_BLUE: u8 = 9;
        pub const LIGHT_GREEN: u8 = 10;
        pub const LIGHT_CYAN: u8 = 11;
        pub const LIGHT_RED: u8 = 12;
        pub const LIGHT_MAGENTA: u8 = 13;
        pub const YELLOW: u8 = 14;
        pub const WHITE: u8 = 15;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset() {
        let info = FramebufferInfo {
            base: 0,
            width: 800,
            height: 600,
            pitch: 800 * 4,
            format: PixelFormat::Bgra32,
        };
        assert_eq!(info.offset(0, 0), Some(0));
        assert_eq!(info.offset(1, 0), Some(4));
        assert_eq!(info.offset(0, 1), Some(800 * 4));
        assert_eq!(info.offset(799, 599), Some(599 * 800 * 4 + 799 * 4));
        assert_eq!(info.offset(800, 0), None);
        assert_eq!(info.offset(0, 600), None);
    }

    #[test]
    fn test_put_pixel_bgra32() {
        let mut buf = [0u8; 16];
        let info = FramebufferInfo {
            base: buf.as_ptr() as u64,
            width: 2,
            height: 2,
            pitch: 8,
            format: PixelFormat::Bgra32,
        };
        let fb = unsafe { Framebuffer::new(buf.as_mut_ptr(), info) };
        fb.put_pixel(1, 0, 0x00FF_80); // 橙红色
        assert_eq!(&buf[4..8], &[0x80, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn test_fill_rect() {
        let mut buf = [0u8; 4 * 4 * 4];
        let info = FramebufferInfo {
            base: buf.as_ptr() as u64,
            width: 4,
            height: 4,
            pitch: 16,
            format: PixelFormat::Rgba32,
        };
        let fb = unsafe { Framebuffer::new(buf.as_mut_ptr(), info) };
        fb.fill_rect(1, 1, 2, 2, 0x00FF00); // 绿色
        // 中心 2x2 应为绿色，四角保持透明
        for y in 0..4 {
            for x in 0..4 {
                let off = y * 16 + x * 4;
                let inside = x >= 1 && x < 3 && y >= 1 && y < 3;
                if inside {
                    assert_eq!(&buf[off..off + 4], &[0x00, 0xFF, 0x00, 0xFF]);
                } else {
                    assert_eq!(&buf[off..off + 4], &[0, 0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn test_rgb565() {
        let mut buf = [0u8; 4];
        let info = FramebufferInfo {
            base: buf.as_ptr() as u64,
            width: 1,
            height: 1,
            pitch: 2,
            format: PixelFormat::Rgb565,
        };
        let fb = unsafe { Framebuffer::new(buf.as_mut_ptr(), info) };
        fb.put_pixel(0, 0, 0xFF_00_00); // 红
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 0xF800);
    }

    #[test]
    fn test_vga_cell() {
        use super::vga_text::Cell;
        let cell = Cell { ch: b'A', fg: Cell::WHITE, bg: Cell::BLACK };
        // WHITE=15（见调色板定义），不是 0x07（LIGHT_GRAY）
        assert_eq!(cell.to_u16(), (Cell::WHITE as u16) << 8 | b'A' as u16);
        assert_eq!(Cell::from_u16((0x47 << 8) | 0x42), Cell { ch: b'B', fg: 0x7, bg: 0x4 });
    }
}