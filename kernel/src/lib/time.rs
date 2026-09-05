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
    unsafe { ticks_to_ms_freq(ticks, TICK_FREQUENCY) }
}

/// 将滴答数转换为微秒
#[inline]
pub fn ticks_to_us(ticks: u64) -> u64 {
    unsafe { ticks_to_us_freq(ticks, TICK_FREQUENCY) }
}

/// 按给定频率将滴答数转换为毫秒（纯函数）
///
/// # 为什么拆分出带频率参数的纯函数：
/// - 换算规则本身与全局状态无关，纯函数可独立测试、
///   可被"频率尚未确定"的路径复用
/// - 全局频率版本只是薄封装，两处换算永不分叉
#[inline]
pub const fn ticks_to_ms_freq(ticks: u64, freq: u64) -> u64 {
    ticks * 1000 / freq
}

/// 按给定频率将滴答数转换为微秒（纯函数）
#[inline]
pub const fn ticks_to_us_freq(ticks: u64, freq: u64) -> u64 {
    ticks * 1_000_000 / freq
}

/// 按给定频率将滴答数换算为（秒, 纳秒）（纯函数）
///
/// # 为什么需要（秒, 纳秒）二元组：
/// - VDSO 的 __vdso_clock_gettime 与内核时钟共享同一套
///   换算规则（阶段 2.2.4 交付物）；集中在此保证一致
/// - 纳秒 = 不足一秒的余数按比例放大，避免浮点运算
#[inline]
pub const fn ticks_to_secs_nanos_freq(ticks: u64, freq: u64) -> (u64, u64) {
    let secs = ticks / freq;
    let nanos = (ticks % freq) * 1_000_000_000 / freq;
    (secs, nanos)
}

/// 将全局 tick 计数换算为（秒, 纳秒）
#[inline]
pub fn ticks_to_secs_nanos(ticks: u64) -> (u64, u64) {
    unsafe { ticks_to_secs_nanos_freq(ticks, TICK_FREQUENCY) }
}

/// 开机以来的单调时间（秒, 纳秒）
///
/// 单调时钟不受墙钟调整影响，是测量"时间间隔"的
/// 唯一正确选择（gettimeofday 之类墙钟另有用途）。
#[inline]
pub fn monotonic_secs_nanos() -> (u64, u64) {
    ticks_to_secs_nanos(get_ticks())
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

/// 获取当前时间戳（用于文件系统的 inode 时间戳）
///
/// 返回自启动以来的秒数（作为伪 Unix 时间戳）。在完整 RTC 支持前，
/// 这提供了单调递增的时间戳用于 atime/mtime/ctime。
///
/// 为什么这样设计：
/// - AXIS 内核还未集成 BIOS 或 RTC 读取，无法获得墙钟时间
/// - 文件系统的 inode 元数据需要时间戳以支持 ls -la 等命令
/// - 使用开机以来的秒数作为临时方案，确保时间单调递增
/// - 将来 RTC 集成后，可直接替换返回值为真实 Unix 时间戳
#[inline]
pub fn current_timestamp_secs() -> i64 {
    uptime_seconds() as i64
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

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_ticks_to_ms_us_freq() {
        // 1000Hz：tick 与毫秒 1:1
        assert_eq!(ticks_to_ms_freq(1500, 1000), 1500);
        assert_eq!(ticks_to_us_freq(1, 1000), 1000);
        // 3000Hz：3 tick = 1ms
        assert_eq!(ticks_to_ms_freq(3000, 3000), 1000);
        assert_eq!(ticks_to_us_freq(3000, 3000), 1_000_000);
    }

    #[test]
    fn test_ticks_to_secs_nanos_freq() {
        // 1000Hz：1500 tick = 1.5 秒整
        assert_eq!(ticks_to_secs_nanos_freq(1500, 1000), (1, 500_000_000));
        // 3GHz：1 tick = 1/3 纳秒级 → 按整纳秒截断
        assert_eq!(ticks_to_secs_nanos_freq(1, 3_000_000_000), (0, 0));
        assert_eq!(ticks_to_secs_nanos_freq(3_000_000_000, 3_000_000_000), (1, 0));
        // 1Hz：tick 与秒 1:1
        assert_eq!(ticks_to_secs_nanos_freq(2, 1), (2, 0));
    }
}
