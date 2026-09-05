// ============================================================
// PCI 配置空间访问
// ============================================================
// 通过传统端口 I/O（0xCF8/0xCFC）读写 PCI 配置空间。
//
// 机制：向 CONFIG_ADDRESS 端口写入编码后的 32 位地址
// （使能位 + 总线/设备/功能/寄存器偏移），再从 CONFIG_DATA
// 端口读写数据。配置地址编码是纯算术，可宿主单元测试；
// 端口读写是 x86 平台操作，由 arch::io 提供。

use crate::arch::x86_64::io::{inl, outl};

/// 配置地址端口（写地址用）
pub const CONFIG_ADDRESS: u16 = 0xCF8;
/// 配置数据端口（读写数据用）
pub const CONFIG_DATA: u16 = 0xCFC;

/// 标准配置空间寄存器偏移（PCI 3.0 规范）
pub mod reg {
    pub const VENDOR_ID: u8 = 0x00;
    pub const DEVICE_ID: u8 = 0x02;
    pub const COMMAND: u8 = 0x04;
    pub const STATUS: u8 = 0x06;
    pub const REVISION: u8 = 0x08;
    pub const PROG_IF: u8 = 0x09;
    pub const SUBCLASS: u8 = 0x0A;
    pub const CLASS: u8 = 0x0B;
    pub const CACHE_LINE_SIZE: u8 = 0x0C;
    pub const LATENCY_TIMER: u8 = 0x0D;
    pub const HEADER_TYPE: u8 = 0x0E;
    pub const BIST: u8 = 0x0F;
    pub const BAR0: u8 = 0x10;
    pub const BAR1: u8 = 0x14;
    pub const BAR2: u8 = 0x18;
    pub const BAR3: u8 = 0x1C;
    pub const BAR4: u8 = 0x20;
    pub const BAR5: u8 = 0x24;
    // PCI-PCI 桥专用寄存器（header type 0x01）
    pub const PRIMARY_BUS: u8 = 0x18;
    pub const SECONDARY_BUS: u8 = 0x19;
    pub const SUBORDINATE_BUS: u8 = 0x1A;
    // 通用寄存器
    pub const SUBSYSTEM_VENDOR: u8 = 0x2C;
    pub const SUBSYSTEM_ID: u8 = 0x2E;
    pub const INTERRUPT_LINE: u8 = 0x3C;
    pub const INTERRUPT_PIN: u8 = 0x3D;
}

/// 编码 PCI 配置地址（纯函数，可宿主单元测试）
///
/// 格式：bit31 = 使能位，bits 23-16 = 总线号，bits 15-11 = 设备号，
/// bits 10-8 = 功能号，bits 7-2 = 寄存器字节偏移（dword 对齐）。
///
/// 为什么 4 字节对齐：PCI 配置空间按 32 位双字访问，低 2 位
/// 固定为 0；读写 8/16 位寄存器时由数据端口按字节偏移取用。
#[inline]
pub const fn config_address(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
}

/// 读 32 位配置寄存器
///
/// # Safety
/// 必须保证 bus/dev/func 对应的设备存在；对不存在的设备读取
/// 会返回 0xFFFFFFFF（硬件行为），不会造成破坏
#[inline]
pub unsafe fn config_read_u32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, dev, func, offset));
        inl(CONFIG_DATA)
    }
}

/// 读 16 位配置寄存器
#[inline]
pub unsafe fn config_read_u16(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, dev, func, offset));
        (inl(CONFIG_DATA) >> ((offset as u32 & 0x2) * 8)) as u16
    }
}

/// 读 8 位配置寄存器
#[inline]
pub unsafe fn config_read_u8(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, dev, func, offset));
        (inl(CONFIG_DATA) >> ((offset as u32 & 0x3) * 8)) as u8
    }
}

/// 写 32 位配置寄存器
///
/// # Safety
/// 必须保证设备存在；错误的写可能破坏设备状态
#[inline]
pub unsafe fn config_write_u32(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, dev, func, offset));
        outl(CONFIG_DATA, value);
    }
}

/// 写 16 位配置寄存器
#[inline]
pub unsafe fn config_write_u16(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    // 16 位写通过读-改-写避免破坏相邻字节
    unsafe {
        let base = (offset as u32) & !0x2;
        let old = config_read_u32(bus, dev, func, base as u8);
        let shifted = (value as u32) << ((offset as u32 & 0x2) * 8);
        let mask = 0xFFFFu32 << ((offset as u32 & 0x2) * 8);
        config_write_u32(bus, dev, func, base as u8, (old & !mask) | shifted);
    }
}

/// 写 8 位配置寄存器
#[inline]
pub unsafe fn config_write_u8(bus: u8, dev: u8, func: u8, offset: u8, value: u8) {
    unsafe {
        let base = (offset as u32) & !0x3;
        let old = config_read_u32(bus, dev, func, base as u8);
        let shifted = (value as u32) << ((offset as u32 & 0x3) * 8);
        let mask = 0xFFu32 << ((offset as u32 & 0x3) * 8);
        config_write_u32(bus, dev, func, base as u8, (old & !mask) | shifted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_address_encoding() {
        // 总线 0、设备 0、功能 0、偏移 0 → 使能位 + 0
        assert_eq!(config_address(0, 0, 0, 0), 0x8000_0000);
        // 总线 1、设备 2、功能 3、偏移 0x10
        assert_eq!(config_address(1, 2, 3, 0x10), 0x8000_0000 | (1 << 16) | (2 << 11) | (3 << 8) | 0x10);
        // 偏移必须 dword 对齐（低 2 位被清）
        assert_eq!(config_address(0, 0, 0, 0x13), 0x8000_0000 | 0x10);
    }

    #[test]
    fn test_config_address_full_range() {
        // 最大总线/设备/功能/偏移
        let a = config_address(255, 31, 7, 0xFF);
        assert_eq!(a, 0x8000_0000 | (255 << 16) | (31 << 11) | (7 << 8) | 0xFC);
    }
}