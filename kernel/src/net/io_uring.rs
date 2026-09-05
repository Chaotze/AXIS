// ============================================================
// io_uring - 高性能异步 I/O 接口
// ============================================================
// 实现 Linux io_uring 风格的异步 I/O 接口
//
// io_uring 特点：
// - 基于共享的环形缓冲区
// - 无系统调用开销（SQ、CQ 用户态轮询）
// - 支持批量操作
// - 支持网络 I/O、文件 I/O 等

use crate::lib::result::KernelResult;
use crate::sync::Spinlock;
use alloc::collections::VecDeque;

// ============================================================
// io_uring 操作码定义
// ============================================================

/// io_uring 操作码
pub mod op {
    pub const NOP: u8 = 0;
    pub const READ: u8 = 1;
    pub const WRITE: u8 = 2;
    pub const SEND: u8 = 29;
    pub const RECV: u8 = 30;
}

// ============================================================
// io_uring 提交队列
// ============================================================

/// io_uring 提交队列条目
#[derive(Debug, Clone)]
pub struct SqEntry {
    /// 操作码
    pub opcode: u8,
    /// 标志
    pub flags: u8,
    /// 文件描述符
    pub fd: i32,
    /// 偏移或其他参数
    pub offset: u64,
    /// 地址
    pub addr: u64,
    /// 长度
    pub len: u32,
    /// 用户数据（用于识别请求）
    pub user_data: u64,
}

impl SqEntry {
    /// 创建新的 SQ 条目
    pub fn new(opcode: u8, fd: i32) -> Self {
        SqEntry {
            opcode,
            flags: 0,
            fd,
            offset: 0,
            addr: 0,
            len: 0,
            user_data: 0,
        }
    }
}

// ============================================================
// io_uring 完成队列
// ============================================================

/// io_uring 完成队列条目
#[derive(Debug, Clone)]
pub struct CqEntry {
    /// 用户数据（对应 SQ 条目的 user_data）
    pub user_data: u64,
    /// 返回值（字节数或错误码）
    pub res: i32,
    /// 标志
    pub flags: u32,
}

impl CqEntry {
    /// 创建新的 CQ 条目
    pub fn new(user_data: u64, res: i32) -> Self {
        CqEntry {
            user_data,
            res,
            flags: 0,
        }
    }
}

// ============================================================
// io_uring 内部状态
// ============================================================

/// io_uring 内部状态
struct IoUringState {
    /// SQ 缓冲区（提交队列）
    sq_buffer: VecDeque<SqEntry>,
    /// CQ 缓冲区（完成队列）
    cq_buffer: VecDeque<CqEntry>,
    /// SQ 索引
    sq_tail: u32,
    /// CQ 索引
    cq_head: u32,
}

impl IoUringState {
    fn new(entries: usize) -> Self {
        IoUringState {
            sq_buffer: VecDeque::with_capacity(entries),
            cq_buffer: VecDeque::with_capacity(entries * 2),
            sq_tail: 0,
            cq_head: 0,
        }
    }
}

// ============================================================
// io_uring 实例
// ============================================================

/// io_uring 实例
pub struct IoUring {
    /// 内部状态
    state: Spinlock<IoUringState>,
    /// 最大条目数
    entries: usize,
}

impl IoUring {
    /// 创建新的 io_uring 实例
    pub fn new(entries: usize) -> KernelResult<Self> {
        // 为什么需要 entries 检查：
        // - 必须是 2 的幂
        // - 至少 1，最多 4096
        if entries == 0 || entries > 4096 || (entries & (entries - 1)) != 0 {
            return Err(crate::prelude::KernelError::InvalidArgument);
        }

        Ok(IoUring {
            state: Spinlock::new(IoUringState::new(entries)),
            entries,
        })
    }

    /// 提交操作
    /// 为什么需要提交：
    /// - 将操作加入 SQ 缓冲区
    /// - 后续由处理器从队列中取出并执行
    pub fn submit(&self, entry: SqEntry) -> KernelResult<()> {
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();

        // 检查队列是否满
        if state.sq_buffer.len() >= self.entries {
            unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }
            return Err(crate::prelude::KernelError::Other("SQ full"));
        }

        state.sq_buffer.push_back(entry);
        state.sq_tail = state.sq_tail.wrapping_add(1);

        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        Ok(())
    }

    /// 获取完成结果（非阻塞）
    /// 为什么需要 peek：
    /// - 检查是否有完成的操作
    /// - 用户态轮询模式下使用
    pub fn peek_cqe(&self) -> Option<CqEntry> {
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();
        let result = state.cq_buffer.pop_front();

        if result.is_some() {
            state.cq_head = state.cq_head.wrapping_add(1);
        }

        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        result
    }

    /// 获取下一个待处理的提交条目
    /// 内部使用：用于处理器读取 SQ 中的条目
    pub fn next_sqe(&self) -> Option<SqEntry> {
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();
        let result = state.sq_buffer.pop_front();

        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        result
    }

    /// 提交完成结果
    /// 内部使用：用于处理器返回操作结果
    pub fn submit_cqe(&self, cqe: CqEntry) -> KernelResult<()> {
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();

        // 检查 CQ 是否满
        if state.cq_buffer.len() >= self.entries * 2 {
            unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }
            return Err(crate::prelude::KernelError::Other("CQ full"));
        }

        state.cq_buffer.push_back(cqe);

        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        Ok(())
    }

    /// 获取队列统计信息
    pub fn stats(&self) -> (usize, usize) {
        let state = self.state.lock();
        (state.sq_buffer.len(), state.cq_buffer.len())
    }
}

// ============================================================
// io_uring 自测
// ============================================================

pub fn selftest() -> bool {
    // 创建 io_uring 实例
    let uring = match IoUring::new(256) {
        Ok(u) => u,
        Err(_) => return false,
    };

    // 提交一个 NOP 操作
    let mut sqe = SqEntry::new(op::NOP, -1);
    sqe.user_data = 42;

    if uring.submit(sqe).is_err() {
        return false;
    }

    // 检查队列状态
    let (sq_len, cq_len) = uring.stats();
    if sq_len != 1 || cq_len != 0 {
        return false;
    }

    true
}

