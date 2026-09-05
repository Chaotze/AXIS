// ============================================================
// 设备驱动层根模块
// ============================================================
// 聚合内核的全部硬件设备驱动，并提供统一初始化入口与启动自测。
//
// 模块结构（与 dev/arch.md 对齐）：
//   serial/   串口（16550）
//   display/  显示（帧缓冲 / UEFI GOP / VESA）
//   input/    输入（HID / PS/2 键盘 / 鼠标）
//   block/    块设备（请求队列 / I/O 调度器 / NVMe / AHCI / virtio）
//   nic/      网络接口卡（e1000 / igc / virtio-net）
//   pci/      PCI 总线（配置空间 / 设备枚举 / ECAM / DMA / IOMMU）
//   acpi/     ACPI（RSDP / RSDT / FADT / MADT / MCFG）
//
// 分层约定：纯算法/寄存器语义模块（pci/config、acpi/parse、
// block/io_scheduler 等）不依赖 arch 与全局锁，可宿主单元测试；
// 装配层（本模块、serial/mod、pci/mod 等）承担全局状态、锁与
// arch 对接，由内核启动自测验证。
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

// 以下子模块随阶段 6 推进逐个接入：
// pub mod block;
// pub mod nic;

/// 设备驱动初始化入口（由 main.rs 调用）
///
/// 顺序为什么如此：串口先行保证任何后续初始化日志都可镜像到
/// 串口；ACPI 先于 PCI（MCFG 提供 ECAM 段）；PCI 先于各设备
/// 驱动（探测依赖枚举结果）。
pub fn init() {
    // 1. 串口驱动
    println!("[DRV] Initializing serial driver...");
    serial::init();

    // 2. ACPI 表解析（PCI 与电源管理依赖）
    println!("[DRV] Initializing ACPI...");
    let _ = acpi::init();

    // 3. PCI 枚举（各设备驱动依赖）
    println!("[DRV] Enumerating PCI...");
    let _ = pci::init();

    // 4. 显示驱动
    println!("[DRV] Initializing display...");
    let _ = display::init();

    // 5. 输入驱动（PS/2 键盘/鼠标，IRQ1/12）
    println!("[DRV] Initializing input...");
    input::init();

    // 后续子系统初始化随模块接入逐个开启：
    // block::init(); nic::init();

    // 运行设备驱动自测
    selftest();
}

/// 设备驱动启动自测入口（返回是否全部通过）
pub fn selftest() -> bool {
    println!("\n[DRV-SELFTEST] Device Drivers Selftest");
    let mut all = true;
    all &= t("serial 16550", serial::selftest());
    all &= t("acpi tables", acpi::selftest());
    all &= t("pci enumeration", pci::selftest());
    all &= t("display framebuffer", display::selftest());
    all &= t("input ps2 decoders", input::selftest());
    // 后续子系统的自测由各自模块的 selftest 提供，随模块接入逐步开启
    println!("[DRV-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}

/// 单测断言容器：记录并打印结果
fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}