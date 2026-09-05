// ============================================================
// AHCI 驱动（SATA 高级主机控制器接口）
// ============================================================
// AHCI HBA 探测与寄存器层实现。
//
// 实现状态：
// ✅ 完成：PCI 探测（类别 01/06）、BAR5（HBA 内存）映射、
//   端口实现位读取与设备连接状态（PxSSTS）检测
// ⏳ 待完成（后续阶段）：
//   - 命令列表/接收 FIS 区建立与端口命令引擎（PxCMD.ST）
//   - SATA 命令构造（IDENTIFY / READ / WRITE）
//   - BlkDevice 实现与注册
// 因此 probe 目前只报告端口与设备状态，不注册可用块设备。

use super::BlkDevice;

/// AHCI HBA 寄存器偏移（BAR5 内存）
#[allow(dead_code)]
mod reg {
    /// 主机能力（32 位）：NP = ports-1
    pub const CAP: u64 = 0x00;
    /// 全局主机控制
    pub const GHC: u64 = 0x04;
    /// 端口实现（位 i = 端口 i 存在）
    pub const PI: u64 = 0x0C;
    /// HBA 版本
    pub const VS: u64 = 0x10;
    /// 每个端口的寄存器块大小
    pub const PORT_STRIDE: u64 = 0x80;
    /// 端口寄存器起始
    pub const PORT_BASE: u64 = 0x100;
}

/// 端口寄存器偏移（相对端口块）
///
/// 名称沿用 AHCI 规范（PxCMD/PxSSTS...），非标准大写常量名
#[allow(dead_code, non_upper_case_globals)]
mod port_reg {
    /// 端口命令与状态
    pub const PxCMD: u64 = 0x18;
    /// 串行 ATA 状态（DET 字段 bits 0-3）
    pub const PxSSTS: u64 = 0x28;
    /// 串行 ATA 错误
    pub const PxSERR: u64 = 0x30;
    /// 端口中断状态
    pub const PxIS: u64 = 0x10;
}

unsafe fn read_reg(base: u64, offset: u64) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

/// 探测 AHCI 控制器（SATA 类别 01/06）
///
/// 找到时打印 HBA 与各端口状态；命令引擎未建立，返回 None。
#[allow(clippy::needless_range_loop)]
pub fn probe() -> Option<alloc::boxed::Box<dyn BlkDevice>> {
    let devices = crate::drivers::pci::find_by_class(0x01, 0x06);
    let dev = devices.first().copied()?;

    // AHCI 的 HBA 寄存器在 BAR5（通常 32 位）
    let bar5 = unsafe { dev.read_bar(5)? };
    let phys = (bar5 & 0xFFFF_FFF0) as u64;
    if phys == 0 {
        println!("[AHCI] controller at {:02X}:{:02X}.{} has no BAR5", dev.bus, dev.dev, dev.func);
        return None;
    }
    let base = crate::config::PHYSICAL_MEMORY_OFFSET + phys;

    unsafe {
        let cap = read_reg(base, reg::CAP);
        let pi = read_reg(base, reg::PI);
        let vs = read_reg(base, reg::VS);
        let ports = ((cap & 0x1F) + 1) as usize;
        println!("[AHCI] controller {:02X}:{:02X}.{} caps ports={} PI=0x{:08X} VS={}.{}",
            dev.bus, dev.dev, dev.func, ports, pi, vs >> 8, vs & 0xFF);

        // 逐个已实现端口检查设备连接
        let mut present = 0;
        for p in 0..ports {
            if pi & (1 << p) == 0 {
                continue;
            }
            let pbase = base + reg::PORT_BASE + (p as u64) * reg::PORT_STRIDE;
            let ssts = read_reg(pbase, port_reg::PxSSTS);
            let det = ssts & 0x0F; // 3 = 设备在线
            let ipm = (ssts >> 8) & 0x0F;
            if det == 3 {
                present += 1;
                println!("[AHCI]   port {}: device attached (SSTS=0x{:X})", p, ssts);
            } else {
                println!("[AHCI]   port {}: no device (SSTS=0x{:X} det={} ipm={})", p, ssts, det, ipm);
            }
        }
        println!("[AHCI] {} port(s) with device, command engine pending (后续阶段)", present);
    }

    None
}