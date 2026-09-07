// ============================================================
// 中断和异常处理器
// ============================================================
// 实现具体的异常和中断处理逻辑

use crate::arch::x86_64::idt::InterruptStackFrame;

/// 异常名称表
const EXCEPTION_NAMES: [&str; 32] = [
    "Divide Error",                     // 0
    "Debug",                            // 1
    "Non-Maskable Interrupt",           // 2
    "Breakpoint",                       // 3
    "Overflow",                         // 4
    "BOUND Range Exceeded",             // 5
    "Invalid Opcode",                   // 6
    "Device Not Available",             // 7
    "Double Fault",                     // 8
    "Coprocessor Segment Overrun",      // 9
    "Invalid TSS",                      // 10
    "Segment Not Present",              // 11
    "Stack-Segment Fault",              // 12
    "General Protection",               // 13
    "Page Fault",                       // 14
    "Reserved",                         // 15
    "x87 FPU Error",                    // 16
    "Alignment Check",                  // 17
    "Machine Check",                    // 18
    "SIMD Floating-Point Exception",    // 19
    "Virtualization Exception",         // 20
    "Reserved", "Reserved", "Reserved", "Reserved", "Reserved",
    "Reserved", "Reserved", "Reserved", "Reserved", "Reserved", "Reserved",
];

/// 处理 CPU 异常
///
/// 异常处理策略：
/// - 可恢复的异常（如页错误）：尝试修复后返回
/// - 不可恢复的异常：打印调试信息后 panic
pub fn handle_exception(vector: usize, error_code: u64, frame: &InterruptStackFrame) {
    match vector {
        14 => handle_page_fault(error_code, frame),
        8 => handle_double_fault(error_code, frame),
        13 => handle_general_protection(error_code, frame),
        _ => handle_generic_exception(vector, error_code, frame),
    }
}

/// 页错误处理
///
/// 页错误原因（错误码位）：
/// - bit 0 (P): 0 = 页不存在, 1 = 页保护冲突
/// - bit 1 (W/R): 0 = 读操作, 1 = 写操作
/// - bit 2 (U/S): 0 = 在内核模式, 1 = 在用户模式
/// - bit 3 (RSVD): 1 = 保留位被设置
/// - bit 4 (I/D): 1 = 指令取指
///
/// CR2 寄存器：保存触发页错误的虚拟地址
///
/// 处理流程：先把问题交给 VMM 的缺页处理器（按需分页 / COW /
/// 交换换入）；VMM 无法解决（非法访问、内核 bug）时再打印现场并 panic。
fn handle_page_fault(error_code: u64, frame: &InterruptStackFrame) {
    // 读取 CR2（发生错误的地址）
    let fault_addr: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) fault_addr, options(nostack, preserves_flags));
    }

    // 交给内存管理层的统一缺页处理器
    // 为什么放到最前面尝试解决：缺页中的绝大多数是“按需分页”等
    // 可恢复事件，先尝试解决；只有解决不了才走完整诊断输出
    if crate::mm::vmm::handle_page_fault(fault_addr, error_code)
        == crate::mm::vmm::PageFaultResult::Resolved
    {
        return;
    }

    // 解析错误码
    let present = error_code & 0x1 != 0;
    let write = error_code & 0x2 != 0;
    let user = error_code & 0x4 != 0;
    let reserved = error_code & 0x8 != 0;
    let instruction = error_code & 0x10 != 0;

    println!("\n!!! PAGE FAULT (UNRESOLVED) !!!");
    println!("Fault address: 0x{:016X}", fault_addr);
    println!("Error code: 0x{:X}", error_code);
    println!("  Present: {}", present);
    println!("  Write: {}", write);
    println!("  User: {}", user);
    println!("  Reserved: {}", reserved);
    println!("  Instruction: {}", instruction);
    print_interrupt_frame(frame);

    // VMM 无法解决（非法访问/内核 bug），panic
    panic!("Unresolved page fault at 0x{:016X}", fault_addr);
}

/// 双重故障处理
///
/// 双重故障发生的原因：
/// - 处理一个异常时又发生了另一个异常
/// - 例如：缺页异常的处理程序本身触发了缺页
///
/// 为什么需要独立的栈（IST）：
/// - 双重故障常常是栈溢出导致的
/// - 如果继续使用同一个栈，会立即触发三重故障（CPU 复位）
fn handle_double_fault(error_code: u64, frame: &InterruptStackFrame) {
    println!("\n!!! DOUBLE FAULT !!!");
    println!("Error code: 0x{:X}", error_code);
    print_interrupt_frame(frame);

    panic!("Double fault - this is usually caused by stack overflow");
}

/// 一般保护错误处理
///
/// GPF 的常见原因：
/// - 访问了空指针或无效地址
/// - 段寄存器加载了无效的选择子
/// - 违反了特权级规则
/// - 执行了特权指令
fn handle_general_protection(error_code: u64, frame: &InterruptStackFrame) {
    println!("\n!!! GENERAL PROTECTION FAULT !!!");
    println!("Error code: 0x{:X}", error_code);

    // 错误码包含段选择子信息
    if error_code != 0 {
        let external = error_code & 0x1 != 0;
        let table = (error_code >> 1) & 0x3;
        let index = (error_code >> 3) & 0x1FFF;

        println!("Segment selector:");
        println!("  External: {}", external);
        println!("  Table: {} (0=GDT, 1=IDT, 2/3=LDT)", table);
        println!("  Index: {}", index);
    }

    print_interrupt_frame(frame);

    panic!("General protection fault");
}

/// 通用异常处理
fn handle_generic_exception(vector: usize, error_code: u64, frame: &InterruptStackFrame) {
    let name = if vector < 32 {
        EXCEPTION_NAMES[vector]
    } else {
        "Unknown Exception"
    };

    println!("\n!!! EXCEPTION: {} (Vector {}) !!!", name, vector);
    if error_code != 0 {
        println!("Error code: 0x{:X}", error_code);
    }
    print_interrupt_frame(frame);

    panic!("Unhandled exception: {}", name);
}

/// 处理硬件中断
///
/// 返回新的栈指针（0 = 不切换）：定时器中断路径可能触发
/// 调度切换，见 interrupt/entry.asm 的换栈说明。
pub fn handle_irq(vector: usize, frame: &InterruptStackFrame) -> usize {
    match vector {
        32 => {
            // 定时器中断：时钟记账 → EOI → 调度 tick 钩子（可能切换）
            handle_timer_interrupt(frame)
        }
        33 => {
            // 键盘（IRQ1）：读取扫描码并解码入队
            crate::drivers::input::keyboard_irq();
            super::apic::send_eoi();
            0
        }
        44 => {
            // 鼠标（IRQ12）：读取数据包字节并解码入队
            crate::drivers::input::mouse_irq();
            super::apic::send_eoi();
            0
        }
        _ => {
            println!("Unhandled IRQ: {}", vector);
            // 发送 EOI
            super::apic::send_eoi();
            0
        }
    }
}

/// 定时器中断处理
///
/// 返回新栈指针（0 = 不切换）。
/// 顺序为什么是 tick → EOI → 调度：
/// - EOI 在切换前发出，LAPIC 不必等新任务运行就能继续收中断
/// - 调度决策（tick_hook）持有任务锁，在中断上下文（IF=0）安全
fn handle_timer_interrupt(frame: &InterruptStackFrame) -> usize {
    // 调用定时器模块的处理函数
    super::timer::handle_tick();

    // 发送 EOI
    super::apic::send_eoi();

    // 调度 tick 钩子：真实上下文切换
    // frame 指向 CPU 压入的 rip 处；其上 17 个槽（15 通用寄存器
    // + 向量 + 伪错误码）是存根的保存区——"RSP 指向 r15 槽"的
    // 保存帧地址 = frame 地址 - 17*8
    let saved_rsp = (frame as *const InterruptStackFrame as usize) - 17 * 8;
    crate::task::tick_hook(saved_rsp)
}

/// 打印中断帧信息
fn print_interrupt_frame(frame: &InterruptStackFrame) {
    println!("Interrupt frame:");
    println!("  RIP:    0x{:016X}", frame.rip);
    println!("  CS:     0x{:016X}", frame.cs);
    println!("  RFLAGS: 0x{:016X}", frame.rflags);
    println!("  RSP:    0x{:016X}", frame.rsp);
    println!("  SS:     0x{:016X}", frame.ss);
}
