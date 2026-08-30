// ============================================================
// 抢占调度（preemption）
// ============================================================
// 抢占判定与时间片记账（纯逻辑层）。
//
// 什么时候应该发生抢占（Linux CFS 的两条触发路径）：
// 1. 时间片耗尽：任务把本轮 sched_slice 用完，need_resched
//    置位，下一次调度点让出 CPU
// 2. vruntime 落后过多：新任务入队后其 vruntime 比当前
//    任务小超过一个"最小抢占粒度"，立即让位——保证
//    交互式（短睡眠）任务不被长计算任务饿死
//
// 为什么需要最小抢占粒度：
// - 无粒度限制会导致任务间频繁乒乓切换（vruntime 差
//   1 tick 就切），上下文切换开销吃掉全部收益；
//   粒度就是"切换开销 vs 响应延迟"的平衡点
//
// 为什么设计为纯逻辑模块：
// - 判定只需要两个数字（当前/最小 vruntime）比较，
//   与硬件无关；真正的切换动作由调度器在 tick/唤醒
//   路径上调用本模块的判定结果执行

/// 最小抢占粒度（vruntime 差超过该值才允许抢占）
///
/// 单位与 vruntime 一致（tick 数 @ 1kHz）；取 4 意味着
/// 任务间至少相差 4 个 tick 的公平量才切换
pub const MIN_PREEMPT_GRANULARITY: u64 = 4;

/// 判定：当前任务是否应被 vruntime 更小的任务抢占
///
/// # 为什么用饱和减而不是直接比较：
/// - 就绪队列为空（min_vr 不存在）时调用方不应调用；
///   此处以饱和减防御异常输入，返回 false
#[inline]
pub fn should_preempt(current_vruntime: u64, min_vruntime: u64) -> bool {
    current_vruntime.saturating_sub(min_vruntime) >= MIN_PREEMPT_GRANULARITY
}

/// 时间片记账：每 tick 扣减剩余时间片，耗尽时置抢占标志
///
/// # 参数
/// - `ticks_left`: 剩余时间片（就地扣减）
/// - `need_resched`: 需要重调度标志（耗尽时置位）
///
/// 返回 true 表示本轮 tick 触发了抢占请求。
/// 为什么记账与判定分离：
/// - 时间片属于任务（thread.rs），判定属于策略（本模块）；
///   分离后两边都能独立测试
#[inline]
pub fn tick_charge(ticks_left: &mut u64, need_resched: &mut bool) -> bool {
    if *ticks_left > 0 {
        *ticks_left -= 1;
        // 仅在"扣到 0 的那一拍"报告一次（边沿触发），
        // 避免耗尽后的重复 tick 反复置位
        if *ticks_left == 0 {
            *need_resched = true;
            return true;
        }
    }
    false
}

/// 任务被选中运行时：装载新时间片并清除抢占标志
#[inline]
pub fn charge_slice(ticks_left: &mut u64, need_resched: &mut bool, slice: u64) {
    *ticks_left = slice;
    *need_resched = false;
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_should_preempt_threshold() {
        // 差 3 tick：不足粒度，不抢占
        assert!(!should_preempt(103, 100));
        // 恰好差 4：触发
        assert!(should_preempt(104, 100));
        // 落后很多：必然触发
        assert!(should_preempt(1000, 100));
        // 领先于队首：不触发
        assert!(!should_preempt(90, 100));
    }

    #[test]
    fn test_tick_charge() {
        let (mut left, mut resched) = (3u64, false);
        // 前两 tick 不触发
        assert!(!tick_charge(&mut left, &mut resched));
        assert!(!tick_charge(&mut left, &mut resched));
        assert_eq!(left, 1);
        assert!(!resched);
        // 最后一 tick 耗尽 → 置位
        assert!(tick_charge(&mut left, &mut resched));
        assert!(resched);
        // 置位后重复 tick 不重复触发（已经耗尽）
        assert!(!tick_charge(&mut left, &mut resched));
    }

    #[test]
    fn test_charge_slice_resets() {
        let (mut left, mut resched) = (0u64, true);
        charge_slice(&mut left, &mut resched, 24);
        assert_eq!(left, 24);
        assert!(!resched);
    }
}
