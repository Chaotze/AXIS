// ============================================================
// PCIe ECAM（Enhanced Configuration Access Mechanism）
// ============================================================
// PCIe 的配置空间访问机制：把整个配置空间映射到一段内存，
// 每个设备/功能占 4KB，通过内存读写替代端口 I/O。
//
// ECAM 段信息来自 ACPI MCFG 表。本模块只做地址换算（纯函数，
// 可宿主单元测试）；真正的内存映射访问依赖 MCFG 提供的基址
// （物理地址经直接映射区访问）。

/// 每个设备/功能配置空间的大小（4KB，标准要求）
pub const ECAM_DEVICE_SIZE: u64 = 0x1000;
/// 每台设备占据的字节数（8 个功能 × 4KB）
pub const ECAM_FUNC_STRIDE: u64 = 0x1000;
/// 每台设备 8 个功能
pub const ECAM_DEVICE_STRIDE: u64 = 0x8000;
/// 每条总线 32 台设备
pub const ECAM_BUS_STRIDE: u64 = 0x100_000;

/// 一个 MCFG 描述的 ECAM 段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EcamRegion {
    /// ECAM 基址（物理地址）
    pub base_address: u64,
    /// PCI 段号
    pub segment: u16,
    /// 起始总线号
    pub start_bus: u8,
    /// 结束总线号
    pub end_bus: u8,
}

impl EcamRegion {
    /// 该段是否覆盖指定的 (段, 总线)
    pub const fn contains(&self, segment: u16, bus: u8) -> bool {
        self.segment == segment && bus >= self.start_bus && bus <= self.end_bus
    }

    /// 计算指定设备配置空间的物理地址（纯函数）
    ///
    /// 布局：基址 + 总线偏移(1MB) + 设备偏移(32KB) + 功能偏移(4KB) + 寄存器偏移
    pub const fn address(&self, bus: u8, dev: u8, func: u8, offset: u16) -> Option<u64> {
        if !self.contains(self.segment, bus) || dev > 31 || func > 7 || offset >= 0x1000 {
            return None;
        }
        Some(
            self.base_address
                + ((bus as u64 - self.start_bus as u64) * ECAM_BUS_STRIDE)
                + ((dev as u64) * ECAM_DEVICE_STRIDE)
                + ((func as u64) * ECAM_FUNC_STRIDE)
                + (offset as u64),
        )
    }
}

/// 从 MCFG 段分配表构建 ECAM 段列表（纯函数）
pub fn regions_from_mcfg(allocations: &[(u64, u16, u8, u8)]) -> alloc::vec::Vec<EcamRegion> {
    allocations.iter().map(|&(base, seg, start, end)| EcamRegion {
        base_address: base,
        segment: seg,
        start_bus: start,
        end_bus: end,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecam_address() {
        let region = EcamRegion {
            base_address: 0xE000_0000,
            segment: 0,
            start_bus: 0,
            end_bus: 255,
        };
        // 总线 0、设备 0、功能 0、偏移 0 → 基址
        assert_eq!(region.address(0, 0, 0, 0), Some(0xE000_0000));
        // 总线 1、设备 0、功能 0 → 基址 + 1MB
        assert_eq!(region.address(1, 0, 0, 0), Some(0xE010_0000));
        // 总线 0、设备 1、功能 0 → 基址 + 32KB
        assert_eq!(region.address(0, 1, 0, 0), Some(0xE000_8000));
        // 总线 0、设备 0、功能 1 → 基址 + 4KB
        assert_eq!(region.address(0, 0, 1, 0), Some(0xE000_1000));
        // 寄存器偏移叠加
        assert_eq!(region.address(0, 2, 3, 0x10), Some(0xE000_0000 + 2 * 0x8000 + 3 * 0x1000 + 0x10));
        // 越界拒绝
        assert_eq!(region.address(0, 32, 0, 0), None);
        assert_eq!(region.address(0, 0, 8, 0), None);
        assert_eq!(region.address(0, 0, 0, 0x1000), None);
    }

    #[test]
    fn test_ecam_contains() {
        let region = EcamRegion {
            base_address: 0xE000_0000,
            segment: 0,
            start_bus: 0,
            end_bus: 0x3F,
        };
        assert!(region.contains(0, 0));
        assert!(region.contains(0, 0x3F));
        assert!(!region.contains(0, 0x40));
        assert!(!region.contains(1, 0));
    }
}