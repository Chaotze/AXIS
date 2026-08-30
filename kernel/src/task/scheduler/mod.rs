// ============================================================
// 调度器（scheduler）根模块
// ============================================================
// 聚合 CFS 就绪队列、抢占策略、CPU 亲和与负载均衡，
// 提供"单核调度器"的最小装配结构。
//
// 分层说明：
// - cfs / preemption / cpu_affinity / load_balance 为纯算法
//   模块：不引用全局状态，可被 unitest 直接编译测试
// - 本模块的 Scheduler 仍是纯结构（无全局锁）：它只把
//   CFS 队列与"当前任务"两个概念绑在一起；全局调度器
//   实例与 tick 钩子在 task/mod.rs 装配
//
// 为什么本轮 Scheduler 不直接做上下文切换：
// - 上下文切换需要 arch 层（TrapFrame、switch.asm）与
//   每任务内核栈的配合，属于下一轮接线内容；本结构
//   的接口（pick_next / yield_current）即切换点，
//   接线时无需改动数据结构

pub mod cfs;
pub mod cpu_affinity;
pub mod load_balance;
pub mod preemption;

pub use cfs::{
    calc_vruntime_delta, nice_to_weight, sched_slice_ticks, CfsRunqueue, VrKey, NICE_0_LOAD,
};
pub use cpu_affinity::CpuMask;
pub use load_balance::LoadBalancer;
pub use preemption::{charge_slice, should_preempt, tick_charge};

use super::thread::{Tid, INVALID_TID};

/// 单核调度器装配（就绪队列 + 当前任务）
pub struct Scheduler<const MAX_TASKS: usize, const MAX_NODES: usize> {
    /// CFS 就绪队列
    pub runqueue: CfsRunqueue<MAX_TASKS, MAX_NODES>,
    /// 当前运行中的线程（未切换过为 INVALID_TID）
    pub current: Tid,
}

impl<const MAX_TASKS: usize, const MAX_NODES: usize> Scheduler<MAX_TASKS, MAX_NODES> {
    /// 创建调度器（初始无当前任务）
    pub fn new() -> Self {
        Self {
            runqueue: CfsRunqueue::new(),
            current: INVALID_TID,
        }
    }

    /// 新任务进入调度视野：按 vruntime 入队
    pub fn admit(&mut self, tid: Tid, vruntime: u64) -> Result<VrKey, &'static str> {
        self.runqueue.enqueue(tid, vruntime)
    }

    /// 选取下一个应运行的任务（不改变队列状态）
    ///
    /// 为什么选取与出队分离：
    /// - 调用方（tick/唤醒路径）可能只想知道"下一个是谁"
    ///   用于抢占判定；真正切换时再调用 switch_in
    pub fn pick_next(&self) -> Option<Tid> {
        self.runqueue.pick_next().map(|(_, tid)| tid)
    }

    /// 当前任务让出 CPU：按其新 vruntime 重新入队
    ///
    /// # 参数
    /// - `key`: 当前任务入队时的树内键（出队定位用）
    /// - `new_vruntime`: 让出后的 vruntime（已含本轮增量）
    pub fn yield_current(&mut self, key: VrKey, new_vruntime: u64) -> Result<VrKey, &'static str> {
        self.runqueue.requeue(key, self.current, new_vruntime)
    }

    /// 切换到给定任务：从队列取出并设为当前
    ///
    /// 切换语义：运行中的任务不在就绪队列里；
    /// 该任务下次让出时用 switch_in 返回时的新键重入队。
    pub fn switch_in(&mut self, key: VrKey) -> bool {
        let tid = key.tid;
        if self.runqueue.dequeue(key) {
            self.current = tid;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_admit_and_pick() {
        let mut sched: Scheduler<8, 24> = Scheduler::new();
        assert_eq!(sched.current, INVALID_TID);
        assert_eq!(sched.pick_next(), None);

        sched.admit(1, 500).unwrap();
        sched.admit(2, 300).unwrap();
        // vruntime 小者优先
        assert_eq!(sched.pick_next(), Some(2));
        assert_eq!(sched.runqueue.len(), 2);
    }

    #[test]
    fn test_yield_current_round_robin() {
        let mut sched: Scheduler<8, 24> = Scheduler::new();
        let k1 = sched.admit(1, 0).unwrap();
        let k2 = sched.admit(2, 0).unwrap();
        sched.current = 1;

        // 任务 1 运行后让出（vruntime 前进 24）
        let _k1_new = sched.yield_current(k1, 24).unwrap();
        assert_eq!(sched.pick_next(), Some(2));
        sched.current = 2;
        let _k2_new = sched.yield_current(k2, 24).unwrap();
        // 1 与 2 的 vruntime 相同，按 tid 平局：1 先
        assert_eq!(sched.pick_next(), Some(1));
    }

    #[test]
    fn test_switch_in() {
        let mut sched: Scheduler<8, 24> = Scheduler::new();
        let k1 = sched.admit(1, 100).unwrap();
        let k2 = sched.admit(2, 50).unwrap();

        // 切换到 vruntime 更小的 2
        let (key, _) = sched.runqueue.pick_next().unwrap();
        assert_eq!(key.tid, 2);
        assert!(sched.switch_in(key));
        assert_eq!(sched.current, 2);
        assert_eq!(sched.runqueue.len(), 1); // 运行中的任务不在队列里
        let _ = (k1, k2);
    }
}
