// ============================================================
// AXIS 内核主入口
// ============================================================

#![no_std]
#![no_main]

mod panic;

use axis_kernel::prelude::*;
use axis_kernel::config;

/// 内核入口点
///
/// 由引导加载程序跳转到此函数
///
/// 初始化流程：
/// 1. 打印启动 Banner
/// 2. 初始化架构相关代码（CPU、GDT、IDT、中断）
/// 3. 初始化内存管理
/// 4. 启动主循环
#[unsafe(no_mangle)]
pub extern "C" fn _boot_rust() -> ! {
    // 清屏（VGA 缓冲区经物理内存映射区访问，见 config::VGA_TEXT_BUFFER）
    axis_kernel::libcore::vga::clear_screen();

    // 打印启动 Banner
    print_banner();

    // 架构初始化
    println!("\n[INIT] Initializing architecture...");
    axis_kernel::arch::init();

    // 内存管理初始化（物理内存 → 堆 → 虚拟内存）
    println!("\n[INIT] Initializing memory management...");
    if let Err(e) = axis_kernel::mm::init() {
        println!("[INIT] MM init failed: {:?}", e);
    } else {
        // 打印内存概况
        let p = axis_kernel::mm::pmm::stats();
        println!("[MM] {} zones ready", p.zone_count);
        for i in 0..p.zone_count {
            let z = &p.zones[i];
            println!(
                "  Zone[{}] type={} pages={} free={} watermark={}",
                i, z.ty, z.total, z.free, z.level
            );
        }

        // 内存管理自测（单元验证 + 压力测试，满足验收标准）
        axis_kernel::mm::selftest();

        // 输出监控统计
        axis_kernel::mm::print_stats();
    }

    // 系统就绪
    println!("\n[INIT] System initialized successfully!");
    println!("[INIT] Kernel is now running...");

    // 再次打印启动 Banner
    print_banner();

    // 主循环
    // 暂时只是挂起等待中断
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// 打印启动 Banner
fn print_banner() {
    println!("{}", config::KERNEL_BANNER);
    println!("  {}: {} v{}", config::KERNEL_NAME, config::KERNEL_SLOGAN, config::KERNEL_VERSION);
    println!("  Copyright (C) {}.", config::KERNEL_AUTHOR);
}
