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
    /// 累计上下文切换次数（监控/验证调度在运转）
    context_switches: u64,
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
/// 2. 为 init 与 3 个演示任务创建内核栈与首次运行帧
/// 3. 跑纯逻辑自测（此时尚未开调度，输出不受并发干扰）
/// 4. start_scheduling()：任务入队，返回后主循环成为 idle，
///    首个定时器 tick 开始真实上下文切换
pub fn init() {
    // 全程关中断：初始化阶段持 TASK 锁并做堆分配，若被定时器
    // 中断打断，中断路径的 tick_hook 会等 TASK 锁自旋，而中断
    // 不返回、init 永不继续 → 死锁（首跳约在 sti 后 1ms，
    // 初始化一旦变慢就会撞进这个窗口）
    let flags = crate::arch::x86_64::cpu::irq_save();

    let mut guard = TASK.lock();
    // Box 分配：TaskState 体量巨大（~150KB），在堆上构造
    // 避免占用引导栈；随后整体移动进全局状态，无大块栈拷贝
    let mut state = alloc::boxed::Box::new(TaskState {
        table: process::ProcessTable::new(),
        sched: scheduler::Scheduler::new(),
        simulated_ticks: 0,
        context_switches: 0,
    });

    // idle：不参与调度，仅占位（主循环 hlt 即其运行态）
    state.table.spawn(process::INVALID_PID, 0).expect("idle 创建失败");
    // init：所有孤儿进程的归宿，进程树的根，也是内核线程
    state.table.spawn(process::INVALID_PID, 0).expect("init 创建失败");
    // 为 init 装配内核栈与首次运行帧（入口 init_task）
    setup_kernel_thread(&mut state, process::INIT_PID, 0, init_task as *const () as usize, 0);

    // 三个演示内核线程（nice 5/0/-5，验证 CFS 权重公平）
    spawn_demo_task(&mut state, "demo-1", 1, 5);
    spawn_demo_task(&mut state, "demo-2", 2, 0);
    spawn_demo_task(&mut state, "demo-3", 3, -5);

    *guard = Some(state);
    drop(guard);

    println!("[TASK] task subsystem ready ({} slots)", MAX_TASKS);
    selftest();

    // 初始化完成：恢复中断，首个 tick 开始真实抢占切换
    unsafe {
        crate::arch::x86_64::cpu::irq_restore(flags);
    }
}

/// 开始调度：任务入队并装载 idle 时间片
///
/// 为什么单独成步：
/// - 自测（selftest）在调度开启前同步运行，输出不被并发
///   任务穿插；本函数返回后，首次 tick 即开始抢占切换
pub fn start_scheduling() {
    let mut guard = TASK.lock();
    let Some(state) = guard.as_mut() else { return };

    // init 与演示任务入队（vruntime 0，按 tid 平局裁决）
    let mut pids: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    for i in 0..MAX_TASKS as u32 {
        if let Some(pcb) = state.table.get(i) {
            if pcb.is_alive() && i != process::IDLE_PID {
                pids.push(i);
            }
        }
    }
    for pid in pids {
        let _ = state.sched.admit(pid, 0);
    }

    // 当前上下文（主循环 hlt）即 idle：设 current 并给时间片，
    // 首个 tick 会把它当作旧任务切出（其帧被保存，iretq 可
    // 在未来"切回 idle"时恢复——当前演示任务恒可运行，idle
    // 仅在启动初期运行）
    state.sched.current = process::IDLE_PID;
    if let Some(idle) = state.table.get_mut(process::IDLE_PID) {
        idle.main_thread.ticks_left = 24;
    }
}

/// 定时器 tick 钩子：真实上下文切换（由中断存根调用）
///
/// # 参数
/// - `saved_rsp`: 当前中断现场在栈上的保存帧地址
///   （"RSP 指向 r15 槽"，见 interrupt/entry.asm）
///
/// # 返回
/// - 0：不切换，存根在原栈恢复
/// - 非 0：目标任务的保存帧 RSP，存根换栈后 iretq 落入新任务
///
/// 为什么在中断上下文中持 TASK 锁是安全的：
/// - 中断门进入时 IF=0，本 CPU 不会重入；就绪队列的堆分配
///   （BTreeMap 节点）走 PMM/KHEAP 锁，与 TASK 无环
pub fn tick_hook(saved_rsp: usize) -> usize {
    let mut guard = TASK.lock();
    let Some(state) = guard.as_mut() else { return 0 };
    state.simulated_ticks += 1;

    let current = state.sched.current;
    if current == thread::INVALID_TID {
        // 防御：调度未初始化时的 tick 直接忽略
        return 0;
    }

    // ---- 1. 当前任务记账：vruntime 前进（1024 定标）+ 时间片扣减
    let (cur_vr, need_switch) = {
        let pcb = state.table.get_mut(current).expect("当前任务缺失");
        let weight = scheduler::nice_to_weight(pcb.main_thread.nice);
        // 1024 定标：1 tick 的真实时间放大 1024 倍记入 vr，
        // 保证高权重（nice<0，权重>1024）任务的每 tick 增量
        // 也不为 0（否则其 vr 永停 → 永远最小 → 饿死其余任务）
        pcb.main_thread.vruntime += scheduler::calc_vruntime_delta(1024, weight);
        let expired = scheduler::tick_charge(
            &mut pcb.main_thread.ticks_left,
            &mut pcb.main_thread.need_resched,
        );
        (pcb.main_thread.vruntime, expired)
    };

    // ---- 2. 抢占判定：时间片耗尽 或 vruntime 落后队首超过粒度
    let min_vr = state.sched.runqueue.min_vruntime().unwrap_or(cur_vr);
    if !need_switch && !scheduler::should_preempt(cur_vr, min_vr) {
        return 0;
    }

    // ---- 3. 保存旧任务现场（idle 不入队：它只在队列空时被唤醒）
    if current != process::IDLE_PID {
        let pcb = state.table.get_mut(current).expect("旧任务缺失");
        pcb.main_thread.trap_frame = saved_rsp;
        let vr = pcb.main_thread.vruntime;
        let _ = state.sched.runqueue.enqueue(current, vr);
    } else {
        let idle = state.table.get_mut(process::IDLE_PID).expect("idle 缺失");
        idle.main_thread.trap_frame = saved_rsp;
    }

    // ---- 4. 选出 vruntime 最小的任务并出队
    let Some((key, next_tid)) = state.sched.runqueue.pick_next() else {
        // 队列空（演示阶段不会发生）：继续 idle
        state.sched.current = process::IDLE_PID;
        return 0;
    };
    state.sched.runqueue.dequeue(key);
    state.sched.current = next_tid;
    state.context_switches += 1;

    // ---- 5. 装载新任务时间片并返回其保存帧 RSP
    let next_pcb = state.table.get_mut(next_tid).expect("新任务缺失");
    let weight = scheduler::nice_to_weight(next_pcb.main_thread.nice);
    let nr = state.sched.runqueue.len() + 1;
    let slice = scheduler::sched_slice_ticks(nr, weight, weight);
    scheduler::charge_slice(
        &mut next_pcb.main_thread.ticks_left,
        &mut next_pcb.main_thread.need_resched,
        slice,
    );
    next_pcb.main_thread.state = thread::ThreadState::Running;
    next_pcb.main_thread.trap_frame
}

/// 汇总统计（监控接口）
///
/// 为什么读锁也要 irq_save：
/// - 任务态（如 init_task）调用时若被定时器抢占，中断路径的
///   tick_hook 会等待同一把 TASK 锁 → 死锁；屏蔽中断使
///   读锁区间不可被抢占（与中断路径的锁获取互斥）
pub fn stats() -> TaskStats {
    let flags = crate::arch::x86_64::cpu::irq_save();
    let result = {
        let guard = TASK.lock();
        match guard.as_ref() {
            None => TaskStats::default(),
            Some(state) => {
                // 计算 idle 任务的运行时间
                // idle 任务从系统启动开始运行，idle_ticks = 当前总 tick - idle 创建 tick
                let idle_ticks = if let Some(idle_pcb) = state.table.get(process::IDLE_PID) {
                    state.simulated_ticks.saturating_sub(idle_pcb.start_tick)
                } else {
                    0
                };

                TaskStats {
                    total_tasks: state.table.len(),
                    runqueue_len: state.sched.runqueue.len(),
                    current: state.sched.current,
                    simulated_ticks: state.simulated_ticks,
                    context_switches: state.context_switches,
                    idle_ticks,
                }
            }
        }
    };
    unsafe {
        crate::arch::x86_64::cpu::irq_restore(flags);
    }
    result
}

/// 获取所有存活进程的 PID 列表
///
/// 用途：procfs readdir() 时需要列出所有进程目录。
/// 为什么提供这个接口而非让 procfs 直接访问任务表：
/// - 保持模块封装性（procfs 不依赖内部结构变化）
/// - 集中权限管理（irq_save/锁保护在这里）
/// - 便于未来支持过滤（如按状态、按权限等）
pub fn list_all_pids() -> alloc::vec::Vec<u32> {
    let flags = crate::arch::x86_64::cpu::irq_save();
    let result = {
        let guard = TASK.lock();
        let mut pids = alloc::vec::Vec::new();
        if let Some(state) = guard.as_ref() {
            for pid in 0..MAX_TASKS as u32 {
                if state.table.exists(pid) {
                    if let Some(pcb) = state.table.get(pid) {
                        // 只列出活进程（包括 idle）
                        if pcb.is_alive() {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
        pids
    };
    unsafe {
        crate::arch::x86_64::cpu::irq_restore(flags);
    }
    result
}

/// 获取指定进程的信息
///
/// 用途：procfs stat/status 等文件需要获取进程详细信息。
/// 为什么返回 Option：进程可能不存在或已退出
pub fn get_process_info(pid: u32) -> Option<ProcessInfo> {
    let flags = crate::arch::x86_64::cpu::irq_save();
    let result = {
        let guard = TASK.lock();
        guard.as_ref().and_then(|state| {
            state.table.get(pid).and_then(|pcb| {
                if pcb.is_alive() {
                    // 生成进程名称
                    let mut name = [0u8; 16];
                    let name_str = alloc::format!("task{}", pid);
                    let name_bytes = name_str.as_bytes();
                    let copy_len = core::cmp::min(name_bytes.len(), 15);
                    name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

                    Some(ProcessInfo {
                        pid,
                        ppid: pcb.parent,
                        state: pcb.state,
                        nice: pcb.main_thread.nice,
                        vruntime: pcb.main_thread.vruntime,
                        name,
                        name_len: copy_len,
                        start_tick: pcb.start_tick,
                    })
                } else {
                    None
                }
            })
        })
    };
    unsafe {
        crate::arch::x86_64::cpu::irq_restore(flags);
    }
    result
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
    /// 累计上下文切换次数
    pub context_switches: u64,
    /// idle(PID 0) 任务运行的 tick 数（空闲时间）
    pub idle_ticks: u64,
}

/// 进程信息结构（用于 procfs 等子系统读取）
///
/// 为什么设计这个结构：
/// - procfs 需要显示进程的详细信息，但无法直接访问全局任务表
/// - 通过公开接口而非暴露内部 PCB，保持模块封装性
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// 进程号
    pub pid: u32,
    /// 父进程号
    pub ppid: u32,
    /// 进程状态
    pub state: pcb::ProcessState,
    /// nice 值（优先级调整）
    pub nice: i8,
    /// 虚拟运行时间（CFS 权重计算用）
    pub vruntime: u64,
    /// 进程名称（演示系统为 task{pid}）
    pub name: [u8; 16],
    pub name_len: usize,
    /// 创建时刻（系统启动以来的 tick）
    pub start_tick: u64,
}

// ---------------------------------------------------------------------
// 内核线程装配（真实上下文切换的构造侧）
// ---------------------------------------------------------------------

/// 内核线程栈大小
/// 64KB：入口调用链 + 中断保存帧 + tick_hook 的堆分配
/// 调用链（BTreeMap 节点分配经 GlobalAlloc → SLUB）较深，
/// 16KB 在实测中出现栈耗尽型崩溃（缺页 at 0x0，帧 RSP
/// 恒定落在栈顶附近）；64KB 留足余量
const KERNEL_THREAD_STACK: usize = 64 * 1024;

/// 为指定 pid 装配内核栈与首次运行帧
///
/// 为什么栈用 kmalloc 而非静态数组：
/// - 任务数动态（256 上限），静态预留浪费；堆分配 4 页/任务，
///   随任务退出可归还（当前演示任务常驻，不归还）
fn setup_kernel_thread(
    state: &mut TaskState,
    pid: u32,
    nice: i8,
    entry: usize,
    arg0: u64,
) {
    // 栈：kmalloc 对齐 16（SwitchFrame 需要 16 字节对齐栈顶）
    let raw = crate::mm::heap::kmalloc(KERNEL_THREAD_STACK, 16);
    assert!(!raw.is_null(), "内核线程栈分配失败");
    let stack_top = raw as usize + KERNEL_THREAD_STACK;
    // 首次运行帧：iretq 后从 entry 开始，rdi = arg0
    let frame_rsp =
        crate::arch::x86_64::context::frame::SwitchFrame::init_stack(entry, stack_top, arg0);

    let pcb = state.table.get_mut(pid).expect("任务不存在");
    pcb.main_thread.nice = nice;
    pcb.main_thread.kernel_stack = raw as usize;
    pcb.main_thread.trap_frame = frame_rsp;
    pcb.main_thread.state = thread::ThreadState::Ready;
}

/// 创建一个演示内核线程（挂在 init 之下）
fn spawn_demo_task(state: &mut TaskState, _name: &str, arg: u64, nice: i8) {
    let pid = state
        .table
        .spawn(process::INIT_PID, 0)
        .expect("演示任务创建失败");
    setup_kernel_thread(state, pid, nice, demo_task as *const () as usize, arg);
}

/// init 内核线程入口：周期打印系统状态（调度在运转的佐证）
extern "C" fn init_task(_arg: u64) -> ! {
    // 【诊断】先纯空转（与 demo 任务同策略）
    let mut round: u64 = 0;
    loop {
        round += 1;
        if round % 1_000_000_000 == 0 {
            let s = stats();
            println!(
                "[INIT] ticks={} switches={} runq={}",
                s.simulated_ticks, s.context_switches, s.runqueue_len
            );
        }
    }
}

/// 演示内核线程入口：以参数区分身份，循环打印进度
///
/// 三个实例 nice 不同（5/0/-5），输出频率天然不同——
/// 屏幕上的交错输出即"多任务并发切换"的直接证据；
/// 计数速度差异则是 CFS 权重公平的直观体现。
extern "C" fn demo_task(arg: u64) -> ! {
    let id = arg;
    // 【诊断】先纯空转：分离"切换机制"与"打印路径"
    // （若空转稳定则切换正常，问题在任务内打印）
    let mut round: u64 = 0;
    loop {
        round += 1;
        if round % 1_000_000_000 == 0 {
            println!("  [TASK {}] round=1G", id);
        }
    }
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
