// ============================================================
// UEFI GOP（Graphics Output Protocol）驱动
// ============================================================
// 使用 UEFI 固件提供的图形输出帧缓冲。
//
// 现状：UEFI 引导程序在 ExitBootServices 前通过 GOP 取得帧缓冲
// 的物理地址/分辨率/行距，需经引导协议传递给内核（bootloader 侧
// 尚未实现传递）；本模块先提供帧缓冲描述结构与封装，待引导协议
// 就绪后由 display::init 组装。
//
// 与 framebuffer 的关系：GOP 提供的本质上就是一个线性帧缓冲，
// 因此复用 display::fb 的绘图原语，本模块只负责「描述 + 注册」。

use super::fb::{Framebuffer, FramebufferInfo, PixelFormat};

/// UEFI GOP 帧缓冲描述（引导程序经启动协议传入）
#[derive(Debug, Clone, Copy)]
pub struct GopInfo {
    /// 帧缓冲物理地址（需映射后使用）
    pub phys_base: u64,
    /// 宽度（像素）
    pub width: usize,
    /// 高度（像素）
    pub height: usize,
    /// 行距（字节）
    pub pitch: usize,
    /// 像素格式
    pub format: PixelFormat,
}

/// GOP 图形输出设备
pub struct GopFb {
    framebuffer: Framebuffer,
}

impl GopFb {
    /// 从引导协议提供的 GOP 信息创建设备
    ///
    /// # Safety
    /// info.phys_base 必须指向由引导程序映射好的帧缓冲
    pub unsafe fn from_info(info: GopInfo) -> GopFb {
        let fb_info = FramebufferInfo {
            // 帧缓冲物理地址经直接映射区访问
            base: crate::config::PHYSICAL_MEMORY_OFFSET + info.phys_base,
            width: info.width,
            height: info.height,
            pitch: info.pitch,
            format: info.format,
        };
        GopFb {
            framebuffer: unsafe { Framebuffer::from_virt_addr(fb_info.base, fb_info) },
        }
    }

    /// 访问绘图对象
    pub const fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }
}