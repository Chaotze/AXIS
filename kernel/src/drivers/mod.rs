// 设备驱动层根模块
// ============================================================
// 聚合内核的全部硬件设备驱动，并提供统一初始化入口与启动自测。
//
// 模块结构：
//   serial/   串口（16550）
//   display/  显示（帧缓冲 / UEFI GOP / VESA）
//   input/    输入（HID / PS/2 键盘 / 鼠标）
//   block/    块设备（请求队列 / I/O 调度器 / NVMe / AHCI / virtio）
//   nic/      网络接口卡（e1000 / igc / virtio-net + 回环网卡）
//   pci/      PCI 总线（配置空间 / 设备枚举 / ECAM / DMA / IOMMU）
//   acpi/     ACPI（RSDP / RSDT / FADT / MADT / MCFG）
//
// 分层约定：纯算法/寄存器语义模块（pci/config、acpi/parse 等）
// 不依赖 arch 与全局锁，可宿主单元测试；装配层承担全局状态、
// 锁与 arch 对接，由内核启动自测验证。
//
// 初始化顺序（drivers::init）：
// 1. 串口：最早就绪，日志镜像立刻可用
// 2. ACPI：解析固件表（RSDT/XSDT），PCI 的 ECAM 依赖 MCFG
// 3. PCI：枚举总线，驱动按 vendor/device 探测硬件
// 4. 显示 / 输入 / 块 / 网卡：基于 PCI 与固定端口探测

pub mod serial;
pub mod acpi;
pub mod pci;
pub mod display;
pub mod input;
pub mod block;
pub mod nic;

/// 设备驱动初始化入口（由 main.rs 调用）
pub fn init() {
    // 全程关中断（irqsave 纪律）：驱动初始化与自测会频繁分配
    // 若被定时器 tick 打断，中断路径与初始化路径可能互相干扰
    let flags = crate::arch::x86_64::cpu::irq_save();

    println!("[DRV] Initializing serial driver...");
    serial::init();

    println!("[DRV] Initializing ACPI...");
    let _ = acpi::init();

    println!("[DRV] Enumerating PCI...");
    let _ = pci::init();

    println!("[DRV] Initializing display...");
    let _ = display::init();

    println!("[DRV] Initializing input...");
    input::init();

    println!("[DRV] Initializing block devices...");
    block::init();

    println!("[DRV] Initializing NICs...");
    let _ = nic::init();

    selftest();

    unsafe {
        crate::arch::x86_64::cpu::irq_restore(flags);
    }
}

/// 设备驱动启动自测入口
pub fn selftest() -> bool {
    println!("\n[DRV-SELFTEST] Device Drivers Selftest");
    let mut all = true;
    all &= t("serial 16550", serial::selftest());
    all &= t("acpi tables", acpi::selftest());
    all &= t("pci enumeration", pci::selftest());
    all &= t("display framebuffer", display::selftest());
    all &= t("input ps2 decoders", input::selftest());
    all &= t("block devices", block::selftest());
    all &= t("nic drivers", nic::selftest());
    println!("[DRV-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}

fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}
