// ============================================================
// 事件 (Event)
// ============================================================
// 一次性或可重置的信号机制

use super::wait_queue::WaitQueue;
use core::sync::atomic::{AtomicBool, Ordering};

/// 事件
///
/// 事件是简单的信号机制：
/// - 线程可以等待事件
/// - 其他线程可以触发事件
/// - 支持自动重置或手动重置
///
/// 应用场景：
/// - 简单的线程通知
/// - 实现一次性初始化
pub struct Event {
    signaled: AtomicBool,
    auto_reset: bool,
    wait_queue: WaitQueue,
}

impl Event {
    /// 创建新的事件
    ///
    /// # 参数
    /// - `auto_reset`: 是否自动重置（被等待后自动清除信号）
    pub const fn new(auto_reset: bool) -> Self {
        Self {
            signaled: AtomicBool::new(false),
            auto_reset,
            wait_queue: WaitQueue::new(),
        }
    }

    /// 等待事件被触发
    pub fn wait(&self) {
        loop {
            // 检查信号
            if self.signaled.load(Ordering::Acquire) {
                // 如果是自动重置，清除信号
                if self.auto_reset {
                    self.signaled.store(false, Ordering::Release);
                }
                return;
            }

            // 等待信号
            self.wait_queue.wait();
        }
    }

    /// 触发事件
    ///
    /// 设置信号并唤醒等待的线程
    pub fn signal(&self) {
        self.signaled.store(true, Ordering::Release);

        if self.auto_reset {
            // 自动重置：只唤醒一个线程
            self.wait_queue.wake_one();
        } else {
            // 手动重置：唤醒所有线程
            self.wait_queue.wake_all();
        }
    }

    /// 清除信号（手动重置）
    pub fn reset(&self) {
        self.signaled.store(false, Ordering::Release);
    }

    /// 检查是否已触发（不阻塞）
    pub fn is_signaled(&self) -> bool {
        self.signaled.load(Ordering::Acquire)
    }
}
