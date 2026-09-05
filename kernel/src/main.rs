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
    axis_kernel::lib::vga::clear_screen();

    // 打印启动 Banner
    print_banner();

    // 架构初始化
    println!("\n[INIT] Initializing architecture...");
    axis_kernel::arch::init();

    // 内存管理初始化（物理内存 → 堆 → 虚拟内存）
    println!("\n[INIT] Initializing memory management...");
    axis_kernel::mm::init();

    // 任务子系统初始化（任务表、调度器、进程树根）
    println!("\n[INIT] Initializing task subsystem...");
    axis_kernel::task::init();

    // 文件系统初始化（VFS、tmpfs、devfs、procfs、sysfs）
    println!("\n[INIT] Initializing file system...");
    axis_kernel::fs::init();

    // 网络协议栈初始化（IPv4/IPv6、TCP/UDP、Socket、ARP）
    println!("\n[INIT] Initializing network stack...");
    axis_kernel::net::init();

    // 系统就绪
    println!("\n[INIT] System initialized successfully!");
    println!("[INIT] Kernel is now running...");

    // 再次打印启动 Banner
    print_banner();

    // 开调度：init 与 3 个演示任务进入就绪队列；
    // 此后主循环成为 idle 任务，由定时器中断驱动切换
    axis_kernel::task::start_scheduling();
    println!("\n[TASK] Starting scheduling... (4 kernel threads, CFS preemption)");

    // 主循环
    // 挂起等待中断
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
