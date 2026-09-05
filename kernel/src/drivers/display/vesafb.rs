// ============================================================
// VESA 帧缓冲驱动
// ============================================================
// 使用 BIOS/VBE 图形模式设置的线性帧缓冲。
//
// 现状：BIOS 引导程序需在进入长模式前通过 INT 10h AX=4F01h
// 查询 VBE 模式并设置线性帧缓冲，然后把模式信息传给内核
// （bootloader 侧尚未实现）；本模块先提供描述结构与封装，
// 待引导协议就绪后由 display::init 组装。
//
// 与 GOP 一样，VESA 帧缓冲本质是线性显存，复用 display::fb。

use super::fb::{Framebuffer, FramebufferInfo, PixelFormat};

/// VESA 帧缓冲描述（引导程序经启动协议传入）
#[derive(Debug, Clone, Copy)]
pub struct VesaFbInfo {
    /// 图形模式号（VBE 模式）
    pub mode: u16,
    /// 帧缓冲物理地址
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

/// VESA 图形输出设备
pub struct VesaFb {
    /// 模式号（保留供诊断）
    mode: u16,
    framebuffer: Framebuffer,
}

impl VesaFb {
    /// 从引导协议提供的 VBE 信息创建设备
    ///
    /// # Safety
    /// info.phys_base 必须指向有效且已映射的帧缓冲
    pub unsafe fn from_info(info: VesaFbInfo) -> VesaFb {
        let fb_info = FramebufferInfo {
            base: crate::config::PHYSICAL_MEMORY_OFFSET + info.phys_base,
            width: info.width,
            height: info.height,
            pitch: info.pitch,
            format: info.format,
        };
        VesaFb {
            mode: info.mode,
            framebuffer: unsafe { Framebuffer::from_virt_addr(fb_info.base, fb_info) },
        }
    }

    /// VBE 模式号
    pub const fn mode(&self) -> u16 {
        self.mode
    }

    /// 访问绘图对象
    pub const fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }
}