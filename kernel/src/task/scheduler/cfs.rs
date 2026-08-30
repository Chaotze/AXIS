// ============================================================
// CFS（完全公平调度器）核心
// ============================================================
// 基于虚拟运行时间（vruntime）的公平调度：权重换算、
// vruntime 记账与就绪队列（纯逻辑层）。
//
// CFS 的核心思想（为什么要用 vruntime）：
// - 真实 CPU 时间按权重分配；把真实时间"缩放"为虚拟时间：
//   高权重任务 vruntime 走得慢（同样的真实时间只涨一点），
//   低权重任务走得快——调度器永远选 vruntime 最小的任务，
//   长期看每个任务获得与权重成正比的时间片
// - vruntime 是唯一标尺：不需要时间片轮转、不需要优先级
//   队列，公平性由"每次选最小 vruntime"自动保证
//
// 为什么就绪队列复用 lib 的 BTreeMap（偏离 roadmap 的"红黑树"）：
// - 红黑树与 B 树的调度场景职责相同（按 vruntime 有序、
//   取最小值、插入删除）；BTreeMap 已经过完整测试
//   （分裂/借位/合并/不变量校验），重复实现红黑树违反
//   "低重复冗余"，且未经测试的新树风险更高
// - 树高 O(log N) 同级；将来若 profile 显示红黑树必要，
//   仅需替换本模块内部容器，接口不变（已写入 arch.md 修订）
//
// 为什么容量是 const 泛型：
// - 任务数编译期确定（MAX_TASKS），节点池内嵌零堆开销；
//   Rust 稳定版不支持 const 泛型算术，节点数上界
//   （MAX_NODES ≥ 2×MAX_TASKS+8）由调用方显式传入，
//   编译期断言校验

use super::super::super::lib::collections::btree::BTreeMap;
use super::super::thread::Tid;

/// nice = 0 的基准权重（vruntime 换算的参照系）
pub const NICE_0_LOAD: u64 = 1024;

/// nice 的取值范围（Linux 语义）
pub const NICE_MIN: i8 = -20;
pub const NICE_MAX: i8 = 19;

/// nice → 权重表
///
/// Linux 公式：weight = 1024 / 1.25^nice
/// 为什么用编译期查表：
/// - 表只有 40 项且公式涉及分数幂，运行时迭代计算每个
///   调度点都会重复；const fn 生成后查表 O(1)
const fn make_weight_table() -> [u32; 40] {
    let mut table = [0u32; 40];
    let mut i = 0;
    while i < 40 {
        let nice = i as i32 - 20;
        let mut weight: u64 = NICE_0_LOAD;
        // 正整数幂：乘 4 除 5（1/1.25）
        let mut n = nice;
        while n > 0 {
            weight = weight * 4 / 5;
            n -= 1;
        }
        // 负整数幂：乘 5 除 4（1.25）
        while n < 0 {
            weight = weight * 5 / 4;
            n += 1;
        }
        table[i] = weight as u32;
        i += 1;
    }
    table
}

/// 权重表（编译期常量）
static WEIGHT_TABLE: [u32; 40] = make_weight_table();

/// 查询 nice 对应的 CFS 权重
#[inline]
pub fn nice_to_weight(nice: i8) -> u32 {
    let index = (nice.clamp(NICE_MIN, NICE_MAX) + 20) as usize;
    WEIGHT_TABLE[index]
}

/// 按权重把真实运行时间（tick 数）换算为 vruntime 增量
///
/// 公式：Δvr = Δreal × NICE_0_LOAD / weight
/// - nice=0 时 weight=1024，vruntime 与真实时间 1:1
/// - 高权重（nice<0）：Δvr < Δreal，vruntime 走得慢 → 更常被选中
#[inline]
pub fn calc_vruntime_delta(delta_ticks: u64, weight: u32) -> u64 {
    delta_ticks * NICE_0_LOAD / weight as u64
}

/// 计算单个任务的时间片（tick 数）
///
/// Linux 语义：调度周期 = max(最小周期, 每任务延迟 × 任务数)，
/// 任务按权重占周期比例获得时间片。
/// 为什么用固定参数而非可调：
/// - 内核配置阶段把常量集中在 cfs.rs，sysctl 化留待
///   系统调用层（阶段 8）
#[inline]
pub fn sched_slice_ticks(nr_running: usize, weight: u32, total_weight: u32) -> u64 {
    const MIN_PERIOD_TICKS: u64 = 24; // 最小调度周期（24ms @ 1kHz）
    const LATENCY_PER_TASK: u64 = 6; // 每任务的期望调度延迟
    let period = MIN_PERIOD_TICKS.max(LATENCY_PER_TASK * nr_running.max(1) as u64);
    // 按权重占比分片；至少 1 tick 保证推进
    let share = period * weight as u64 / total_weight.max(1) as u64;
    share.max(1)
}

/// 就绪队列键：vruntime 相同用 tid 打破平局（保证唯一）
///
/// 为什么需要二元组键：
/// - 两个任务 vruntime 完全相等是常态（公平调度的收敛点），
///   BTreeMap 键必须唯一；键序即调度顺序：
///   先比 vruntime（小者优先），再比 tid（稳定打破平局）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VrKey {
    /// 虚拟运行时间
    pub vruntime: u64,
    /// 线程号（平局裁决）
    pub tid: Tid,
}

impl VrKey {
    pub const fn new(vruntime: u64, tid: Tid) -> Self {
        Self { vruntime, tid }
    }
}

/// CFS 就绪队列（定长 B 树，键 = VrKey，值 = Tid）
pub struct CfsRunqueue<const MAX_TASKS: usize, const MAX_NODES: usize> {
    tree: BTreeMap<VrKey, Tid, 3, 4>,
    /// 已入队任务数
    len: usize,
}

impl<const MAX_TASKS: usize, const MAX_NODES: usize> CfsRunqueue<MAX_TASKS, MAX_NODES> {
    /// 创建空就绪队列
    pub fn new() -> Self {
        // 2-3-4 树最坏节点数 < 2×键数（每节点至少 1 键），
        // 留 8 个余量防边界
        assert!(MAX_NODES >= 2 * MAX_TASKS + 8, "MAX_NODES 不足：至少 2*MAX_TASKS+8");
        Self {
            tree: BTreeMap::new(),
            len: 0,
        }
    }

    /// 队内任务数
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 入队
    ///
    /// 返回该任务在树中的键（出队/重入队需要原键定位）。
    pub fn enqueue(&mut self, tid: Tid, vruntime: u64) -> Result<VrKey, &'static str> {
        if self.len >= MAX_TASKS {
            return Err("就绪队列已满");
        }
        let key = VrKey::new(vruntime, tid);
        self.tree.insert(key, tid);
        self.len += 1;
        Ok(key)
    }

    /// 出队（按入队时返回的键）
    pub fn dequeue(&mut self, key: VrKey) -> bool {
        if self.tree.remove(&key).is_some() {
            self.len -= 1;
            true
        } else {
            false
        }
    }

    /// 出队后按新 vruntime 重新入队（时间片用完的标准动作）
    pub fn requeue(&mut self, old_key: VrKey, tid: Tid, new_vruntime: u64) -> Result<VrKey, &'static str> {
        self.dequeue(old_key);
        self.enqueue(tid, new_vruntime)
    }

    /// 选取下一个运行任务（vruntime 最小者）
    ///
    /// 不改变队列（调用方在任务实际运行时再出队/重入队，
    /// 避免"选中但切换失败"的中间态破坏队列）
    pub fn pick_next(&self) -> Option<(VrKey, Tid)> {
        self.tree.first().map(|(k, v)| (*k, *v))
    }

    /// 队列最小 vruntime（抢占判定与 load_balance 用）
    pub fn min_vruntime(&self) -> Option<u64> {
        self.tree.first().map(|(k, _)| k.vruntime)
    }

    /// 队列最大 vruntime
    pub fn max_vruntime(&self) -> Option<u64> {
        self.tree.last().map(|(k, _)| k.vruntime)
    }

    /// 队列中所有任务 vruntime 的离散度（max-min）
    ///
    /// 用途：公平性验证——长期运行后该值应收敛到
    /// 时间片量级；自测与监控接口都以此判断公平
    pub fn vruntime_spread(&self) -> u64 {
        match (self.min_vruntime(), self.max_vruntime()) {
            (Some(min), Some(max)) => max - min,
            _ => 0,
        }
    }
}

/// 队列是否含给定任务（按 tid 全树遍历——仅调试/自测用）
impl<const MAX_TASKS: usize, const MAX_NODES: usize> CfsRunqueue<MAX_TASKS, MAX_NODES> {
    pub fn contains_tid(&self, tid: Tid) -> bool {
        let mut found = false;
        self.tree.visit(&mut |_, v| {
            if *v == tid {
                found = true;
            }
        });
        found
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_weight_table() {
        // nice 0 → 1024（基准）
        assert_eq!(nice_to_weight(0), 1024);
        // 权重随 nice 单调下降
        assert!(nice_to_weight(-20) > nice_to_weight(19));
        // 对称性抽查：nice=1 → 1024*4/5 = 819
        assert_eq!(nice_to_weight(1), 819);
        // 越界被钳制
        assert_eq!(nice_to_weight(-100), nice_to_weight(-20));
    }

    #[test]
    fn test_vruntime_delta_scaling() {
        // nice 0：1:1
        assert_eq!(calc_vruntime_delta(100, nice_to_weight(0)), 100);
        // 高权重（nice=-20，weight=88761）：vr 走得慢
        let fast = calc_vruntime_delta(100, nice_to_weight(-20));
        let slow = calc_vruntime_delta(100, nice_to_weight(19));
        assert!(fast < 100 && slow > 100);
    }

    #[test]
    fn test_sched_slice_proportional() {
        // 两任务同权重：对半
        let total = nice_to_weight(0) * 2;
        let s = sched_slice_ticks(2, nice_to_weight(0), total);
        assert!(s >= 6); // 周期 ≥ max(24, 6*2)=24 的一半附近
        // 高权重任务分得更多
        let big = nice_to_weight(-5);
        let small = nice_to_weight(5);
        let total2 = big + small;
        assert!(sched_slice_ticks(2, big, total2) > sched_slice_ticks(2, small, total2));
    }

    #[test]
    fn test_runqueue_order() {
        let mut rq: CfsRunqueue<16, 40> = CfsRunqueue::new();
        assert!(rq.is_empty());

        // 乱序入队：调度顺序必须按 vruntime 升序
        let k1 = rq.enqueue(1, 300).unwrap();
        let k2 = rq.enqueue(2, 100).unwrap();
        let k3 = rq.enqueue(3, 200).unwrap();

        assert_eq!(rq.len(), 3);
        let (key, tid) = rq.pick_next().unwrap();
        assert_eq!(tid, 2);
        assert_eq!(key.vruntime, 100);
        assert_eq!(rq.min_vruntime(), Some(100));
        assert_eq!(rq.max_vruntime(), Some(300));
        assert_eq!(rq.vruntime_spread(), 200);

        // 出队最小者后，次小者成为队首
        rq.dequeue(key);
        assert_eq!(rq.pick_next().unwrap().1, 3);
        assert!(rq.contains_tid(1));
        assert!(!rq.contains_tid(2));
        assert_eq!(k1.vruntime, 300);
        assert_eq!(k2.tid, 2);
        assert_eq!(k3.vruntime, 200);
    }

    #[test]
    fn test_requeue_advances() {
        let mut rq: CfsRunqueue<16, 40> = CfsRunqueue::new();
        let key = rq.enqueue(7, 1000).unwrap();
        // 任务运行后 vruntime 前进，重新入队排到后面
        let new_key = rq.requeue(key, 7, 2000).unwrap();
        assert_eq!(new_key.vruntime, 2000);
        assert_eq!(rq.min_vruntime(), Some(2000));
        // 旧键已失效
        assert!(!rq.dequeue(key));
        assert!(rq.dequeue(new_key));
    }

    #[test]
    fn test_fairness_convergence() {
        // 模拟两个任务（权重 3:1）反复调度，
        // 验证 vruntime 差距收敛（公平性验收的核心指标）
        let mut rq: CfsRunqueue<4, 16> = CfsRunqueue::new();
        let (heavy_w, light_w) = (nice_to_weight(-5), nice_to_weight(5));
        let (mut heavy_vr, mut light_vr) = (0u64, 0u64);

        let run_heavy = |rq: &mut CfsRunqueue<4, 16>, vr: &mut u64| {
            let key = VrKey::new(*vr, 1);
            *vr += calc_vruntime_delta(12, heavy_w);
            let _ = rq.requeue(key, 1, *vr);
        };
        let run_light = |rq: &mut CfsRunqueue<4, 16>, vr: &mut u64| {
            let key = VrKey::new(*vr, 2);
            *vr += calc_vruntime_delta(12, light_w);
            let _ = rq.requeue(key, 2, *vr);
        };

        let k1 = rq.enqueue(1, 0).unwrap();
        let k2 = rq.enqueue(2, 0).unwrap();
        let _ = (k1, k2);

        for _ in 0..100 {
            let (_, tid) = rq.pick_next().unwrap();
            if tid == 1 {
                run_heavy(&mut rq, &mut heavy_vr);
            } else {
                run_light(&mut rq, &mut light_vr);
            }
        }

        // 100 轮后两个 vruntime 应高度接近（差距 << 单轮增量）
        let spread = if heavy_vr > light_vr { heavy_vr - light_vr } else { light_vr - heavy_vr };
        assert!(spread <= 64, "vruntime 未收敛：spread={}", spread);
    }
}
