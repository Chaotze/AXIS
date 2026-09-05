// ============================================================
// 块设备请求队列
// ============================================================
// 块 I/O 的统一请求描述与排队管理：驱动程序提交请求，完成后
// 由队列返回结果。请求中的缓冲指针由具体驱动解释，队列自己
// 只负责顺序、容量与完成状态。
//
// 为什么需要请求队列：
// - 块设备（NVMe/AHCI/virtio）的硬件队列深度有限，软件层需要
//   排队模型来背压与合并请求
// - 为 I/O 调度器（io_scheduler.rs）提供统一的请求形态
//
// 纯逻辑设计：请求结构与队列不接触硬件，可宿主单测。

use crate::prelude::{KernelError, KernelResult};

/// 请求操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqOp {
    Read,
    Write,
    /// 冲刷（确保此前写入落盘）
    Flush,
}

/// 请求完成状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqStatus {
    Pending,
    Ok,
    Error,
}

/// 块 I/O 请求
#[derive(Debug, Clone, Copy)]
pub struct BlkRequest {
    /// 操作类型
    pub op: ReqOp,
    /// 起始逻辑块号
    pub lba: u64,
    /// 扇区数
    pub sectors: u32,
    /// 数据缓冲（虚拟地址；调度器测试场景可为 null）
    pub buf: *mut u8,
    /// 提交序号（全局单调递增，用于公平调度）
    pub seq: u64,
    /// 完成状态
    pub status: ReqStatus,
    /// 队列内槽位索引（-1 = 尚未入队）
    pub slot: i32,
}

// 手动实现 Send：`buf` 的所有权语义由提交方与设备驱动约定
unsafe impl Send for BlkRequest {}

impl BlkRequest {
    /// 构造请求
    pub const fn new(op: ReqOp, lba: u64, sectors: u32, buf: *mut u8, seq: u64) -> Self {
        Self {
            op,
            lba,
            sectors,
            buf,
            seq,
            status: ReqStatus::Pending,
            slot: -1,
        }
    }

    /// 以正确结果完成请求
    pub fn complete_ok(&mut self) {
        self.status = ReqStatus::Ok;
    }

    /// 以错误结果完成请求
    pub fn complete_err(&mut self) {
        self.status = ReqStatus::Error;
    }
}

/// 块设备请求队列
///
/// 维护队列容量与进行中请求数；调度策略由外部 IoScheduler 决定，
/// 本队列只提供容量控制与完成计数。
#[derive(Debug, Clone)]
pub struct BlkQueue {
    /// 队列容量（同时在队请求上限）
    capacity: usize,
    /// 进行中（尚未完成的）请求数
    outstanding: usize,
    /// 下一请求序号
    next_seq: u64,
}

impl BlkQueue {
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            outstanding: 0,
            next_seq: 0,
        }
    }

    /// 提交一个请求（返回填入的序号）
    pub fn submit(&mut self, req: &mut BlkRequest) -> KernelResult<u64> {
        if self.outstanding >= self.capacity {
            return Err(KernelError::Unsupported); // 队列已满，应由调度器背压
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        req.seq = seq;
        req.status = ReqStatus::Pending;
        req.slot = -1;
        self.outstanding += 1;
        Ok(seq)
    }

    /// 完成一个请求
    pub fn complete(&mut self, req: &mut BlkRequest) {
        if req.status == ReqStatus::Pending {
            // 未显式设置的请求默认视为成功（由设备驱动设置）
            req.complete_ok();
        }
        if self.outstanding > 0 {
            self.outstanding -= 1;
        }
        req.slot = -1;
    }

    /// 进行中请求数
    pub const fn outstanding(&self) -> usize {
        self.outstanding
    }

    /// 队列是否已满
    pub const fn is_full(&self) -> bool {
        self.outstanding >= self.capacity
    }

    /// 队列是否空闲
    pub const fn is_idle(&self) -> bool {
        self.outstanding == 0
    }

    /// 队列容量
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// 下一请求序号（统计用）
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

/// 默认队列深度（对应常见硬件的 256-1024 深度，取中间值）
pub const DEFAULT_QUEUE_DEPTH: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_and_complete() {
        let mut q = BlkQueue::new(2);
        let mut r1 = BlkRequest::new(ReqOp::Read, 0, 1, core::ptr::null_mut(), 0);
        let mut r2 = BlkRequest::new(ReqOp::Write, 4, 2, core::ptr::null_mut(), 0);

        assert!(q.submit(&mut r1).is_ok());
        assert!(q.submit(&mut r2).is_ok());
        assert_eq!(q.outstanding(), 2);
        assert!(q.is_full());

        // 超容量拒绝
        let mut r3 = BlkRequest::new(ReqOp::Flush, 0, 0, core::ptr::null_mut(), 0);
        assert!(q.submit(&mut r3).is_err());

        r1.complete_ok();
        q.complete(&mut r1);
        assert_eq!(q.outstanding(), 1);
        assert!(!q.is_full());
        assert_eq!(r1.status, ReqStatus::Ok);
    }

    #[test]
    fn test_seq_monotonic() {
        let mut q = BlkQueue::new(8);
        let mut r = BlkRequest::new(ReqOp::Read, 0, 1, core::ptr::null_mut(), 0);
        let s1 = q.submit(&mut r).unwrap();
        let s2 = q.submit(&mut r).unwrap();
        assert_eq!(s1, 0);
        assert_eq!(s2, 1);
    }
}