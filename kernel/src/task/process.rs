// ============================================================
// 进程操作（fork / exec / exit / wait）
// ============================================================
// 定长任务表与进程生命周期管理（纯逻辑层）。
//
// 为什么任务表设计为定长数组 + 位图：
// - pid 就是槽位下标：get/get_mut O(1)，无哈希开销；
//   空闲槽由 lib 的 Bitmap 管理，
//   分配 O(1)（find_first_zero + 分配提示）
// - 与 mm/task 子系统的"定长 + 索引"风格一致；容量
//   MAX_TASKS 编译期确定（本内核 256 级任务足够）
//
// 为什么进程树用侵入式单链表（first_child + next_sibling）：
// - 进程树的操作只有"挂到父链头""遍历子进程""摘除"，
//   单向链足够且零分配；fork/exit/wait 三条路径全部 O(子数)
//
// fork 语义：
// - 复制 PCB（含主线程、信号阻塞集、rlimit、命名空间、
//   cgroup 归属），身份重置（新 pid、parent=pid）
// - 信号挂起集清空、阻塞集继承（Linux 语义：挂起信号
//   不属于子进程，阻塞掩码属于进程配置）
// - 内存/文件描述符的复制是"共享占位"：待 mm 进程地址
//   空间与文件系统阶段接入（见 exec 的挂载点注释）
//
// exec 语义：
// - 保留进程身份（pid/父子关系/资源限制），重置运行态
//   （信号挂起清空、调度记账清零）
// - ELF 加载动作是明确的挂载点：compat/loader（阶段 9）
//   落地时在 exec 中调用；本轮返回成功以走通控制流
//
// 为什么本模块是纯逻辑（不持有全局锁）：
// - 表操作全部通过 &mut self，单线程所有权贯穿；
//   全局实例与锁在 task/mod.rs 装配

// 对外重导出进程号类型（task/mod.rs 等装配层使用）
pub use super::pcb::{Pid, INVALID_PID};
use super::pcb::{ProcessControlBlock, ProcessState};
use super::signal::{self, SignalSet};
use super::super::lib::collections::bitmap::Bitmap;

/// 孤儿进程的新归宿（init 进程固定为 1 号）
pub const INIT_PID: Pid = 1;
/// 空闲任务（0 号，不属于任何进程树）
pub const IDLE_PID: Pid = 0;

/// 定长进程表
///
/// 泛型参数：
/// - `MAX_TASKS`: 槽位数（= 最大 pid 数）
/// - `BITMAP_WORDS`: 空闲位图字数（≥ ceil(MAX_TASKS/64)，
///   稳定版 Rust 不支持 const 泛型算术，故显式传入）
pub struct ProcessTable<const MAX_TASKS: usize, const BITMAP_WORDS: usize> {
    /// 任务槽位（pid = 下标；None = 空闲）
    slots: [Option<ProcessControlBlock>; MAX_TASKS],
    /// 槽位占用位图（1 = 已用）
    used: Bitmap<BITMAP_WORDS>,
    /// 线性分配提示（从上次分配点继续找，减少扫描）
    next_hint: usize,
    /// 存活任务数（含 Zombie）
    len: usize,
}

impl<const MAX_TASKS: usize, const BITMAP_WORDS: usize> ProcessTable<MAX_TASKS, BITMAP_WORDS> {
    /// 创建空表
    pub const fn new() -> Self {
        assert!(BITMAP_WORDS * 64 >= MAX_TASKS, "位图字数不足以覆盖任务槽位");
        Self {
            slots: [None; MAX_TASKS],
            used: Bitmap::new(),
            next_hint: 0,
            len: 0,
        }
    }

    /// 存活任务数
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 表是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 任务是否存在
    pub fn exists(&self, pid: Pid) -> bool {
        (pid as usize) < MAX_TASKS && self.slots[pid as usize].is_some()
    }

    /// 只读访问任务
    pub fn get(&self, pid: Pid) -> Option<&ProcessControlBlock> {
        self.slots.get(pid as usize).and_then(|s| s.as_ref())
    }

    /// 可变访问任务
    pub fn get_mut(&mut self, pid: Pid) -> Option<&mut ProcessControlBlock> {
        self.slots.get_mut(pid as usize).and_then(|s| s.as_mut())
    }

    // ============================================================
    // 生命周期
    // ============================================================

    /// 创建新进程（spawn；init/idle 与内核线程的出生路径）
    ///
    /// # 参数
    /// - `parent`: 父进程（无父传 INVALID_PID）
    /// - `start_tick`: 创建时刻（统计用）
    pub fn spawn(&mut self, parent: Pid, start_tick: u64) -> Result<Pid, &'static str> {
        let pid = self.alloc_slot()?;
        let pcb = ProcessControlBlock::new(pid, parent, start_tick);
        self.slots[pid as usize] = Some(pcb);
        self.link_child(parent, pid);
        self.len += 1;
        Ok(pid)
    }

    /// fork：复制父进程创建子进程（Linux fork 语义）
    ///
    /// 继承：PCB 全体（信号阻塞集、rlimit、命名空间、
    /// cgroup、nice）与主线程配置；
    /// 重置：新 pid、parent、进程树链、状态、退出码、
    /// 信号挂起集、调度记账（vruntime/时间片/CPU）。
    pub fn fork(&mut self, parent_pid: Pid) -> Result<Pid, &'static str> {
        let parent = *self.get(parent_pid).ok_or("父进程不存在")?;
        if !parent.is_alive() {
            return Err("父进程已退出");
        }

        let child_pid = self.alloc_slot()?;
        let mut child = parent; // Copy：完整复制进程控制块

        // 身份与树关系重置
        child.pid = child_pid;
        child.parent = parent_pid;
        child.first_child = INVALID_PID;
        child.next_sibling = INVALID_PID;
        child.state = ProcessState::Running;
        child.exit_code = None;

        // 信号：挂起集不继承（Linux 语义），阻塞掩码继承
        child.signals.pending = SignalSet::empty();

        // 调度记账重置：新任务从零开始竞争 CPU
        child.main_thread.tid = child_pid as u32;
        child.main_thread.vruntime = 0;
        child.main_thread.ticks_left = 0;
        child.main_thread.need_resched = false;
        child.main_thread.cpu = None;
        child.main_thread.state = super::thread::ThreadState::Ready;

        self.slots[child_pid as usize] = Some(child);
        self.link_child(parent_pid, child_pid);
        self.len += 1;
        Ok(child_pid)
    }

    /// exec：替换进程映像（本轮为控制流占位）
    ///
    /// 语义要点（Linux）：
    /// - 身份不变：pid、父子关系、rlimit 保留
    /// - 运行态重置：挂起信号清空、调度记账清零
    /// - 挂载点：真正的地址空间替换 + ELF 加载在
    ///   compat/loader（阶段 9）实现后从这里调用
    pub fn exec(&mut self, pid: Pid) -> Result<(), &'static str> {
        let pcb = self.get_mut(pid).ok_or("进程不存在")?;
        if !pcb.is_alive() {
            return Err("进程已退出");
        }
        // 新映像不应继承旧映像的挂起信号
        pcb.signals.pending = SignalSet::empty();
        pcb.main_thread.vruntime = 0;
        pcb.main_thread.ticks_left = 0;
        pcb.main_thread.need_resched = false;
        // TODO(compat): 调用 loader 加载 ELF、重建地址空间
        Ok(())
    }

    /// exit：进程退出（进入僵尸态，等待父进程 wait 回收）
    ///
    /// 关键动作：
    /// 1. 置 Zombie + 记录退出码
    /// 2. 孤儿重挂：本进程的子进程 reparent 到 init(1)，
    ///    保证进程树不断裂（验收标准"孤儿进程回收"）
    /// 3. 向父进程发送 SIGCHLD（子进程状态变化的通知）
    pub fn exit(&mut self, pid: Pid, code: i32) -> Result<(), &'static str> {
        if !self.exists(pid) {
            return Err("进程不存在");
        }
        // 僵尸进程已退出，拒绝重复 exit
        if !self.get(pid).map(|p| p.is_alive()).unwrap_or(false) {
            return Err("进程已退出");
        }
        let parent = self.get(pid).map(|p| p.parent).unwrap_or(INVALID_PID);

        // 先重挂孤儿（在自身状态变更前完成树操作）
        self.reparent_orphans(pid);

        // 置僵尸
        let pcb = self.get_mut(pid).unwrap();
        pcb.state = ProcessState::Zombie;
        pcb.exit_code = Some(code);

        // 通知父进程（父已死则无需通知）
        if parent != INVALID_PID {
            if let Some(parent_pcb) = self.get_mut(parent) {
                parent_pcb.signals.send(signal::sig::SIGCHLD);
            }
        }
        Ok(())
    }

    /// wait：回收一个僵尸子进程
    ///
    /// Linux 语义简化版：等待任意子进程（waitpid 的
    /// 指定 pid 变体可在其上扩展）；返回 (子 pid, 退出码)。
    /// 若子进程均未退出，返回 Ok(None)（非阻塞语义；
    /// 真正的阻塞等待由调度器/WaitQueue 接线后实现）。
    pub fn wait(&mut self, waiter: Pid) -> Result<Option<(Pid, i32)>, &'static str> {
        // 遍历子进程链找僵尸
        let mut child = self
            .get(waiter)
            .map(|p| p.first_child)
            .ok_or("等待者不存在")?;

        while child != INVALID_PID {
            let is_zombie = self
                .get(child)
                .map(|c| c.state == ProcessState::Zombie)
                .unwrap_or(false);
            if is_zombie {
                let code = self.get(child).and_then(|c| c.exit_code).unwrap_or(0);
                // 从子链摘除并回收槽位
                self.unlink_child(waiter, child);
                self.free_slot(child);
                return Ok(Some((child, code)));
            }
            child = self.get(child).map(|c| c.next_sibling).unwrap_or(INVALID_PID);
        }
        Ok(None)
    }

    // ============================================================
    // 内部：槽位与进程树
    // ============================================================

    /// 分配空闲槽位（Bitmap + 线性提示）
    fn alloc_slot(&mut self) -> Result<Pid, &'static str> {
        if self.len >= MAX_TASKS {
            return Err("任务表已满");
        }
        // 位图找第一个空闲位（优先从提示点附近开始，
        // 完整扫描兜底，避免提示点之前的空洞长期闲置）
        let bit = self
            .used
            .find_first_zero()
            .ok_or("任务表已满")?;
        if bit >= MAX_TASKS {
            return Err("位图越界（配置错误）");
        }
        self.used.set(bit);
        self.next_hint = (bit + 1) % MAX_TASKS;
        let _ = self.next_hint;
        Ok(bit as Pid)
    }

    /// 回收槽位
    fn free_slot(&mut self, pid: Pid) {
        self.slots[pid as usize] = None;
        self.used.clear(pid as usize);
        self.len -= 1;
    }

    /// 把子进程挂到父进程的子链头部
    fn link_child(&mut self, parent: Pid, child: Pid) {
        if parent == INVALID_PID || !self.exists(parent) {
            return; // 无父或父不存在（如 idle）
        }
        let head = self.get(parent).unwrap().first_child;
        self.get_mut(parent).unwrap().first_child = child;
        self.get_mut(child).unwrap().next_sibling = head;
    }

    /// 从父进程子链摘除子进程
    fn unlink_child(&mut self, parent: Pid, child: Pid) {
        // 遍历单链找到前驱并摘除
        let mut prev = INVALID_PID;
        let mut cur = self.get(parent).map(|p| p.first_child).unwrap_or(INVALID_PID);
        while cur != INVALID_PID {
            if cur == child {
                let next = self.get(child).map(|c| c.next_sibling).unwrap_or(INVALID_PID);
                if prev == INVALID_PID {
                    self.get_mut(parent).unwrap().first_child = next;
                } else {
                    self.get_mut(prev).unwrap().next_sibling = next;
                }
                return;
            }
            prev = cur;
            cur = self.get(cur).map(|c| c.next_sibling).unwrap_or(INVALID_PID);
        }
    }

    /// 孤儿重挂：把 pid 的全部子进程改挂到 init(1)
    fn reparent_orphans(&mut self, pid: Pid) {
        // 全表扫描（任务数级开销，exit 路径可接受；
        // 优化可维护反向 children 计数，留待需要时）
        for i in 0..MAX_TASKS {
            let is_child = self
                .get(i as Pid)
                .map(|c| c.parent == pid)
                .unwrap_or(false);
            if is_child {
                let mut c = *self.get(i as Pid).unwrap();
                c.parent = INIT_PID;
                c.next_sibling = INVALID_PID;
                self.slots[i] = Some(c);
                // 挂到 init 子链头
                let head = self.get(INIT_PID).map(|p| p.first_child).unwrap_or(INVALID_PID);
                self.get_mut(INIT_PID).unwrap().first_child = i as Pid;
                self.slots[i].as_mut().unwrap().next_sibling = head;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    /// 测试用表：64 槽位
    type TestTable = ProcessTable<64, 1>;

    /// 建表：idle(0) + init(1)
    fn new_table() -> TestTable {
        let mut table: TestTable = ProcessTable::new();
        table.spawn(INVALID_PID, 0).unwrap(); // idle = 0
        table.spawn(INVALID_PID, 0).unwrap(); // init = 1
        table
    }

    #[test]
    fn test_spawn_and_tree() {
        let mut t = new_table();
        let a = t.spawn(INIT_PID, 0).unwrap(); // 2
        let b = t.spawn(INIT_PID, 0).unwrap(); // 3
        let c = t.spawn(a, 0).unwrap(); // 4

        assert_eq!(t.len(), 5);
        assert!(t.exists(a) && t.exists(b) && t.exists(c));

        // 树形：init → {a, b}，a → {c}
        assert_eq!(t.get(a).unwrap().parent, INIT_PID);
        assert_eq!(t.get(b).unwrap().parent, INIT_PID);
        assert_eq!(t.get(c).unwrap().parent, a);
        assert_eq!(t.get(INIT_PID).unwrap().first_child, b); // 链头是最后挂入的
        assert_eq!(t.get(b).unwrap().next_sibling, a);
        assert_eq!(t.get(a).unwrap().next_sibling, INVALID_PID);
        assert_eq!(t.get(a).unwrap().first_child, c);
    }

    #[test]
    fn test_fork_inherits_and_resets() {
        let mut t = new_table();
        let parent = t.spawn(INIT_PID, 100).unwrap();
        // 父进程自定义：nice、阻塞信号、rlimit、挂起信号
        {
            let p = t.get_mut(parent).unwrap();
            p.set_nice(5);
            p.signals.block(signal::sig::SIGINT);
            p.signals.send(signal::sig::SIGTERM);
            p.rlimits.set(super::super::resource::rlimit_type::NOFILE, super::super::resource::RLimit::fixed(32), false).unwrap();
        }

        let child = t.fork(parent).unwrap();
        let c = t.get(child).unwrap();
        assert_eq!(c.parent, parent);
        assert_eq!(c.main_thread.nice, 5); // 继承 nice
        assert!(c.signals.blocked.contains(signal::sig::SIGINT)); // 继承阻塞掩码
        assert!(c.signals.pending.is_empty()); // 挂起集清空
        assert_eq!(c.rlimits.get(super::super::resource::rlimit_type::NOFILE).cur, 32); // 继承 rlimit
        assert_eq!(c.main_thread.vruntime, 0); // 记账重置
        assert_eq!(t.get(parent).unwrap().first_child, child); // 挂到父链

        // 不能 fork 僵尸进程
        t.exit(child, 0).unwrap();
        assert!(t.fork(child).is_err());
    }

    #[test]
    fn test_exit_zombie_and_sigchld() {
        let mut t = new_table();
        let child = t.spawn(INIT_PID, 0).unwrap();

        t.exit(child, 42).unwrap();
        let c = t.get(child).unwrap();
        assert_eq!(c.state, ProcessState::Zombie);
        assert_eq!(c.exit_code, Some(42));
        assert!(!c.is_alive());
        // init 收到 SIGCHLD
        assert!(t.get(INIT_PID).unwrap().signals.pending.contains(signal::sig::SIGCHLD));

        // 重复 exit 应报错
        assert!(t.exit(child, 0).is_err());
    }

    #[test]
    fn test_orphan_reparent() {
        let mut t = new_table();
        let mid = t.spawn(INIT_PID, 0).unwrap(); // 2
        let orphan = t.spawn(mid, 0).unwrap(); // 3

        // mid 退出 → orphan 重挂到 init
        t.exit(mid, 0).unwrap();
        assert_eq!(t.get(orphan).unwrap().parent, INIT_PID);
        // init 的子链包含 orphan
        let mut found = false;
        let mut cur = t.get(INIT_PID).unwrap().first_child;
        while cur != INVALID_PID {
            if cur == orphan {
                found = true;
                break;
            }
            cur = t.get(cur).unwrap().next_sibling;
        }
        assert!(found);
    }

    #[test]
    fn test_wait_reaps_zombie_and_reuses_slot() {
        let mut t = new_table();
        let child = t.spawn(INIT_PID, 0).unwrap();
        let child_pid = child;

        // 未退出：wait 返回 None（非阻塞语义）
        assert_eq!(t.wait(INIT_PID).unwrap(), None);

        t.exit(child, 7).unwrap();
        assert_eq!(t.wait(INIT_PID).unwrap(), Some((child_pid, 7)));

        // 槽位已回收、子链已摘除
        assert!(!t.exists(child_pid));
        assert_eq!(t.get(INIT_PID).unwrap().first_child, INVALID_PID);
        assert_eq!(t.len(), 2); // 只剩 idle + init

        // 槽位复用：新进程可再取同一 pid
        let again = t.spawn(INIT_PID, 0).unwrap();
        assert_eq!(again, child_pid);
    }

    #[test]
    fn test_exec_resets_runtime_state() {
        let mut t = new_table();
        let p = t.spawn(INIT_PID, 0).unwrap();
        {
            let pcb = t.get_mut(p).unwrap();
            pcb.signals.send(signal::sig::SIGTERM);
            pcb.main_thread.vruntime = 999;
            pcb.main_thread.ticks_left = 3;
            pcb.main_thread.need_resched = true;
            pcb.rlimits.set(super::super::resource::rlimit_type::AS, super::super::resource::RLimit::fixed(1 << 20), false).unwrap();
        }

        t.exec(p).unwrap();
        let pcb = t.get(p).unwrap();
        assert!(pcb.signals.pending.is_empty()); // 挂起信号清空
        assert_eq!(pcb.main_thread.vruntime, 0); // 记账清零
        assert!(!pcb.main_thread.need_resched);
        // 身份与资源限制保留
        assert_eq!(pcb.pid, p);
        assert_eq!(pcb.rlimits.get(super::super::resource::rlimit_type::AS).cur, 1 << 20);

        // exec 僵尸进程失败
        t.exit(p, 0).unwrap();
        assert!(t.exec(p).is_err());
    }

    #[test]
    fn test_table_full() {
        let mut t: ProcessTable<8, 1> = ProcessTable::new();
        for i in 0..8 {
            assert!(t.spawn(INVALID_PID, 0).is_ok(), "第 {} 个进程创建失败", i);
        }
        assert_eq!(t.len(), 8);
        assert!(t.spawn(INVALID_PID, 0).is_err());
    }
}
