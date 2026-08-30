// ============================================================
// PCB（进程控制块）
// ============================================================
// 进程的内核态元数据结构：身份、状态机、进程树、资源集合。
//
// 为什么 PCB 按"资源容器"设计（与 Thread 分离）：
// - 沿用 Linux 模型：进程（PCB）持有资源与生命周期，
//   线程（Thread）是调度实体；1:1 与 1:N 模型都可用
//   同一套结构表达，为多线程留好扩展位
// - 进程树（父子/兄弟关系）、信号、rlimit、命名空间、
//   cgroup 都是"进程级"概念，归属 PCB 而非线程
//
// 为什么设计为纯数据结构模块：
// - PCB 只做"字段 + 状态转换 + 进程树关系维护"，不碰
//   全局任务表与锁；表管理在 process.rs（ProcessTable），
//   全局装配在 task/mod.rs——分层与 mm 子系统一致，
//   使本模块可被 unitest 直接编译验证
//
// 为什么用 Copy 派生：
// - PCB 全部字段为纯数据（无 Drop、无指针），Copy 使
//   fork 的"复制进程控制块"退化为按位复制，再逐字段
//   修正身份信息，简洁且不易漏字段

use super::cgroup::{CgroupId, INVALID_CGROUP};
use super::namespace::NamespaceView;
use super::resource::RLimits;
use super::signal::SignalState;
use super::thread::Thread;

/// 进程号
pub type Pid = u32;

/// 无效进程号（空槽位/无父进程等语义）
pub const INVALID_PID: Pid = u32::MAX;

/// 进程状态机
///
/// 状态迁移（本轮支持的核心路径）：
/// ```text
///              fork                 exit
///  (none) ──────────▶ Running ──────────▶ Zombie ─── wait 回收 ──▶ (槽位释放)
///                        ▲                  │
///                        └──── 被阻塞 ───────┘（后续 WaitQueue 接线）
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// 就绪/运行中
    Running,
    /// 阻塞（等待事件；WaitQueue 与调度器接线后启用）
    Blocked,
    /// 僵尸：已退出、等待父进程 wait 回收
    Zombie,
}

impl ProcessState {
    /// 是否存活（非僵尸）
    pub const fn is_alive(self) -> bool {
        !matches!(self, ProcessState::Zombie)
    }
}

/// 进程控制块
#[derive(Debug, Clone, Copy)]
pub struct ProcessControlBlock {
    /// 进程号（= 任务表槽位，见 process.rs）
    pub pid: Pid,
    /// 父进程号（init 进程为 INVALID_PID）
    pub parent: Pid,
    /// 状态
    pub state: ProcessState,
    /// 退出码（仅 Zombie 态有效）
    pub exit_code: Option<i32>,

    // ---- 进程树（侵入式单链表：first_child + next_sibling）----
    /// 第一个子进程
    pub first_child: Pid,
    /// 下一个兄弟进程（父进程相同）
    pub next_sibling: Pid,

    /// 主线程（调度实体；1:1 模型本轮只此一个）
    pub main_thread: Thread,

    // ---- 资源与归属 ----
    /// 信号状态（挂起/阻塞）
    pub signals: SignalState,
    /// 资源限制
    pub rlimits: RLimits,
    /// 命名空间视图
    pub namespaces: NamespaceView,
    /// 所属 cgroup
    pub cgroup: CgroupId,

    // ---- 统计 ----
    /// 创建时刻的系统 tick
    pub start_tick: u64,
}

impl ProcessControlBlock {
    /// 创建新进程控制块
    ///
    /// 为什么 fork 的"复制"不放在这里：
    /// - 新建（spawn，如 init/idle）与复制（fork）的字段
    ///   语义不同：fork 需要继承父进程的资源集合，
    ///   由 process.rs 显式逐字段继承，保证两个路径
    ///   各自清晰可审计
    pub fn new(pid: Pid, parent: Pid, start_tick: u64) -> Self {
        Self {
            pid,
            parent,
            state: ProcessState::Running,
            exit_code: None,
            first_child: INVALID_PID,
            next_sibling: INVALID_PID,
            main_thread: Thread::new(pid as super::thread::Tid, pid),
            signals: SignalState::default(),
            rlimits: RLimits::default(),
            namespaces: NamespaceView::default(),
            cgroup: INVALID_CGROUP,
            start_tick,
        }
    }

    /// 是否存活
    pub const fn is_alive(&self) -> bool {
        self.state.is_alive()
    }

    /// 是否有子进程
    pub const fn has_children(&self) -> bool {
        self.first_child != INVALID_PID
    }

    /// 设置 nice（主线程与附加线程统一在此设置）
    ///
    /// 为什么 nice 是进程级属性：
    /// - POSIX 语义中 nice 属于进程；线程级权重由 cgroup
    ///   cpu.weight 表达，两者正交
    pub fn set_nice(&mut self, nice: i8) {
        self.main_thread.nice = nice;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_new_pcb_defaults() {
        let pcb = ProcessControlBlock::new(42, INVALID_PID, 100);
        assert_eq!(pcb.pid, 42);
        assert_eq!(pcb.parent, INVALID_PID);
        assert!(pcb.is_alive());
        assert_eq!(pcb.state, ProcessState::Running);
        assert!(!pcb.has_children());
        assert_eq!(pcb.exit_code, None);
        // 主线程身份与进程一致（1:1 模型）
        assert_eq!(pcb.main_thread.pid, 42);
        // 资源集合默认态
        assert_eq!(pcb.rlimits.get(super::super::resource::rlimit_type::CPU).cur, super::super::resource::RLIM_INFINITY);
        assert_eq!(pcb.cgroup, INVALID_CGROUP);
    }

    #[test]
    fn test_state_machine() {
        let mut pcb = ProcessControlBlock::new(1, INVALID_PID, 0);
        pcb.state = ProcessState::Blocked;
        assert!(pcb.is_alive());
        pcb.state = ProcessState::Zombie;
        assert!(!pcb.is_alive());
    }

    #[test]
    fn test_nice_propagates_to_main_thread() {
        let mut pcb = ProcessControlBlock::new(1, INVALID_PID, 0);
        pcb.set_nice(5);
        assert_eq!(pcb.main_thread.nice, 5);
    }
}
