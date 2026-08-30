// ============================================================
// x86_64 中断处理
// ============================================================
// 中断和异常的高层处理逻辑

pub mod apic;
pub mod handler;
pub mod timer;
pub mod ioapic;
pub mod msi;

use crate::arch::x86_64::idt::InterruptStackFrame;

/// 初始化中断系统
///
/// 初始化顺序：
/// 1. 禁用 PIC（8259A）- 使用 APIC 代替
/// 2. 初始化 Local APIC
/// 3. 初始化 I/O APIC
/// 4. 初始化定时器
/// 5. 启用中断
pub fn init() {
    // 禁用传统的 8259A PIC
    // 为什么要禁用：APIC 是更现代的中断控制器，性能更好
    unsafe {
        disable_pic();
    }

    // 初始化 APIC
    unsafe {
        apic::init();
        ioapic::init();
    }

    // 将 IRQ 0-15 的中断存根（entry.asm 中的 irq_N_handler）
    // 安装到 IDT 的向量 32-47。
    // 为什么在这里做：IDT 已在 idt::init() 中只安装了 CPU 异常，
    // 硬件中断向量必须由中断子系统补全；若不安装，定时器中断
    // 首次触发就会命中空的 IDT 表项而崩溃。
    unsafe {
        super::idt::install_irq_handlers();
    }

    // 初始化定时器
    timer::init();

    // 启用中断
    // 为什么现在才启用：前面的初始化必须在中断禁用状态下完成
    unsafe {
        core::arch::asm!("sti", options(nostack, preserves_flags));
    }

    println!("[INTERRUPT] Interrupt system initialized");
}

/// 禁用 8259A PIC
///
/// 为什么需要禁用：
/// - 现代系统使用 APIC，不需要 PIC
/// - PIC 和 APIC 同时启用会造成冲突
/// - PIC 的中断向量范围与 CPU 异常重叠
///
/// 禁用方式：
/// - 将所有 IRQ 屏蔽掉（写入 0xFF 到 IMR）
unsafe fn disable_pic() {
    use core::arch::asm;

    unsafe {
        // PIC1（主片）IMR = 0x21
        // PIC2（从片）IMR = 0xA1
        // 写入 0xFF 屏蔽所有中断

        // 主片
        asm!(
            "mov al, 0xFF",
            "out 0x21, al",
            options(nostack, preserves_flags)
        );

        // 从片
        asm!(
            "mov al, 0xFF",
            "out 0xA1, al",
            options(nostack, preserves_flags)
        );
    }
}

/// 处理异常
///
/// 从汇编存根调用，统一处理所有 CPU 异常
pub fn handle_exception(vector: usize, error_code: u64, frame: &InterruptStackFrame) {
    handler::handle_exception(vector, error_code, frame);
}

/// 处理硬件中断
///
/// 从汇编存根调用；返回"新的栈指针"（0 = 不切换上下文）：
/// - 定时器中断路径可能触发调度切换，返回目标任务的
///   保存帧 RSP，由 entry.asm 在恢复寄存器前换栈
/// - 其余中断返回 0
#[unsafe(no_mangle)]
pub extern "C" fn handle_irq(vector: u64, frame: &InterruptStackFrame) -> usize {
    handler::handle_irq(vector as usize, frame)
}
