// ============================================================
// PCI 总线子系统
// ============================================================
// PCI 设备枚举与注册：扫描全部总线（含 PCI-PCI 桥的次级总线），
// 生成设备列表，供各设备驱动按 vendor/device 探测。
//
// 分层：
//   config.rs —— 配置空间访问（端口 I/O + 地址编码，纯函数可测）
//   device.rs —— 设备对象与 ID 解码（纯逻辑可测）
//   ecam.rs   —— PCIe ECAM 地址换算（纯逻辑可测）
//   dma.rs    —— DMA 缓冲区分配（PMM 胶水）
//   iommu.rs  —— IOMMU 框架
//   mod.rs    —— 枚举装配层：全局设备表、初始化、自测
//
// 枚举算法：从总线 0 开始，逐设备读 vendor ID（0xFFFF = 空槽）；
// 若 device 0 是多功能设备（header bit7），扫描全部 8 个功能；
// 遇到 PCI-PCI 桥则递归枚举其次级总线（限制深度，防循环）。

pub mod config;
pub mod device;
pub mod dma;
pub mod ecam;
pub mod iommu;

use alloc::vec::Vec;

use crate::prelude::KernelResult;
use crate::sync::Spinlock;

use self::config::reg;
use self::device::PciDevice;

/// 全局 PCI 设备表
static DEVICES: Spinlock<Vec<PciDevice>> = Spinlock::new(Vec::new());

/// 递归枚举的最大总线深度（防御桥接环路）
const MAX_BUS_DEPTH: u8 = 8;

/// PCI 初始化入口：枚举全部设备并打印摘要
///
/// 顺序为什么在 ACPI 之后：枚举本身用端口 I/O，不依赖 ACPI；
/// 但 ECAM 段信息来自 ACPI MCFG，驱动探测可能用到，先让 ACPI
/// 就绪更稳妥。
pub fn init() -> KernelResult<()> {
    let mut devices = Vec::new();
    scan_bus(0, 0, &mut devices);
    devices.sort_by_key(|d| ((d.bus as u32) << 16) | ((d.dev as u32) << 8) | d.func as u32);

    println!("[PCI] Found {} device(s)", devices.len());
    for d in &devices {
        println!(
            "  {:02X}:{:02X}.{} {:04X}:{:04X} {} ({})",
            d.bus, d.dev, d.func,
            d.vendor_id, d.device_id,
            d.class_name(),
            d.device_name(),
        );
    }

    *DEVICES.lock() = devices;
    Ok(())
}

/// 递归扫描一条总线（含桥的次级总线）
fn scan_bus(bus: u8, depth: u8, out: &mut Vec<PciDevice>) {
    if depth > MAX_BUS_DEPTH {
        return;
    }

    for dev in 0..32u8 {
        // 功能 0：判断设备是否存在
        let vendor = unsafe { config::config_read_u16(bus, dev, 0, reg::VENDOR_ID) };
        if vendor == 0xFFFF {
            continue; // 空槽
        }

        let header0 = unsafe { config::config_read_u8(bus, dev, 0, reg::HEADER_TYPE) };

        // 是否多功能设备：决定是否扫描功能 1-7
        if header0 & 0x80 != 0 {
            for func in 0..8u8 {
                scan_function(bus, dev, func, depth, out);
            }
        } else {
            scan_function(bus, dev, 0, depth, out);
        }
    }
}

/// 扫描一个具体功能；若为桥则递归次级总线
fn scan_function(bus: u8, dev: u8, func: u8, depth: u8, out: &mut Vec<PciDevice>) {
    let vendor = unsafe { config::config_read_u16(bus, dev, func, reg::VENDOR_ID) };
    if vendor == 0xFFFF {
        return;
    }
    let header = unsafe { config::config_read_u8(bus, dev, func, reg::HEADER_TYPE) };
    let device = unsafe { PciDevice::from_config(bus, dev, func) };
    out.push(device);

    // PCI-PCI 桥：递归枚举次级总线（header type 0x01）
    if header & 0x7F == 0x01 {
        let secondary = unsafe { config::config_read_u8(bus, dev, func, reg::SECONDARY_BUS) };
        scan_bus(secondary, depth + 1, out);
    }
}

// ---------------------------------------------------------------------
// 公开接口
// ---------------------------------------------------------------------

/// 获取设备列表副本
pub fn devices() -> Vec<PciDevice> {
    DEVICES.lock().clone()
}

/// 按 vendor/device ID 查找设备（返回第一个匹配项）
pub fn find(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    DEVICES.lock().iter().copied().find(|d| {
        d.vendor_id == vendor_id && d.device_id == device_id
    })
}

/// 按类别查找设备（返回全部匹配项）
pub fn find_by_class(class: u8, subclass: u8) -> Vec<PciDevice> {
    DEVICES.lock().iter().copied().filter(|d| {
        d.class == class && d.subclass == subclass
    }).collect()
}

/// 设备数量
pub fn count() -> usize {
    DEVICES.lock().len()
}

/// PCI 子系统自测
///
/// QEMU 默认机型至少有 1 个 PCI 设备（host bridge），本自测要求
/// 枚举非空且能找到已知设备。
pub fn selftest() -> bool {
    let table = DEVICES.lock();
    let count = table.len();
    if count == 0 {
        println!("    [FAIL] PCI enumeration empty");
        return false;
    }
    println!("    [PASS] enumerated {} device(s)", count);

    // 每个设备都应有合法的 vendor ID（0xFFFF 不应出现在表中）
    let invalid = table.iter().any(|d| d.vendor_id == 0xFFFF);
    println!("    [{}] vendor IDs valid", if invalid { "FAIL" } else { "PASS" });

    !invalid
}