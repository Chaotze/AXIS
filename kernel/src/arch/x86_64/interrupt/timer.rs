// ============================================================
// 定时器管理
// ============================================================
// 系统定时器（APIC Timer）

use crate::config::TIMER_FREQUENCY;

/// 初始化定时器
///
/// 使用 APIC Timer 作为系统定时器
/// 相比 PIT（8254），APIC Timer 是每个 CPU 独立的，支持多核
pub fn init() {
    unsafe {
        init_apic_timer();
    }

    println!("[TIMER] Timer initialized at {}Hz", TIMER_FREQUENCY);
}

/// 初始化 APIC Timer
///
/// APIC Timer 有三种模式：
/// - One-shot: 计数到 0 后停止
/// - Periodic: 计数到 0 后自动重载
/// - TSC-Deadline: 基于 TSC（时间戳计数器）
///
/// 我们使用 Periodic 模式作为系统定时器
unsafe fn init_apic_timer() {
    unsafe {
        // APIC Timer 寄存器地址
        const LAPIC_BASE: u64 = 0xFEE00000;
        const LVT_TIMER: u32 = 0x320;
        const TIMER_DIV: u32 = 0x3E0;
        const TIMER_INIT: u32 = 0x380;

        // 设置分频器为 128
        // Divide Configuration Register:
        // 0000 = divide by 2
        // 0001 = divide by 4
        // ...
        // 1010 = divide by 128
        let div_reg = (LAPIC_BASE + TIMER_DIV as u64) as *mut u32;
        core::ptr::write_volatile(div_reg, 0b1010);

        // 配置 LVT Timer 寄存器
        // bits 17: Timer Mode (0=one-shot, 1=periodic)
        // bits 16: Mask (0=enabled, 1=masked)
        // bits 7-0: Vector
        let timer_vector = 32; // IRQ 0 映射到向量 32
        let lvt_timer = (LAPIC_BASE + LVT_TIMER as u64) as *mut u32;
        let lvt_value = (1 << 17) | timer_vector; // Periodic mode + vector
        core::ptr::write_volatile(lvt_timer, lvt_value);

        // 设置初始计数值
        // APIC Timer 频率 = CPU频率 / 分频器
        // 假设 CPU 频率为 2GHz，分频器为 128
        // Timer频率 = 2GHz / 128 = 15.625MHz
        // 要达到 1000Hz，计数值 = 15.625MHz / 1000 = 15625
        //
        // 实际应该通过校准获取准确值，这里用估算值
        let init_count = 15625 * (TIMER_FREQUENCY / 1000);
        let init_reg = (LAPIC_BASE + TIMER_INIT as u64) as *mut u32;
        core::ptr::write_volatile(init_reg, init_count);
    }
}

/// 定时器中断处理
///
/// 由中断处理程序调用
pub fn handle_tick() {
    // 更新系统时钟
    crate::libcore::time::tick();

    // 后续：调度器时间片检查
}
