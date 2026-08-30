// ============================================================
// 信号处理机制
// ============================================================
// 信号的表示、发送、阻塞与默认动作（纯逻辑层）。
//
// 为什么需要信号：
// - 进程间异步通知的基础设施（SIGKILL 终止、SIGTERM 请求
//   退出、SIGCHLD 子进程状态变化……），也是验收标准
//   "信号发送和处理正常"的落点
//
// 为什么设计为纯逻辑模块：
// - 位集运算、默认动作查询、发送/阻塞语义不依赖硬件，
//   可在宿主（unitest）完整验证；真正"投递到用户态"
//   （设置用户栈帧、sigreturn）属于架构胶水，待系统调用
//   层落地后接线（见 pcb.rs 中的处理钩子占位）
//
// 信号号的语义（沿用 Linux 标准编号）：
// - 为什么照抄 Linux 编号：dev.md 要求采用 Linux 通用
//   系统调用 ABI 与语义，信号号是 ABI 的一部分，自定义
//   编号会在将来兼容层引入无谓的翻译成本

/// 标准信号号（Linux ABI）
pub mod sig {
    pub const SIGHUP: u32 = 1;
    pub const SIGINT: u32 = 2;
    pub const SIGQUIT: u32 = 3;
    pub const SIGILL: u32 = 4;
    pub const SIGTRAP: u32 = 5;
    pub const SIGABRT: u32 = 6;
    pub const SIGBUS: u32 = 7;
    pub const SIGFPE: u32 = 8;
    pub const SIGKILL: u32 = 9;
    pub const SIGUSR1: u32 = 10;
    pub const SIGSEGV: u32 = 11;
    pub const SIGUSR2: u32 = 12;
    pub const SIGPIPE: u32 = 13;
    pub const SIGALRM: u32 = 14;
    pub const SIGTERM: u32 = 15;
    pub const SIGCHLD: u32 = 17;
    pub const SIGCONT: u32 = 18;
    pub const SIGSTOP: u32 = 19;
    pub const SIGTSTP: u32 = 20;
}

/// 信号集（位集，1 bit 对应 1 个信号号）
///
/// 为什么用 64 位掩码：
/// - 标准信号仅 1-31（实时信号 34-64 为扩展预留），
///   单字掩码让"检查 + 置位"可用一条位运算完成，
///   后续与原子操作结合时天然支持无锁发送
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SignalSet(pub u64);

impl SignalSet {
    /// 空集
    pub const fn empty() -> Self {
        Self(0)
    }

    /// 包含单个信号的集合
    pub const fn of(signal: u32) -> Self {
        Self(1u64 << signal)
    }

    /// 是否包含某信号
    pub const fn contains(&self, signal: u32) -> bool {
        self.0 & (1u64 << signal) != 0
    }

    /// 并入一个信号
    pub fn add(&mut self, signal: u32) {
        self.0 |= 1u64 << signal;
    }

    /// 移除一个信号
    pub fn remove(&mut self, signal: u32) {
        self.0 &= !(1u64 << signal);
    }

    /// 并集
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// 交集
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// 集合是否为空
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 取出并清除编号最小的信号（如 SIGCHLD 合并语义：
    /// 同号多次发送只保留一次，取信号时按编号优先级）
    pub fn take_lowest(&mut self) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        let lowest = self.0.trailing_zeros() as u32;
        self.0 &= self.0 - 1;
        Some(lowest)
    }
}

/// 信号的默认处置方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    /// 忽略（SIGCHLD 等）
    Ignore,
    /// 终止进程（SIGTERM 等）
    Terminate,
    /// 终止并产生 core dump（SIGSEGV 等）
    CoreDump,
    /// 停止进程（SIGSTOP 等）
    Stop,
    /// 继续进程（SIGCONT）
    Continue,
}

/// 查询信号的默认处置
///
/// 为什么集中成表：
/// - 每个信号的默认语义是标准约定，集中一处避免
///   exit/kill 等调用方各自维护一份
pub const fn default_action(signal: u32) -> DefaultAction {
    match signal {
        sig::SIGCHLD | sig::SIGCONT => DefaultAction::Ignore,
        sig::SIGILL | sig::SIGABRT | sig::SIGBUS | sig::SIGFPE | sig::SIGSEGV => {
            DefaultAction::CoreDump
        }
        sig::SIGSTOP | sig::SIGTSTP => DefaultAction::Stop,
        _ => DefaultAction::Terminate,
    }
}

/// 进程的信号状态（挂起集 + 阻塞掩码）
#[derive(Debug, Clone, Copy, Default)]
pub struct SignalState {
    /// 已发送但尚未投递的信号
    pub pending: SignalSet,
    /// 被进程阻塞的信号（SIGKILL/SIGSTOP 除外）
    pub blocked: SignalSet,
}

impl SignalState {
    /// 发送信号：未阻塞则挂起，否则丢弃（标准语义）
    ///
    /// # 为什么 SIGKILL/SIGSTOP 不可阻塞：
    /// - 这是 Linux 的强制语义：保证系统始终能终止/挂起
    ///   失控进程，否则恶意进程可阻塞一切信号逃避管理
    pub fn send(&mut self, signal: u32) {
        if signal != sig::SIGKILL && signal != sig::SIGSTOP && self.blocked.contains(signal) {
            return; // 被阻塞：丢弃（位集无队列，标准信号语义）
        }
        self.pending.add(signal);
    }

    /// 阻塞一个信号（返回是否生效）
    pub fn block(&mut self, signal: u32) -> bool {
        if signal == sig::SIGKILL || signal == sig::SIGSTOP {
            return false; // 不可阻塞
        }
        self.blocked.add(signal);
        true
    }

    /// 解除阻塞；被解除的信号若此前有挂起，一并保留待投递
    pub fn unblock(&mut self, signal: u32) {
        self.blocked.remove(signal);
    }

    /// 取下一个可投递的信号（挂起且未阻塞）
    ///
    /// 为什么投递前再次检查阻塞：
    /// - 信号可能在挂起后被阻塞；投递点（返回用户态前）
    ///   只交付"当下可交付"的信号，保持语义准确
    /// - 按信号号升序取：小号信号优先（与 Linux 一致，
    ///   避免低优先级信号被饿死）
    pub fn next_deliverable(&mut self) -> Option<u32> {
        let mut rest = self.pending.0;
        while rest != 0 {
            let sig = rest.trailing_zeros() as u32;
            // SIGKILL/SIGSTOP 不可阻塞：即便被强行写入阻塞
            // 掩码也视为可投递（与 send 的强制语义一致）
            if sig == sig::SIGKILL || sig == sig::SIGSTOP || !self.blocked.contains(sig) {
                // 投递即清除挂起位（信号无队列，一次交付）
                self.pending.remove(sig);
                return Some(sig);
            }
            rest &= rest - 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_signal_set_ops() {
        let mut set = SignalSet::empty();
        set.add(sig::SIGTERM);
        set.add(sig::SIGCHLD);
        assert!(set.contains(sig::SIGTERM));
        assert!(set.contains(sig::SIGCHLD));
        assert!(!set.contains(sig::SIGINT));

        set.remove(sig::SIGTERM);
        assert!(!set.contains(sig::SIGTERM));

        // take_lowest 按编号升序取出
        let mut s = SignalSet::of(sig::SIGCHLD).union(SignalSet::of(sig::SIGTERM));
        assert_eq!(s.take_lowest(), Some(sig::SIGTERM)); // 15 < 17
        assert_eq!(s.take_lowest(), Some(sig::SIGCHLD));
        assert_eq!(s.take_lowest(), None);
    }

    #[test]
    fn test_default_actions() {
        assert_eq!(default_action(sig::SIGCHLD), DefaultAction::Ignore);
        assert_eq!(default_action(sig::SIGTERM), DefaultAction::Terminate);
        assert_eq!(default_action(sig::SIGSEGV), DefaultAction::CoreDump);
        assert_eq!(default_action(sig::SIGSTOP), DefaultAction::Stop);
        assert_eq!(default_action(sig::SIGCONT), DefaultAction::Ignore);
    }

    #[test]
    fn test_send_pending_and_block() {
        let mut st = SignalState::default();
        st.send(sig::SIGTERM);
        st.send(sig::SIGTERM); // 同号合并（标准信号无队列）
        assert!(st.pending.contains(sig::SIGTERM));

        // 阻塞后发送被丢弃
        assert!(st.block(sig::SIGTERM));
        st.send(sig::SIGTERM);
        assert_eq!(st.next_deliverable(), None);

        // 解除阻塞后可投递
        st.unblock(sig::SIGTERM);
        assert_eq!(st.next_deliverable(), Some(sig::SIGTERM));
        assert_eq!(st.next_deliverable(), None);
    }

    #[test]
    fn test_kill_unblockable() {
        let mut st = SignalState::default();
        // SIGKILL/SIGSTOP 不允许阻塞
        assert!(!st.block(sig::SIGKILL));
        assert!(!st.block(sig::SIGSTOP));
        // 即使"被阻塞"，发送仍生效
        st.blocked.add(sig::SIGKILL);
        st.send(sig::SIGKILL);
        assert!(st.pending.contains(sig::SIGKILL));
        assert_eq!(st.next_deliverable(), Some(sig::SIGKILL));
    }
}
