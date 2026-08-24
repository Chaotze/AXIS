// ============================================================
// 信号量 (Semaphore)
// ============================================================
// 计数信号量，控制对有限资源的访问

use super::wait_queue::WaitQueue;
use core::sync::atomic::{AtomicUsize, Ordering};

/// 信号量
///
/// 特点：
/// - 维护一个计数值，表示可用资源数
/// - P 操作（acquire）：计数减 1，如果为 0 则阻塞
/// - V 操作（release）：计数加 1，唤醒等待的线程
///
/// 应用场景：
/// - 限制并发访问数量（如连接池）
/// - 实现生产者-消费者模型
pub struct Semaphore {
    count: AtomicUsize,
    wait_queue: WaitQueue,
}

impl Semaphore {
    /// 创建新的信号量
    ///
    /// # 参数
    /// - `initial`: 初始计数值
    pub const fn new(initial: usize) -> Self {
        Self {
            count: AtomicUsize::new(initial),
            wait_queue: WaitQueue::new(),
        }
    }

    /// P 操作（获取资源）
    ///
    /// 如果计数为 0，当前线程会阻塞
    pub fn acquire(&self) {
        loop {
            let count = self.count.load(Ordering::Acquire);

            if count > 0 {
                // 尝试减少计数
                if self.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::Acquire,
                    Ordering::Relaxed
                ).is_ok() {
                    return;
                }
            } else {
                // 计数为 0，阻塞等待
                self.wait_queue.wait();
            }
        }
    }

    /// V 操作（释放资源）
    ///
    /// 增加计数并唤醒一个等待的线程
    pub fn release(&self) {
        self.count.fetch_add(1, Ordering::Release);
        self.wait_queue.wake_one();
    }

    /// 尝试获取（非阻塞）
    pub fn try_acquire(&self) -> bool {
        let count = self.count.load(Ordering::Acquire);

        if count > 0 {
            self.count.compare_exchange(
                count,
                count - 1,
                Ordering::Acquire,
                Ordering::Relaxed
            ).is_ok()
        } else {
            false
        }
    }
}
