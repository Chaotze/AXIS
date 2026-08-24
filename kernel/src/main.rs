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
    // 清屏
    clear_screen();

    // 打印启动 Banner
    print_banner();

    // 架构初始化
    println!("\n[INIT] Initializing architecture...");
    axis_kernel::arch::init();

    // 系统就绪
    println!("\n[INIT] System initialized successfully!");
    println!("[INIT] Kernel is now running...\n");

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

/// 清屏
fn clear_screen() {
    const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;
    const WIDTH: usize = 80;
    const HEIGHT: usize = 25;
    const BLANK: u16 = 0x0720; // 黑底白字空格

    unsafe {
        for i in 0..(WIDTH * HEIGHT) {
            VGA_BUFFER.add(i).write_volatile(BLANK);
        }
    }
}
