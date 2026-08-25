// ============================================================
// 时间相关功能
// ============================================================
// 提供时间测量和延迟功能

use core::sync::atomic::{AtomicU64, Ordering};

/// 系统启动后的时钟滴答数
static TICKS: AtomicU64 = AtomicU64::new(0);

/// 时钟频率（Hz）
static mut TICK_FREQUENCY: u64 = 1000; // 默认 1000Hz = 1ms per tick

/// 增加时钟滴答
///
/// 由定时器中断调用
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// 获取当前时钟滴答数
#[inline]
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// 设置时钟频率
///
/// # Safety
/// 必须在系统初始化时调用，之后不应修改
pub unsafe fn set_tick_frequency(hz: u64) {
    unsafe {
        TICK_FREQUENCY = hz;
    }
}

/// 将滴答数转换为毫秒
#[inline]
pub fn ticks_to_ms(ticks: u64) -> u64 {
    unsafe { ticks * 1000 / TICK_FREQUENCY }
}

/// 将滴答数转换为微秒
#[inline]
pub fn ticks_to_us(ticks: u64) -> u64 {
    unsafe { ticks * 1_000_000 / TICK_FREQUENCY }
}

/// 获取自启动以来的毫秒数
#[inline]
pub fn uptime_ms() -> u64 {
    ticks_to_ms(get_ticks())
}

/// 获取自启动以来的秒数
#[inline]
pub fn uptime_seconds() -> u64 {
    uptime_ms() / 1000
}

/// 忙等待指定的毫秒数
///
/// 注意：这是忙等待，会占用 CPU
/// 只应在早期初始化或短时间延迟时使用
pub fn delay_ms(ms: u64) {
    let start = get_ticks();
    let target = start + unsafe { ms * TICK_FREQUENCY / 1000 };

    while get_ticks() < target {
        core::hint::spin_loop();
    }
}
