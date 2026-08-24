// ============================================================
// x86_64 架构支持
// ============================================================
// 提供 x86_64 架构的底层硬件访问和初始化

pub mod cpu;
pub mod gdt;
pub mod idt;
pub mod memory;
pub mod paging;
pub mod interrupt;
pub mod context;

/// x86_64 架构初始化
///
/// 初始化顺序至关重要：
/// 1. CPU 特性检测和启用（SSE、SMEP/SMAP 等）
/// 2. GDT 和 TSS（任务状态段）设置
/// 3. IDT（中断描述符表）初始化
/// 4. 中断控制器（APIC）初始化
/// 5. 页表和内存管理初始化
///
/// 为什么按这个顺序：
/// - CPU 特性必须最先启用，后续代码可能依赖这些特性
/// - GDT 必须在 IDT 之前，因为中断门需要引用 GDT 中的代码段
/// - IDT 必须在启用中断之前设置好
/// - APIC 初始化需要访问内存映射寄存器，可能依赖页表
pub fn init() {
    // 1. 检测并启用 CPU 特性
    unsafe {
        cpu::init();
    }

    // 2. 设置 GDT 和 TSS
    gdt::init();

    // 3. 初始化 IDT
    idt::init();

    // 4. 初始化中断系统
    interrupt::init();

    println!("[ARCH] x86_64 initialization complete");
}
