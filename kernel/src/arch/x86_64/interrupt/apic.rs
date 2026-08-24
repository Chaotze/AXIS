// ============================================================
// Local APIC 管理
// ============================================================
// 高级可编程中断控制器（本地部分）

/// Local APIC 基地址（默认）
const LAPIC_BASE: u64 = 0xFEE00000;

/// Local APIC 寄存器偏移
#[allow(dead_code)]
mod reg {
    pub const ID: u32 = 0x20;           // Local APIC ID
    pub const VERSION: u32 = 0x30;      // Local APIC Version
    pub const TPR: u32 = 0x80;          // Task Priority Register
    pub const PPR: u32 = 0xA0;          // Processor Priority Register
    pub const EOI: u32 = 0xB0;          // End of Interrupt
    pub const SPURIOUS: u32 = 0xF0;     // Spurious Interrupt Vector
    pub const ICR_LOW: u32 = 0x300;     // Interrupt Command Register (低32位)
    pub const ICR_HIGH: u32 = 0x310;    // Interrupt Command Register (高32位)
    pub const LVT_TIMER: u32 = 0x320;   // LVT Timer Register
    pub const LVT_LINT0: u32 = 0x350;   // LVT LINT0 Register
    pub const LVT_LINT1: u32 = 0x360;   // LVT LINT1 Register
    pub const LVT_ERROR: u32 = 0x370;   // LVT Error Register
    pub const TIMER_INIT: u32 = 0x380;  // Timer Initial Count
    pub const TIMER_CURRENT: u32 = 0x390; // Timer Current Count
    pub const TIMER_DIV: u32 = 0x3E0;   // Timer Divide Configuration
}

/// 读 APIC 寄存器
///
/// # Safety
/// 必须确保 APIC 已启用且地址映射正确
#[inline]
unsafe fn read_reg(offset: u32) -> u32 {
    unsafe {
        let addr = (LAPIC_BASE + offset as u64) as *const u32;
        core::ptr::read_volatile(addr)
    }
}

/// 写 APIC 寄存器
///
/// # Safety
/// 必须确保 APIC 已启用且地址映射正确
#[inline]
unsafe fn write_reg(offset: u32, value: u32) {
    unsafe {
        let addr = (LAPIC_BASE + offset as u64) as *mut u32;
        core::ptr::write_volatile(addr, value);
    }
}

/// 初始化 Local APIC
///
/// 步骤：
/// 1. 启用 APIC（通过 MSR 和 Spurious 寄存器）
/// 2. 配置 LVT 表项
/// 3. 设置任务优先级
pub unsafe fn init() {
    unsafe {
        // 启用 APIC 通过 IA32_APIC_BASE MSR
        // 这一步通常在启动时已完成

        // 启用 APIC 通过 Spurious Interrupt Vector Register
        // bit 8 = 软件启用/禁用
        // bits 0-7 = spurious 向量号（通常设为 0xFF）
        let spurious = read_reg(reg::SPURIOUS);
        write_reg(reg::SPURIOUS, spurious | 0x1FF); // 启用 + 向量 0xFF

        // 设置任务优先级为 0（接受所有中断）
        write_reg(reg::TPR, 0);

        // 配置 LVT 表项
        // 暂时屏蔽所有 LVT 中断
        write_reg(reg::LVT_TIMER, 0x10000);  // 屏蔽定时器
        write_reg(reg::LVT_LINT0, 0x10000);  // 屏蔽 LINT0
        write_reg(reg::LVT_LINT1, 0x10000);  // 屏蔽 LINT1
        write_reg(reg::LVT_ERROR, 0x10000);  // 屏蔽错误

        println!("[APIC] Local APIC initialized");
    }
}

/// 发送 EOI（End of Interrupt）
///
/// 中断处理完成后必须调用，通知 APIC 可以发送下一个中断
///
/// 为什么需要 EOI：
/// - APIC 在收到 EOI 之前不会发送相同或更低优先级的中断
/// - 防止中断处理程序被重入
#[inline]
pub fn send_eoi() {
    unsafe {
        write_reg(reg::EOI, 0);
    }
}

/// 获取 Local APIC ID
#[allow(dead_code)]
pub fn get_id() -> u32 {
    unsafe {
        read_reg(reg::ID) >> 24
    }
}
