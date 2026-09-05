// ============================================================
// PCI 设备对象
// ============================================================
// 描述一个已枚举的 PCI 设备：总线位置、厂商/设备 ID、类别编码
// 以及常用配置空间方法。
//
// 纯逻辑部分（ID 解码、类别名称）可宿主单元测试。

use core::fmt;

use super::config::reg;

/// PCI 设备摘要
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice {
    /// 总线号
    pub bus: u8,
    /// 设备号（0-31）
    pub dev: u8,
    /// 功能号（0-7）
    pub func: u8,
    /// 厂商 ID（0xFFFF = 不存在）
    pub vendor_id: u16,
    /// 设备 ID
    pub device_id: u16,
    /// 基类代码
    pub class: u8,
    /// 子类代码
    pub subclass: u8,
    /// 编程接口
    pub prog_if: u8,
    /// 修订号
    pub revision: u8,
    /// 头部类型（bit7 = 多功能设备）
    pub header_type: u8,
    /// 子系统厂商 ID
    pub subsystem_vendor: u16,
    /// 子系统 ID
    pub subsystem_id: u16,
    /// 中断引脚（0 = 不使用）
    pub interrupt_pin: u8,
    /// 中断线路（由固件分配）
    pub interrupt_line: u8,
}

impl PciDevice {
    /// 从配置空间组装设备信息（读取由调用方保证设备存在）
    pub unsafe fn from_config(bus: u8, dev: u8, func: u8) -> Self {
        unsafe {
            Self {
                bus,
                dev,
                func,
                vendor_id: super::config::config_read_u16(bus, dev, func, reg::VENDOR_ID),
                device_id: super::config::config_read_u16(bus, dev, func, reg::DEVICE_ID),
                class: super::config::config_read_u8(bus, dev, func, reg::CLASS),
                subclass: super::config::config_read_u8(bus, dev, func, reg::SUBCLASS),
                prog_if: super::config::config_read_u8(bus, dev, func, reg::PROG_IF),
                revision: super::config::config_read_u8(bus, dev, func, reg::REVISION),
                header_type: super::config::config_read_u8(bus, dev, func, reg::HEADER_TYPE),
                subsystem_vendor: super::config::config_read_u16(bus, dev, func, reg::SUBSYSTEM_VENDOR),
                subsystem_id: super::config::config_read_u16(bus, dev, func, reg::SUBSYSTEM_ID),
                interrupt_pin: super::config::config_read_u8(bus, dev, func, reg::INTERRUPT_PIN),
                interrupt_line: super::config::config_read_u8(bus, dev, func, reg::INTERRUPT_LINE),
            }
        }
    }

    /// 是否 PCI-PCI 桥（header type 0x01）
    pub const fn is_bridge(&self) -> bool {
        self.header_type & 0x7F == 0x01
    }

    /// 是否多功能设备（header type bit7）
    pub const fn is_multifunction(&self) -> bool {
        self.header_type & 0x80 != 0
    }

    /// 读取本设备配置空间的 32 位寄存器
    ///
    /// # Safety
    /// offset 必须落在合法的配置空间范围
    #[inline]
    pub unsafe fn read_config_u32(&self, offset: u8) -> u32 {
        unsafe { super::config::config_read_u32(self.bus, self.dev, self.func, offset) }
    }

    /// 读取本设备配置空间的 16 位寄存器
    #[inline]
    pub unsafe fn read_config_u16(&self, offset: u8) -> u16 {
        unsafe { super::config::config_read_u16(self.bus, self.dev, self.func, offset) }
    }

    /// 读取本设备配置空间的 8 位寄存器
    #[inline]
    pub unsafe fn read_config_u8(&self, offset: u8) -> u8 {
        unsafe { super::config::config_read_u8(self.bus, self.dev, self.func, offset) }
    }

    /// 写入本设备配置空间的 32 位寄存器
    ///
    /// # Safety
    /// 错误的写可能破坏设备状态
    #[inline]
    pub unsafe fn write_config_u32(&self, offset: u8, value: u32) {
        unsafe { super::config::config_write_u32(self.bus, self.dev, self.func, offset, value) }
    }

    /// 写入本设备配置空间的 16 位寄存器
    #[inline]
    pub unsafe fn write_config_u16(&self, offset: u8, value: u16) {
        unsafe { super::config::config_write_u16(self.bus, self.dev, self.func, offset, value) }
    }

    /// 写入本设备配置空间的 8 位寄存器
    #[inline]
    pub unsafe fn write_config_u8(&self, offset: u8, value: u8) {
        unsafe { super::config::config_write_u8(self.bus, self.dev, self.func, offset, value) }
    }

    /// 读取 BAR（基地址寄存器）
    ///
    /// 返回原始值：bit0 为 1 表示 I/O 空间，否则为内存空间
    /// （32 位 BAR 由低 4 位标识类型；64 位 BAR 需读相邻两个）
    pub unsafe fn read_bar(&self, index: usize) -> Option<u32> {
        if index > 5 {
            return None;
        }
        Some(unsafe { self.read_config_u32(reg::BAR0 + (index as u8) * 4) })
    }

    /// 类别名称（基类 + 子类 → 可读字符串）
    pub fn class_name(&self) -> &'static str {
        class_name(self.class, self.subclass)
    }

    /// 厂商名称（已知厂商 ID → 名称）
    pub fn vendor_name(&self) -> &'static str {
        vendor_name(self.vendor_id)
    }

    /// 设备型号名称（已知 (厂商, 设备) 组合 → 名称）
    pub fn device_name(&self) -> &'static str {
        device_name(self.vendor_id, self.device_id)
    }
}

impl fmt::Display for PciDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}.{} {:04X}:{:04X} class={:02X}{:02X}{:02X} rev={:02X}",
            self.bus, self.dev, self.func,
            self.vendor_id, self.device_id,
            self.class, self.subclass, self.prog_if, self.revision,
        )
    }
}

/// 基类/子类 → 类别名称
pub const fn class_name(class: u8, subclass: u8) -> &'static str {
    match class {
        0x00 => "Unclassified",
        0x01 => match subclass {
            0x00 => "SCSI", 0x01 => "IDE", 0x02 => "Floppy",
            0x06 => "SATA", 0x08 => "NVMe", _ => "Storage",
        },
        0x02 => match subclass {
            0x00 => "Ethernet", 0x01 => "Token Ring", 0x03 => "Wireless",
            0x05 => "CAN", _ => "Network",
        },
        0x03 => match subclass {
            0x00 => "VGA", 0x01 => "XGA", 0x02 => "3D", _ => "Display",
        },
        0x04 => "Multimedia",
        0x05 => "Memory",
        0x06 => match subclass {
            0x00 => "Host Bridge", 0x01 => "ISA Bridge", 0x04 => "PCI-PCI Bridge",
            0x05 => "CardBus", 0x06 => "RapidIO", 0x07 => "Reserved",
            0x08 => "PCMCIA", _ => "Bridge",
        },
        0x07 => match subclass {
            0x00 => "Serial", 0x01 => "Parallel", 0x05 => "Modem",
            0x80 => "Serial 16550", _ => "Communication",
        },
        0x08 => match subclass {
            0x00 => "PIC", 0x01 => "DMA", 0x02 => "Timer",
            0x03 => "RTC", 0x04 => "PCI Hotplug", 0x05 => "SD Host",
            0x06 => "IOMMU", _ => "System",
        },
        0x09 => "Input",
        0x0B => "Processor",
        0x0C => match subclass {
            0x00 => "FireWire", 0x03 => "USB", 0x05 => "SMBus",
            0x06 => "InfiniBand", _ => "Serial Bus",
        },
        0x11 => "Signal Processing",
        0x13 => "Non-Essential",
        0x40 => "Co-Processor",
        0xFF => "Unassigned",
        _ => "Unknown",
    }
}

/// 已知厂商 ID → 名称
pub fn vendor_name(vendor_id: u16) -> &'static str {
    match vendor_id {
        0x8086 => "Intel",
        0x1022 => "AMD",
        0x1234 => "Bochs/QEMU",
        0x1AF4 => "VirtIO",
        0x104B => "QEMU(BusLogic)",
        0x10EC => "Realtek",
        0x10DE => "NVIDIA",
        0x1002 => "ATI/AMD",
        0x1106 => "VIA",
        0x14E4 => "Broadcom",
        0x15AD => "VMware",
        0x80EE => "VirtualBox",
        0x1B36 => "Red Hat/QEMU",
        _ => "Unknown",
    }
}

/// 已知 (厂商, 设备) 组合 → 设备名称
pub fn device_name(vendor_id: u16, device_id: u16) -> &'static str {
    match (vendor_id, device_id) {
        // QEMU 默认机型常见设备
        (0x1234, 0x1111) => "Bochs VGA",
        (0x8086, 0x1237) => "PIIX3 Host Bridge",
        (0x8086, 0x7000) => "PIIX3 ISA Bridge",
        (0x8086, 0x7010) => "PIIX3 IDE Controller",
        (0x8086, 0x7113) => "PIIX4 Power Management",
        // 网卡
        (0x8086, 0x100E) => "Intel 82540EM (e1000)",
        (0x8086, 0x100F) => "Intel 82545EM (e1000)",
        (0x8086, 0x10D3) => "Intel 82574L (e1000e)",
        (0x8086, 0x1533) => "Intel I210",
        (0x8086, 0x15F2) => "Intel I225-V (igc)",
        (0x8086, 0x125B) => "Intel I226-V (igc)",
        (0x10EC, 0x8139) => "Realtek RTL8139",
        (0x10EC, 0x8168) => "Realtek RTL8168",
        // 存储
        (0x1AF4, 0x1001) => "VirtIO Block (legacy)",
        (0x1AF4, 0x1042) => "VirtIO Block (modern)",
        (0x1AF4, 0x1002) => "VirtIO SCSI",
        (0x1AF4, 0x1044) => "VirtIO SCSI (modern)",
        (0x8086, 0x5845) => "NVMe Controller (QEMU)",
        (0x1B36, 0x0009) => "QEMU AHCI Controller",
        // 网络（virtio）
        (0x1AF4, 0x1000) => "VirtIO Net (legacy)",
        (0x1AF4, 0x1041) => "VirtIO Net (modern)",
        (0x1AF4, 0x1003) => "VirtIO Console",
        (0x1AF4, 0x1005) => "VirtIO RNG",
        (0x1AF4, 0x1009) => "VirtIO 9P",
        (0x1AF4, 0x1052) => "VirtIO Input Keyboard",
        (0x1AF4, 0x1053) => "VirtIO Input Tablet",
        (0x1AF4, 0x1050) => "VirtIO GPU",
        // 其他
        (0x8086, 0x2415) => "ICH4 Audio",
        // q35 平台常见设备
        (0x8086, 0x29C0) => "ICH9 Host Bridge",
        (0x8086, 0x2918) => "ICH9 LPC Bridge",
        (0x8086, 0x2922) => "ICH9 SATA Controller (AHCI)",
        (0x8086, 0x2930) => "ICH9 SMBus Controller",
        (0x1106, 0x3038) => "VIA USB UHCI",
        _ => "Unknown Device",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_names() {
        assert_eq!(class_name(0x03, 0x00), "VGA");
        assert_eq!(class_name(0x02, 0x00), "Ethernet");
        assert_eq!(class_name(0x01, 0x08), "NVMe");
        assert_eq!(class_name(0x06, 0x04), "PCI-PCI Bridge");
        assert_eq!(class_name(0xFF, 0x00), "Unassigned");
    }

    #[test]
    fn test_vendor_and_device_names() {
        assert_eq!(vendor_name(0x8086), "Intel");
        assert_eq!(vendor_name(0x1AF4), "VirtIO");
        assert_eq!(vendor_name(0x1234), "Bochs/QEMU");
        assert_eq!(device_name(0x8086, 0x100E), "Intel 82540EM (e1000)");
        assert_eq!(device_name(0x1AF4, 0x1042), "VirtIO Block (modern)");
        assert_eq!(device_name(0x1234, 0x1111), "Bochs VGA");
    }

    #[test]
    fn test_device_flags() {
        let d = PciDevice {
            bus: 0, dev: 1, func: 0,
            vendor_id: 0x8086, device_id: 0x7000,
            class: 0x06, subclass: 0x01, prog_if: 0, revision: 0,
            header_type: 0x00,
            subsystem_vendor: 0, subsystem_id: 0,
            interrupt_pin: 0, interrupt_line: 0,
        };
        assert!(!d.is_bridge());
        assert!(!d.is_multifunction());
        assert_eq!(d.class_name(), "ISA Bridge");

        let bridge = PciDevice { header_type: 0x01, ..d };
        assert!(bridge.is_bridge());

        let multifunc = PciDevice { header_type: 0x80, ..d };
        assert!(multifunc.is_multifunction());
    }
}