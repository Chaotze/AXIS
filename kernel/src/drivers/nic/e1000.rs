// ============================================================
// Intel e1000 网卡驱动
// ============================================================
// e1000（82540EM/82545EM/82574L 等）以太网控制器：PCI 探测、
// 软件复位、EEPROM 读 MAC、接收地址寄存器编程、收发器使能。
//
// 实现状态：
// ✅ 完成：设备探测、软件复位、MAC 读取、接收/发送控制器使能、
//   链路状态检测（QEMU 默认 e1000 设备可实测）
// ⏳ 待完成（网络栈阶段）：
//   - RX/TX 描述符环建立与中断（或轮询）
//   - NicDevice::send/recv 实现与注册
// 因此当前阶段以「初始化 + 报告」交付，不注册可用的网卡对象。

use crate::prelude::KernelResult;
use super::driver::MacAddr;

/// e1000 MMIO 寄存器偏移（BAR0）
#[allow(dead_code)]
mod reg {
    pub const CTRL: u64 = 0x0000;
    pub const STATUS: u64 = 0x0008;
    pub const EERD: u64 = 0x0014;
    pub const ICR: u64 = 0x00C0;
    pub const IMS: u64 = 0x00D0;
    pub const RCTL: u64 = 0x0100;
    pub const TCTL: u64 = 0x0400;
    pub const RDBAL: u64 = 0x2800;
    pub const RDBAH: u64 = 0x2804;
    pub const RDLEN: u64 = 0x2808;
    pub const RDH: u64 = 0x2810;
    pub const RDT: u64 = 0x2818;
    pub const TDBAL: u64 = 0x3800;
    pub const TDBAH: u64 = 0x3804;
    pub const TDLEN: u64 = 0x3808;
    pub const TDH: u64 = 0x3810;
    pub const TDT: u64 = 0x3818;
    pub const MTA: u64 = 0x5200;
    pub const RAL0: u64 = 0x5400;
    pub const RAH0: u64 = 0x5404;
}

// CTRL 位
const CTRL_RST: u32 = 1 << 26;
// STATUS 位
const STATUS_LU: u32 = 1 << 1;  // 链路接通
// RCTL 位
const RCTL_EN: u32 = 1 << 1;    // 接收器使能
const RCTL_BAM: u32 = 1 << 15;  // 接收广播
// TCTL 位
const TCTL_EN: u32 = 1 << 0;    // 发送器使能
const TCTL_PSP: u32 = 1 << 3;   // 填充短帧
// EERD 位
const EERD_START: u32 = 1 << 0;
const EERD_DONE: u32 = 1 << 4;

/// e1000 设备（当前仅保存探测信息，不持有 MMIO 映射）
pub struct E1000Device {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub mac: MacAddr,
    pub link_up: bool,
}

/// 读 MMIO 32 位寄存器
unsafe fn read_reg(base: u64, off: u64) -> u32 {
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}

unsafe fn write_reg(base: u64, off: u64, val: u32) {
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}

/// 从 EEPROM 读取一个字（EEPROM 读命令）
unsafe fn eeprom_read(base: u64, word: u16) -> Option<u16> {
    unsafe {
        write_reg(base, reg::EERD, EERD_START | ((word as u32) << 8));
        // 轮询完成位
        for _ in 0..2000 {
            let v = read_reg(base, reg::EERD);
            if v & EERD_DONE != 0 {
                return Some((v >> 16) as u16);
            }
            core::hint::spin_loop();
        }
    }
    None
}

/// 探测并初始化 e1000 设备
///
/// 找到设备时完成复位、MAC 读取与收发控制器使能并打印；
/// 由于帧收发通路尚未实现，返回 None（不注册网卡）。
pub fn probe() -> KernelResult<()> {
    // 已知 e1000 设备 ID；也接受类别 02/00（以太网）+ Intel 厂商
    let known = [0x100E, 0x100F, 0x10D3];
    let dev = {
        let mut found = None;
        for id in known {
            if let Some(d) = crate::drivers::pci::find(0x8086, id) {
                found = Some(d);
                break;
            }
        }
        if found.is_none() {
            found = crate::drivers::pci::find_by_class(0x02, 0x00)
                .into_iter().find(|d| d.vendor_id == 0x8086);
        }
        found
    };
    let Some(dev) = dev else {
        return Ok(());
    };

    // BAR0 映射（e1000 的 MMIO 寄存器）
    let bar0 = unsafe { dev.read_bar(0) }.unwrap_or(0);
    let phys = if bar0 & 0x4 != 0 {
        // 64 位 BAR
        let hi = unsafe { dev.read_bar(1) }.unwrap_or(0) as u64;
        (bar0 & 0xFFFF_FFF0) as u64 | (hi << 32)
    } else {
        (bar0 & 0xFFFF_FFF0) as u64
    };
    if phys == 0 {
        println!("[E1000] {:02X}:{:02X}.{} has no BAR0", dev.bus, dev.dev, dev.func);
        return Ok(());
    }
    let base = crate::config::PHYSICAL_MEMORY_OFFSET + phys;

    // 软件复位
    unsafe {
        write_reg(base, reg::CTRL, read_reg(base, reg::CTRL) | CTRL_RST);
    }
    // 等待复位完成（CTRL.RST 自清零）
    for _ in 0..100_000 {
        let v = unsafe { read_reg(base, reg::CTRL) };
        if v & CTRL_RST == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    // 读 EEPROM 前 3 个字得到 MAC（82540EM 布局：word0 = 字节1,0；
    // word1 = 字节3,2；word2 = 字节5,4）
    let w0 = unsafe { eeprom_read(base, 0) }.unwrap_or(0xFFFF);
    let w1 = unsafe { eeprom_read(base, 1) }.unwrap_or(0xFFFF);
    let w2 = unsafe { eeprom_read(base, 2) }.unwrap_or(0xFFFF);
    let mac = MacAddr::new([
        (w0 & 0xFF) as u8,
        ((w0 >> 8) & 0xFF) as u8,
        (w1 & 0xFF) as u8,
        ((w1 >> 8) & 0xFF) as u8,
        (w2 & 0xFF) as u8,
        ((w2 >> 8) & 0xFF) as u8,
    ]);

    // 写入接收地址寄存器（RAH bit31 = 地址有效）
    unsafe {
        write_reg(base, reg::RAL0, u32::from_le_bytes([mac.0[0], mac.0[1], mac.0[2], mac.0[3]]));
        write_reg(base, reg::RAH0, 0x8000_0000 | u32::from(mac.0[4]) | (u32::from(mac.0[5]) << 8));
        // 使能接收（含广播）与发送
        write_reg(base, reg::RCTL, RCTL_EN | RCTL_BAM);
        write_reg(base, reg::TCTL, TCTL_EN | TCTL_PSP | (0x10 << 4) | (0x40 << 16));
    }

    // 链路状态
    let link_up = unsafe { read_reg(base, reg::STATUS) } & STATUS_LU != 0;
    println!("[E1000] {:02X}:{:02X}.{} MAC={} link={} BAR=0x{:X}",
        dev.bus, dev.dev, dev.func, mac.to_string(), link_up, phys);
    println!("[E1000] RX/TX 描述符环与收发通路待网络栈阶段（阶段 7）");

    Ok(())
}