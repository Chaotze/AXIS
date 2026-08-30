// ============================================================
// 进程和线程管理（task）根模块
// ============================================================
// 聚合任务子系统的全部模块，并提供全局装配：
//   - 纯逻辑层（可被 unitest 宿主测试）：pcb / thread /
//     process / signal / resource / namespace / cgroup /
//     scheduler（cfs、preemption、cpu_affinity、load_balance）
//   - 装配层（本模块）：全局任务表 + 全局调度器实例、
//     初始化 init()、内核自测 selftest()
//
// 分层约定：
// - 纯逻辑模块不引用 arch 与全局锁，模块间用 super::
//   相对路径
// - 本模块是唯一持有全局状态（Spinlock<Option<TaskState>>）
//   的地方；锁序：TASK 为叶（不向内持有其他全局锁）

pub mod cgroup;
pub mod namespace;
pub mod pcb;
pub mod process;
pub mod resource;
pub mod scheduler;
pub mod signal;
pub mod thread;

use crate::sync::Spinlock;

/// 任务表容量（同时存在的进程/线程上限）
pub const MAX_TASKS: usize = 256;
/// 空闲位图字数（4 × 64 = 256 位，恰好覆盖 MAX_TASKS）
pub const TASK_BITMAP_WORDS: usize = 4;
/// CFS 就绪队列节点池容量（≥ 2×MAX_TASKS + 8）
pub const RUNQ_NODES: usize = 2 * MAX_TASKS + 8;

/// 装配态：任务表 + 调度器
struct TaskState {
    /// 进程表（pid = 槽位）
    table: process::ProcessTable<MAX_TASKS, TASK_BITMAP_WORDS>,
    /// CFS 调度器（单核形态）
    sched: scheduler::Scheduler<MAX_TASKS, RUNQ_NODES>,
    /// 模拟 tick 计数（自测/统计用；真实 tick 由
    /// timer 中断经 tick_hook 注入）
    simulated_ticks: u64,
}

/// 全局任务状态
///
/// 为什么用 Spinlock<Option<Box<TaskState>>> 而非
/// Spinlock<Option<TaskState>>：
/// - TaskState 约 150KB（256 槽进程表 + CFS 节点池），
///   直接作为值会在 init 的栈上构造并大块拷贝，极易
///   压爆引导栈；Box 让它在堆上落位，锁内只存指针
/// - 初始化前为 None，init() 一次性置入；此后所有访问
///   经锁保护，避免 static mut 的裸数据竞争
static TASK: Spinlock<Option<alloc::boxed::Box<TaskState>>> = Spinlock::new(None);

/// 任务子系统初始化
///
/// 顺序：
/// 1. 创建 idle(0) 与 init(1) 进程（进程树的根）
/// 2. 把 init 的主线程加入就绪队列（第一个可调度任务）
pub fn init() {
    let mut guard = TASK.lock();
    // Box 分配：TaskState 体量巨大（~150KB），在堆上构造
    // 避免占用引导栈；随后整体移动进全局状态，无大块栈拷贝
    let mut state = alloc::boxed::Box::new(TaskState {
        table: process::ProcessTable::new(),
        sched: scheduler::Scheduler::new(),
        simulated_ticks: 0,
    });

    // idle：不参与调度，仅占位（后续 idle 循环使用）
    state.table.spawn(process::INVALID_PID, 0).expect("idle 创建失败");
    // init：所有孤儿进程的归宿，进程树的根
    state.table.spawn(process::INVALID_PID, 0).expect("init 创建失败");

    // init 主线程进入调度视野
    state
        .sched
        .admit(process::INIT_PID, 0)
        .expect("init 入队失败");

    *guard = Some(state);
    drop(guard);

    println!("[TASK] task subsystem ready ({} slots)", MAX_TASKS);
    selftest();
}

/// 定时器 tick 钩子（timer 中断经此进入调度视野）
///
/// 本轮只做记账与抢占判定，不执行真实切换（见模块头
/// 说明）；返回"下一个应运行的任务"供上层观察。
/// 接线步骤：interrupt/timer.rs 的 handle_tick 在
/// time::tick() 之后调用本函数。
pub fn tick_hook() -> Option<thread::Tid> {
    let mut guard = TASK.lock();
    let state = guard.as_mut()?;
    state.simulated_ticks += 1;

    // 记账：当前任务 vruntime 前进 1 tick（按权重折算）
    let current = state.sched.current;
    if current != thread::INVALID_TID {
        if let Some(pcb) = state.table.get_mut(current) {
            let weight = scheduler::nice_to_weight(pcb.main_thread.nice);
            pcb.main_thread.vruntime += scheduler::calc_vruntime_delta(1, weight);
        }
    }

    // 抢占判定：当前任务落后队首超过粒度则请求切换
    if let Some(min_vr) = state.sched.runqueue.min_vruntime() {
        if current != thread::INVALID_TID {
            let cur_vr = state
                .table
                .get(current)
                .map(|p| p.main_thread.vruntime)
                .unwrap_or(0);
            if scheduler::should_preempt(cur_vr, min_vr) {
                // TODO(arch): 真实上下文切换接线点（TrapFrame + switch.asm）
            }
        }
    }

    state.sched.pick_next()
}

/// 汇总统计（监控接口）
pub fn stats() -> TaskStats {
    let guard = TASK.lock();
    match guard.as_ref() {
        None => TaskStats::default(),
        Some(state) => TaskStats {
            total_tasks: state.table.len(),
            runqueue_len: state.sched.runqueue.len(),
            current: state.sched.current,
            simulated_ticks: state.simulated_ticks,
        },
    }
}

/// 任务子系统监控统计
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskStats {
    /// 存活任务总数
    pub total_tasks: usize,
    /// 就绪队列长度
    pub runqueue_len: usize,
    /// 当前任务
    pub current: thread::Tid,
    /// 累计 tick
    pub simulated_ticks: u64,
}

// ---------------------------------------------------------------------
// 内核启动自测（验收标准的内核内验证）
// ---------------------------------------------------------------------

/// 运行全部任务子系统自测；返回是否全部通过
pub fn selftest() -> bool {
    println!("\n[TASK-SELFTEST] Task Subsystem Selftest");
    let mut all = true;
    all &= t("process tree & fork", selftest_process_tree());
    all &= t("exit / wait / orphan reparent", selftest_lifecycle());
    all &= t("signal send/block", selftest_signals());
    all &= t("rlimit enforcement", selftest_rlimit());
    all &= t("CFS fairness (3 tasks x 2000 ticks)", selftest_cfs_fairness());
    all &= t("cpu affinity selection", selftest_affinity());
    all &= t("multi-core load balance", selftest_load_balance());
    all &= t("namespace unshare", selftest_namespace());
    all &= t("cgroup v2 limits", selftest_cgroup());
    println!("[TASK-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}

/// 单测断言容器
fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}

/// 自测用宏
macro_rules! check {
    ($cond:expr $(, $msg:expr)?) => {
        if !($cond) {
            $(println!("    [check] FAILED at: {}", $msg);)?
            return false;
        }
    };
}

/// 1) 进程树与 fork
///
/// 为什么自测用 64 槽小表而非全局 MAX_TASKS：
/// - 表体量与槽位数成正比（256 槽 ≈ 100KB），局部构造
///   会挤占引导栈；64 槽足以覆盖树/生命周期全部路径
fn selftest_process_tree() -> bool {
    let mut table: process::ProcessTable<64, 1> = process::ProcessTable::new();
    table.spawn(process::INVALID_PID, 0).unwrap(); // idle
    table.spawn(process::INVALID_PID, 0).unwrap(); // init

    let a = table.spawn(process::INIT_PID, 0).unwrap();
    let _b = table.spawn(process::INIT_PID, 0).unwrap();
    let c = table.fork(a).unwrap();

    check!(table.len() == 5);
    check!(table.get(c).unwrap().parent == a, "fork parent");
    check!(table.get(a).unwrap().first_child == c, "child link");
    check!(table.get(c).unwrap().main_thread.nice == table.get(a).unwrap().main_thread.nice, "nice inherited");
    true
}

/// 2) 生命周期：exit / wait / 孤儿回收
fn selftest_lifecycle() -> bool {
    let mut table: process::ProcessTable<64, 1> = process::ProcessTable::new();
    table.spawn(process::INVALID_PID, 0).unwrap();
    table.spawn(process::INVALID_PID, 0).unwrap();

    let mid = table.spawn(process::INIT_PID, 0).unwrap();
    let orphan = table.spawn(mid, 0).unwrap();
    let leaf = table.spawn(mid, 0).unwrap();

    // mid 退出：孤儿重挂 init，init 收到 SIGCHLD
    table.exit(mid, 3).unwrap();
    check!(table.get(orphan).unwrap().parent == process::INIT_PID, "orphan reparent");
    check!(table.get(leaf).unwrap().parent == process::INIT_PID, "orphan reparent(2)");
    check!(table.get(process::INIT_PID).unwrap().signals.pending.contains(signal::sig::SIGCHLD), "SIGCHLD notify");

    // wait 回收 mid（唯一僵尸）；orphan/leaf 仍存活，不应被回收
    check!(table.wait(process::INIT_PID).unwrap() == Some((mid, 3)), "wait reaps mid");
    check!(table.wait(process::INIT_PID).unwrap().is_none(), "no zombie left");

    // orphan/leaf 退出后（init 是其新父），依次可回收
    table.exit(orphan, 1).unwrap();
    table.exit(leaf, 2).unwrap();
    check!(table.wait(process::INIT_PID).unwrap().is_some(), "wait reaps orphan");
    check!(table.wait(process::INIT_PID).unwrap().is_some(), "wait reaps leaf");
    check!(table.wait(process::INIT_PID).unwrap().is_none(), "all reaped");
    true
}

/// 3) 信号
fn selftest_signals() -> bool {
    let mut st = signal::SignalState::default();
    st.send(signal::sig::SIGTERM); // 15
    st.send(signal::sig::SIGKILL); // 9
    // 按信号号升序投递：SIGKILL(9) 先于 SIGTERM(15)
    check!(st.next_deliverable() == Some(signal::sig::SIGKILL), "lowest signal first");
    check!(st.next_deliverable() == Some(signal::sig::SIGTERM), "then higher signal");
    check!(st.next_deliverable().is_none());

    // SIGKILL 不可阻塞
    check!(!st.block(signal::sig::SIGKILL), "SIGKILL unblockable");
    true
}

/// 4) rlimit
fn selftest_rlimit() -> bool {
    let mut limits = resource::RLimits::default();
    check!(limits.set(resource::rlimit_type::NOFILE, resource::RLimit::fixed(64), false).is_ok());
    check!(limits.check(resource::rlimit_type::NOFILE, 64).is_ok(), "within limit");
    check!(limits.check(resource::rlimit_type::NOFILE, 65).is_err(), "over limit");
    // 软限制不能超过硬限制
    let bad = resource::RLimit { cur: 100, max: 50 };
    check!(limits.set(resource::rlimit_type::DATA, bad, true).is_err(), "soft<=hard");
    true
}

/// 5) CFS 公平性：3 个任务（nice 5/0/-5 → 权重约 1:3:9）2000 tick
fn selftest_cfs_fairness() -> bool {
    let mut rq: scheduler::CfsRunqueue<8, 24> = scheduler::CfsRunqueue::new();
    // 任务 1/2/3 权重比 ≈ 335 : 1024 : 3125（nice 5/0/-5）
    let weights = [
        scheduler::nice_to_weight(5),
        scheduler::nice_to_weight(0),
        scheduler::nice_to_weight(-5),
    ];
    let mut vrs = [0u64; 3];
    let mut runs = [0u64; 3];

    let mut keys = [None; 3];
    for (i, k) in keys.iter_mut().enumerate() {
        *k = Some(rq.enqueue(i as u32 + 1, 0).unwrap());
    }

    for _ in 0..2000 {
        let (key, tid) = rq.pick_next().unwrap();
        let idx = tid as usize - 1;
        // 每个任务每次运行 12 tick，vruntime 按权重缩放
        let delta = scheduler::calc_vruntime_delta(12, weights[idx]);
        vrs[idx] += delta;
        runs[idx] += 12;
        let _ = rq.requeue(key, tid, vrs[idx]);
    }

    // 公平性判据：各任务 vruntime 应几乎一致（差距 << 单轮最大增量）
    let max_vr = *vrs.iter().max().unwrap();
    let min_vr = *vrs.iter().min().unwrap();
    check!(max_vr - min_vr <= 64, "vruntime converged");

    // 运行时间比校验：
    // 模拟中 vr 步长 = 12*1024/w 被整数截断（如 3125 权重下
    // 3.93→3），因此"时间比 == 连续权重比"仅在极限成立；
    // 严谨做法是按实际有效步长（1024/delta）反推期望——
    // 这校验的是"时间 ∝ 1/步长"这一 CFS 核心性质本身
    let total: u64 = runs.iter().sum();
    let deltas = [
        scheduler::calc_vruntime_delta(12, weights[0]),
        scheduler::calc_vruntime_delta(12, weights[1]),
        scheduler::calc_vruntime_delta(12, weights[2]),
    ];
    let w_eff: [u64; 3] = [
        scheduler::NICE_0_LOAD / deltas[0],
        scheduler::NICE_0_LOAD / deltas[1],
        scheduler::NICE_0_LOAD / deltas[2],
    ];
    let wsum_eff: u64 = w_eff.iter().sum();
    for i in 0..3 {
        let ideal = total * w_eff[i] / wsum_eff;
        let diff = (runs[i] as i64 - ideal as i64).abs();
        check!(diff < total as i64 / 50, "time share proportional");
    }
    true
}

/// 6) CPU 亲和性
fn selftest_affinity() -> bool {
    let mut mask: scheduler::CpuMask<1> = scheduler::CpuMask::none();
    mask.set(2);
    mask.set(5);
    check!(mask.select_cpu(Some(3)) == Some(5), "prefer forward");
    check!(mask.select_cpu(Some(6)) == Some(2), "wraparound");
    check!(mask.select_cpu(None) == Some(2), "no prefer lowest");
    true
}

/// 7) 多核负载均衡
fn selftest_load_balance() -> bool {
    let mut lb: scheduler::LoadBalancer<4> = scheduler::LoadBalancer::new();
    lb.set_active_cpus(4);
    lb.set_load(0, 40);
    lb.set_load(1, 4);
    lb.set_load(2, 8);
    lb.set_load(3, 12);

    for _ in 0..100 {
        if lb.balance_once().is_none() {
            break;
        }
    }
    let loads = [lb.load(0), lb.load(1), lb.load(2), lb.load(3)];
    let max = *loads.iter().max().unwrap();
    let min = *loads.iter().min().unwrap();
    check!(max - min <= 2, "load converged");
    check!(lb.total_load() == 64, "load conserved");
    true
}

/// 8) 命名空间
fn selftest_namespace() -> bool {
    let mut reg = namespace::NamespaceRegistry::default();
    let mut view = namespace::NamespaceView::default();
    for ty in namespace::NamespaceType::ALL {
        view.set(ty, reg.create(ty));
    }

    let mut set = namespace::NamespaceSet::empty();
    set.add(namespace::NamespaceType::Pid);
    let changed = reg.unshare(&mut view, set);
    check!(changed.contains(namespace::NamespaceType::Pid), "pid ns changed");
    check!(!changed.contains(namespace::NamespaceType::Net), "net ns unchanged");
    check!(view.get(namespace::NamespaceType::Pid) == 1, "new ns id");
    true
}

/// 9) cgroup v2
fn selftest_cgroup() -> bool {
    let mut table: cgroup::CgroupTable<8> = cgroup::CgroupTable::new();
    table.init();
    check!(table.set_cpu_max(0, 50_000, 100_000).is_ok());
    check!(table.set_memory_max(0, 1 << 20).is_ok());

    let child = table.create_child(0).unwrap();
    check!(table.get(child).unwrap().cpu_max == Some((50_000, 100_000)), "limits inherited");

    let root = table.get_mut(0).unwrap();
    root.cpu_usage_us = 50_001;
    check!(root.cpu_exceeded(), "cpu exceeded");
    root.memory_current = (1 << 20) + 1;
    check!(root.memory_exceeded(), "mem exceeded");
    true
}
