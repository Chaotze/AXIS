// ============================================================
// 线程（Thread）
// ============================================================
// 调度实体的数据结构：身份、运行状态与 CFS 记账字段。
//
// 为什么线程与进程分离：
// - 线程是"被调度"的单位（有自己的 vruntime、时间片、
//   亲和性），进程是"拥有资源"的单位（PCB）；
//   1:1 模型下每进程一个主线程，1:N 时 PCB 可挂多个，Thread
//
// 为什么设计为纯数据结构模块：
// - 状态字段与转换逻辑不依赖硬件；内核栈/TrapFrame 的
//   真实指针由上下文切换接线时填充，此处以地址占位并
//   注明用途，使本模块可被 unitest 编译验证
//
// 为什么 Copy 派生：
// - 全部字段为纯数据；fork 复制线程描述符时按位复制，
//   再重置运行时字段（vruntime、cpu 等），简单可靠

use super::scheduler::cpu_affinity::CpuMask;

/// 线程号
pub type Tid = u32;

/// 无效线程号
pub const INVALID_TID: Tid = u32::MAX;

/// 线程运行状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    /// 就绪（在就绪队列中等待调度）
    Ready,
    /// 正在某个 CPU 上运行
    Running,
    /// 阻塞（等待事件；WaitQueue 接线后启用）
    Blocked,
    /// 已退出（等待回收）
    Exited,
}

/// 线程描述符（调度实体）
#[derive(Debug, Clone, Copy)]
pub struct Thread {
    /// 线程号
    pub tid: Tid,
    /// 所属进程
    pub pid: super::pcb::Pid,
    /// 运行状态
    pub state: ThreadState,
    /// 当前所在 CPU（未运行时为 None）
    pub cpu: Option<u32>,

    // ---- CFS 记账（见 scheduler/cfs.rs 的计算规则）----
    /// 虚拟运行时间：公平性的唯一标尺，越小越优先
    pub vruntime: u64,
    /// 静态优先级（nice，-20..=19）
    pub nice: i8,
    /// 剩余时间片（tick 数；耗尽置 need_resched）
    pub ticks_left: u64,
    /// 需要重新调度标志（由 preemption 判定置位）
    pub need_resched: bool,

    /// CPU 亲和掩码（1 字 = 64 核，足够当前阶段）
    pub affinity: CpuMask<1>,

    // ---- 硬件上下文占位（上下文切换接线时填充）----
    /// 内核栈指针（TrapFrame 所在栈顶）
    ///
    /// 为什么占位而非直接定义：TrapFrame 布局与
    /// arch/x86_64/context 绑定，线程模块保持架构无关，
    /// 接线时由 arch 层写入真实指针
    pub kernel_stack: usize,
    /// 陷阱帧地址（与 kernel_stack 同源，见 entry.asm 布局）
    pub trap_frame: usize,
}

impl Thread {
    /// 创建线程
    ///
    /// 默认态：Ready、vruntime 为 0、nice 为 0（由 CFS
    /// 的权重表映射到 NICE_0_LOAD）、亲和全掩码。
    pub const fn new(tid: Tid, pid: super::pcb::Pid) -> Self {
        Self {
            tid,
            pid,
            state: ThreadState::Ready,
            cpu: None,
            vruntime: 0,
            nice: 0,
            ticks_left: 0,
            need_resched: false,
            affinity: CpuMask::none(),
            kernel_stack: 0,
            trap_frame: 0,
        }
    }

    /// 是否处于可被调度状态
    pub const fn is_runnable(&self) -> bool {
        matches!(self.state, ThreadState::Ready)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_new_thread_defaults() {
        let t = Thread::new(7, 3);
        assert_eq!(t.tid, 7);
        assert_eq!(t.pid, 3);
        assert_eq!(t.state, ThreadState::Ready);
        assert!(t.is_runnable());
        assert_eq!(t.vruntime, 0);
        assert_eq!(t.nice, 0);
        assert_eq!(t.cpu, None);
        assert!(!t.need_resched);
        assert_eq!(t.kernel_stack, 0);
        assert_eq!(t.trap_frame, 0);
    }

    #[test]
    fn test_state_transitions() {
        let mut t = Thread::new(1, 1);
        t.state = ThreadState::Running;
        assert!(!t.is_runnable());
        t.state = ThreadState::Blocked;
        assert!(!t.is_runnable());
        t.state = ThreadState::Ready;
        assert!(t.is_runnable());
    }
}
