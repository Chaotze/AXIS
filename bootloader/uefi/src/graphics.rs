// ============================================================
// UEFI 图形输出初始化
// ============================================================
// 使用 GOP (Graphics Output Protocol) 初始化图形模式

use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat};

/// 帧缓冲区信息
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// 帧缓冲区物理地址
    pub addr: u64,

    /// 宽度（像素）
    pub width: u32,

    /// 高度（像素）
    pub height: u32,

    /// 每行字节数（stride/pitch）
    #[allow(dead_code)]
    pub pitch: u32,

    /// 每像素位数
    #[allow(dead_code)]
    pub bpp: u8,

    /// 像素格式
    #[allow(dead_code)]
    pub pixel_format: PixelFormatType,
}

/// 像素格式类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormatType {
    /// RGB 格式
    Rgb,
    /// BGR 格式
    Bgr,
    /// 位掩码格式（自定义）
    Bitmask,
}

/// 初始化图形输出
///
/// 使用 UEFI GOP 协议获取当前图形模式的帧缓冲区信息
///
/// # 为什么不主动设置图形模式？
/// - UEFI 固件通常在启动时已经设置了合适的图形模式
/// - 主动设置可能会选择不兼容的模式，导致黑屏
/// - 内核启动后可以根据需要切换模式
pub fn init_graphics(boot_services: &uefi::table::boot::BootServices) -> uefi::Result<FramebufferInfo> {
    // 查找 GOP 协议句柄
    let gop_handle = boot_services
        .get_handle_for_protocol::<GraphicsOutput>()
        .map_err(|_| Status::UNSUPPORTED)?;

    // 打开 GOP 协议
    let mut gop = boot_services
        .open_protocol_exclusive::<GraphicsOutput>(gop_handle)
        .map_err(|_| Status::UNSUPPORTED)?;

    // 获取当前模式信息
    let mode = gop.current_mode_info();
    let mut framebuffer = gop.frame_buffer();

    // 转换像素格式
    let pixel_format = match mode.pixel_format() {
        PixelFormat::Rgb => PixelFormatType::Rgb,
        PixelFormat::Bgr => PixelFormatType::Bgr,
        PixelFormat::Bitmask | PixelFormat::BltOnly => {
            // BltOnly 模式不提供直接帧缓冲区访问，需要使用 Blt 操作
            // 这里简化处理，假设为 Bitmask
            PixelFormatType::Bitmask
        }
    };

    // 计算每像素位数
    // GOP 通常使用 32 位像素（RGBA 或 BGRA）
    let bpp = 32;

    // 计算 pitch（每行字节数）
    let (width, height) = mode.resolution();
    let pitch = mode.stride() * 4; // stride 是每行的像素数，乘以 4（每像素 4 字节）

    Ok(FramebufferInfo {
        addr: framebuffer.as_mut_ptr() as u64,
        width: width as u32,
        height: height as u32,
        pitch: pitch as u32,
        bpp,
        pixel_format,
    })
}
