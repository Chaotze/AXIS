// ============================================================
// 屏障 (Barrier)
// ============================================================
// 同步多个线程到达同一执行点

use super::spinlock::Spinlock;
use super::wait_queue::WaitQueue;
use core::sync::atomic::{AtomicUsize, Ordering};

/// 屏障
///
/// 屏障用于同步多个线程：
/// - 线程调用 wait() 会阻塞
/// - 当所有线程都到达屏障时，所有线程同时被唤醒
///
/// 应用场景：
/// - 并行计算的阶段同步
/// - 确保所有线程完成初始化后再继续
pub struct Barrier {
    count: AtomicUsize,
    total: usize,
    generation: Spinlock<usize>,
    wait_queue: WaitQueue,
}

impl Barrier {
    /// 创建新的屏障
    ///
    /// # 参数
    /// - `count`: 需要等待的线程数量
    pub const fn new(count: usize) -> Self {
        Self {
            count: AtomicUsize::new(0),
            total: count,
            generation: Spinlock::new(0),
            wait_queue: WaitQueue::new(),
        }
    }

    /// 等待所有线程到达
    ///
    /// 当最后一个线程到达时，所有线程被唤醒
    pub fn wait(&self) {
        let generation = *self.generation.lock();

        // 增加计数
        let count = self.count.fetch_add(1, Ordering::AcqRel) + 1;

        if count == self.total {
            // 最后一个线程到达
            // 重置计数，增加世代号
            self.count.store(0, Ordering::Release);
            *self.generation.lock() = generation + 1;

            // 唤醒所有等待的线程
            self.wait_queue.wake_all();
        } else {
            // 等待其他线程
            loop {
                self.wait_queue.wait();

                // 检查世代号，确保不会被旧的通知唤醒
                let current_gen = *self.generation.lock();
                if current_gen > generation {
                    break;
                }
            }
        }
    }
}
