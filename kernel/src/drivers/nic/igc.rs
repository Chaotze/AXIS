// ============================================================
// Intel igc 网卡驱动（i225/i226）
// ============================================================
// 新一代 Intel 2.5GbE 控制器（igc 系列）。
//
// 实现状态：
// ✅ 完成：PCI 探测与设备识别（i225-V/i226-V）
// ⏳ 待完成（后续阶段）：
//   - 寄存器基址映射与 NVM（SPI flash）读 MAC
//   - 描述符环与收发通路（与 e1000 共用上层，仅寄存器集不同）
//
// 默认 QEMU 不提供 igc 设备；探测到未知设备时静默返回。

use crate::prelude::KernelResult;

/// igc 设备 ID（i225/i226 部分常用型号）
const IGC_DEVICE_IDS: &[u16] = &[
    0x15F2, // I225-V
    0x15F3, // I225-IT
    0x15F4, // I225-LM
    0x15F5, // I225-K
    0x15F6, // I225-K2
    0x15F7, // I225-K3
    0x15FC, // I225-K2
    0x125B, // I226-V
    0x125C, // I226-LM
];

/// 探测 igc 网卡（当前仅识别与报告）
pub fn probe() -> KernelResult<()> {
    for &id in IGC_DEVICE_IDS {
        if let Some(dev) = crate::drivers::pci::find(0x8086, id) {
            println!("[IGC] {:02X}:{:02X}.{} i225/i226 controller detected (ID 0x{:04X})",
                dev.bus, dev.dev, dev.func, id);
            println!("[IGC] NVM/寄存器初始化与收发通路待后续阶段");
            return Ok(());
        }
    }
    Ok(())
}