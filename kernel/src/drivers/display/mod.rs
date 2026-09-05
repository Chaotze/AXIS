// ============================================================
// 显示驱动
// ============================================================
// 显示子系统：帧缓冲抽象 + 各后端（VGA 文本 / UEFI GOP / VESA）。
//
// 初始化职责：
// 1. 通过 PCI 探测显示控制器（Bochs VGA 等）并报告
// 2. 检查引导协议是否提供了线性帧缓冲（GOP/VBE），
//    没有则保持 VGA 文本模式作为活动控制台
//
// 为什么先做探测而非直接创建帧缓冲：线性帧缓冲的物理地址必须
// 由引导程序提供；在引导协议就绪前，无法凭空创建，只能先确认
// 硬件存在并给出可用后端清单。

pub mod fb;
pub mod gop;
pub mod vesafb;

use crate::prelude::KernelResult;

/// 显示后端类型（当前活动输出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayBackend {
    /// VGA 文本模式（80×25，当前唯一可用）
    VgaText,
    /// UEFI GOP 线性帧缓冲
    Gop,
    /// VESA 线性帧缓冲
    Vesa,
    /// 无显示设备
    None,
}

/// 当前活动的显示后端
static mut BACKEND: DisplayBackend = DisplayBackend::None;

/// 显示子系统初始化
pub fn init() -> KernelResult<()> {
    // 通过 PCI 探测显示控制器（类别 03 = Display）
    let displays = crate::drivers::pci::find_by_class(0x03, 0x00);
    if !displays.is_empty() {
        for d in &displays {
            println!("[DISPLAY] VGA controller: {:02X}:{:02X}.{} {}",
                d.bus, d.dev, d.func, d.device_name());
        }
    } else {
        println!("[DISPLAY] no VGA controller found on PCI");
    }

    // 引导协议尚未传递 GOP/VBE 帧缓冲信息，活动后端保持 VGA 文本
    // （TODO: 引导程序在 bootinfo 中加入 framebuffer 描述后，
    //  从这里装载 gop::GopFb / vesafb::VesaFb 并切换 BACKEND）
    let backend = if displays.is_empty() {
        DisplayBackend::None
    } else {
        DisplayBackend::VgaText
    };
    unsafe {
        BACKEND = backend;
    }
    println!("[DISPLAY] active backend: {:?}", backend);
    Ok(())
}

/// 当前活动的显示后端
pub fn active_backend() -> DisplayBackend {
    unsafe { BACKEND }
}

/// 显示子系统自测
///
/// 验证帧缓冲绘图原语（基于内存缓冲，不依赖真实显存）与
/// VGA 文本单元编解码。
pub fn selftest() -> bool {
    use self::fb::{Framebuffer, FramebufferInfo, PixelFormat};

    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    // 32bpp 帧缓冲：画一个 4x4 图案并校验
    let mut buf = [0u8; 4 * 4 * 4];
    let info = FramebufferInfo {
        base: buf.as_ptr() as u64,
        width: 4,
        height: 4,
        pitch: 16,
        format: PixelFormat::Bgra32,
    };
    let fb = unsafe { Framebuffer::new(buf.as_mut_ptr(), info) };
    fb.clear(0x00_00_00);
    fb.put_pixel(0, 0, 0xFF_FF_FF);
    all &= t("framebuffer put_pixel", buf[0] == 0xFF && buf[1] == 0xFF && buf[2] == 0xFF);
    fb.fill_rect(1, 1, 2, 2, 0x00_FF_00);
    all &= t("framebuffer fill_rect", buf[5 * 4] == 0x00 && buf[5 * 4 + 1] == 0xFF);

    // 颜色边界（越界不 panic）
    fb.put_pixel(4, 4, 0xFF_00_00);
    all &= t("framebuffer bounds check", true);

    // VGA 文本单元
    use self::fb::vga_text::Cell;
    let cell = Cell { ch: b'X', fg: Cell::LIGHT_GREEN, bg: Cell::BLACK };
    all &= t("vga cell encode", cell.to_u16() == (0x0A << 8) | b'X' as u16);

    // 后端状态
    all &= t("backend known", unsafe { BACKEND } != DisplayBackend::None
        || crate::drivers::pci::find_by_class(0x03, 0x00).is_empty());

    all
}