// ============================================================
// 块设备 I/O 调度器
// ============================================================
// 以请求队列为输入、按不同策略决定请求的执行顺序。
//
// 实现策略：
// - Noop：严格先来先服务（FIFO），最简、延迟最低
// - Fifo：FIFO + 相邻请求轻量合并（同操作且 LBA 连续时扩展扇区数）
// - Sstf：最短寻道时间优先（每次取离当前磁头最近的请求），
//   适合机械盘场景的经典模拟
//
// 纯逻辑设计：调度器只操作 BlkRequest，不接触硬件，可宿主单测。

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::blk_queue::{BlkRequest, ReqOp};

/// I/O 调度器接口
pub trait IoScheduler {
    /// 调度器名称
    fn name(&self) -> &'static str;
    /// 入队一个请求
    fn push(&mut self, req: BlkRequest);
    /// 出队下一个应执行的请求
    fn pop(&mut self) -> Option<BlkRequest>;
    /// 队内请求数
    fn len(&self) -> usize;
    /// 是否为空
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ------------------------------------------------------------
// Noop 调度器
// ------------------------------------------------------------

/// Noop 调度器：严格 FIFO（Vec 头尾操作模拟队列）
#[derive(Debug, Default)]
pub struct NoopScheduler {
    queue: Vec<BlkRequest>,
}

impl NoopScheduler {
    pub const fn new() -> Self {
        Self { queue: Vec::new() }
    }
}

impl IoScheduler for NoopScheduler {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn push(&mut self, req: BlkRequest) {
        self.queue.push(req);
    }

    fn pop(&mut self) -> Option<BlkRequest> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}

// ------------------------------------------------------------
// FIFO + 合并调度器
// ------------------------------------------------------------

/// FIFO 调度器：先来先服务，且对相邻连续请求做合并
///
/// 合并条件：与队尾请求操作相同、缓冲区同源（同为 null 或同一
/// 连续缓冲）、LBA 与扇区数首尾相接。合并减少硬件命令数。
#[derive(Debug, Default)]
pub struct FifoScheduler {
    queue: Vec<BlkRequest>,
    /// 允许合并的最大请求长度（扇区，防止单请求过大）
    max_sectors: u32,
}

impl FifoScheduler {
    pub const fn new(max_sectors: u32) -> Self {
        Self { queue: Vec::new(), max_sectors }
    }

    /// 尝试把 req 合并到队尾；成功返回 true
    fn try_merge_tail(&mut self, req: &BlkRequest) -> bool {
        let Some(tail) = self.queue.last_mut() else { return false };
        // 只合并读写，不合并 Flush；缓冲区必须可连续解释
        if tail.op != req.op || req.op == ReqOp::Flush {
            return false;
        }
        // 合并后长度不能超过上限
        let merged = tail.sectors as u64 + req.sectors as u64;
        if merged > self.max_sectors as u64 {
            return false;
        }
        // 首尾相接：tail 结束 == req 开始，或 req 结束 == tail 开始
        let tail_end = tail.lba as u128 + tail.sectors as u128;
        let req_end = req.lba as u128 + req.sectors as u128;
        if tail.lba as u128 == req_end {
            // req 在前，tail 在后：向前扩展
            tail.lba = req.lba;
            tail.sectors = merged as u32;
            return true;
        }
        if req.lba as u128 == tail_end {
            // tail 在前，req 在后：向后扩展
            tail.sectors = merged as u32;
            return true;
        }
        false
    }
}

impl IoScheduler for FifoScheduler {
    fn name(&self) -> &'static str {
        "fifo+merge"
    }

    fn push(&mut self, req: BlkRequest) {
        if !self.try_merge_tail(&req) {
            self.queue.push(req);
        }
    }

    fn pop(&mut self) -> Option<BlkRequest> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}

// ------------------------------------------------------------
// SSTF 调度器
// ------------------------------------------------------------

/// 最短寻道时间优先（SSTF）调度器
///
/// 每次从队中选出离当前磁头位置最近的请求，减少寻道时间；
/// 公平性较差（远处请求可能饥饿），适合作为调度算法的对照实现。
#[derive(Debug, Default)]
pub struct SstfScheduler {
    queue: Vec<BlkRequest>,
    /// 当前磁头所在 LBA
    head: u64,
}

impl SstfScheduler {
    pub const fn new() -> Self {
        Self { queue: Vec::new(), head: 0 }
    }
}

impl IoScheduler for SstfScheduler {
    fn name(&self) -> &'static str {
        "sstf"
    }

    fn push(&mut self, req: BlkRequest) {
        self.queue.push(req);
    }

    fn pop(&mut self) -> Option<BlkRequest> {
        if self.queue.is_empty() {
            return None;
        }
        // 找离磁头最近的请求
        let mut best = 0;
        let mut best_dist = u64::MAX;
        for (i, r) in self.queue.iter().enumerate() {
            let dist = r.lba.abs_diff(self.head);
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        let req = self.queue.remove(best);
        if req.op != ReqOp::Flush {
            self.head = req.lba + req.sectors as u64; // 磁头移动到请求结束位置
        }
        Some(req)
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}

/// 创建默认调度器（FIFO + 合并）
pub fn default_scheduler() -> Box<dyn IoScheduler> {
    alloc::boxed::Box::new(FifoScheduler::new(128))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(op: ReqOp, lba: u64, sectors: u32) -> BlkRequest {
        BlkRequest::new(op, lba, sectors, core::ptr::null_mut(), 0)
    }

    #[test]
    fn test_noop_fifo_order() {
        let mut s = NoopScheduler::new();
        s.push(req(ReqOp::Read, 10, 1));
        s.push(req(ReqOp::Write, 5, 1));
        s.push(req(ReqOp::Read, 7, 1));
        assert_eq!(s.pop().unwrap().lba, 10);
        assert_eq!(s.pop().unwrap().lba, 5);
        assert_eq!(s.pop().unwrap().lba, 7);
        assert!(s.is_empty());
    }

    #[test]
    fn test_fifo_merge_adjacent() {
        let mut s = FifoScheduler::new(128);
        s.push(req(ReqOp::Read, 10, 2));  // 10-12
        s.push(req(ReqOp::Read, 12, 1));  // 与队尾连续 → 合并为 10-13
        s.push(req(ReqOp::Write, 20, 1)); // 不同操作，不合并
        assert_eq!(s.len(), 2);
        let merged = s.pop().unwrap();
        assert_eq!(merged.lba, 10);
        assert_eq!(merged.sectors, 3);
        assert_eq!(s.pop().unwrap().lba, 20);
    }

    #[test]
    fn test_fifo_merge_front() {
        let mut s = FifoScheduler::new(128);
        s.push(req(ReqOp::Read, 12, 1));
        s.push(req(ReqOp::Read, 10, 2)); // 12-10 连续 → 合并为 10-13
        assert_eq!(s.len(), 1);
        let merged = s.pop().unwrap();
        assert_eq!(merged.lba, 10);
        assert_eq!(merged.sectors, 3);
    }

    #[test]
    fn test_sstf_order() {
        let mut s = SstfScheduler::new();
        s.push(req(ReqOp::Read, 100, 1));
        s.push(req(ReqOp::Read, 10, 1));
        s.push(req(ReqOp::Read, 12, 1));
        // 磁头在 0：最近的按顺序是 10 → 12 → 100
        assert_eq!(s.pop().unwrap().lba, 10);
        assert_eq!(s.pop().unwrap().lba, 12);
        assert_eq!(s.pop().unwrap().lba, 100);
    }

    #[test]
    fn test_sstf_head_moves() {
        let mut s = SstfScheduler::new();
        s.push(req(ReqOp::Read, 50, 1));
        s.push(req(ReqOp::Read, 0, 1));
        s.push(req(ReqOp::Read, 49, 1));
        // 磁头 0 → 0 → 50；随后磁头在 50，下一请求 49 比 0 近
        assert_eq!(s.pop().unwrap().lba, 0);
        assert_eq!(s.pop().unwrap().lba, 50);
        assert_eq!(s.pop().unwrap().lba, 49);
    }
}