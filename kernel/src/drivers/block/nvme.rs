// ============================================================
// NVMe 驱动（NVM Express）
// ============================================================
// NVMe 控制器探测与寄存器层实现。
//
// 实现状态：
// ✅ 完成：PCI 探测（类别 01/08/02）、BAR0 映射、寄存器读取
//   （CAP/VS/CSTS）、控制器信息打印
// ⏳ 待完成（后续阶段）：
//   - 管理队列（ASQ/ACQ）建立与 Identify 命令
//   - I/O 队列建立与提交/完成队列门铃操作
//   - 命名空间容量读取与读写命令（BlkDevice 实现）
// 因此 probe 目前只报告控制器存在，不注册可用的块设备。

use super::BlkDevice;

/// NVMe BAR0 寄存器偏移
#[allow(dead_code)]
mod reg {
    /// 控制器能力（64 位）
    pub const CAP: u64 = 0x0000;
    /// 版本（32 位）
    pub const VS: u64 = 0x0008;
    /// 控制器状态（32 位）
    pub const CSTS: u64 = 0x001C;
    /// 中断屏蔽
    pub const INTMS: u64 = 0x000C;
    /// 中断清除
    pub const INTMC: u64 = 0x0010;
    /// 控制器配置（32 位）
    pub const CC: u64 = 0x0014;
    /// 管理队列属性
    pub const AQA: u64 = 0x0024;
    /// 管理提交队列基址（64 位）
    pub const ASQ: u64 = 0x0028;
    /// 管理完成队列基址（64 位）
    pub const ACQ: u64 = 0x0030;
    /// SQ0 门铃（+ 4<<DSTRD * sqid）
    pub const SQ0TDBL: u64 = 0x1000;
}

/// 读取 NVMe 控制器寄存器（BAR0 经物理内存映射区访问）
unsafe fn read_reg(base: u64, offset: u64) -> u32 {
    unsafe {
        core::ptr::read_volatile((base + offset) as *const u32)
    }
}

unsafe fn read_reg64(base: u64, offset: u64) -> u64 {
    unsafe {
        core::ptr::read_volatile((base + offset) as *const u64)
    }
}

/// 探测 NVMe 控制器
///
/// 找到设备时打印寄存器信息；由于管理队列尚未建立，返回 None
/// （不注册可用设备）。实机/QEMU 挂载 NVMe 盘时可确认探测输出。
pub fn probe() -> Option<alloc::boxed::Box<dyn BlkDevice>> {
    // PCI 类别：01（存储）/ 08（NVM Express）/ 02（NVMHCI）
    let devices = crate::drivers::pci::find_by_class(0x01, 0x08);
    let dev = devices.into_iter().find(|d| d.prog_if == 0x02)?;

    // BAR0/BAR1 组成 64 位 BAR
    let bar0 = unsafe { dev.read_bar(0)? };
    let bar1 = unsafe { dev.read_bar(1)? };
    let phys = (bar0 & 0xFFFF_FFF0) as u64 | ((bar1 as u64) << 32);
    if phys == 0 {
        println!("[NVME] controller at {:02X}:{:02X}.{} has no BAR0", dev.bus, dev.dev, dev.func);
        return None;
    }
    let base = crate::config::PHYSICAL_MEMORY_OFFSET + phys;

    unsafe {
        let cap = read_reg64(base, reg::CAP);
        let vs = read_reg(base, reg::VS);
        let csts = read_reg(base, reg::CSTS);
        let mqes = (cap & 0xFFFF) as u16;          // 最大队列深度
        let dstrd = (cap >> 32) & 0xF;             // 门铃步长指数
        let to = ((cap >> 24) & 0xFF) as u32;      // 超时（500ms 单位）
        let ver_major = vs >> 16;
        let ver_minor = vs & 0xFFFF;
        let ready = csts & 1;
        println!("[NVME] controller {:02X}:{:02X}.{} NVMe {}.{} MQES={} DSTRD={} TO={} RDY={} BAR=0x{:X}",
            dev.bus, dev.dev, dev.func, ver_major, ver_minor, mqes, dstrd, to, ready, phys);
        println!("[NVME] admin queue setup pending (后续阶段)");
    }

    // 管理队列未建立前不注册可用设备
    None
}