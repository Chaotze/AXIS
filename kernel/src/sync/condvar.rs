// ============================================================
// 条件变量 (Condition Variable)
// ============================================================
// 配合 Mutex 使用，提供等待和通知机制

use super::mutex::MutexGuard;
use super::wait_queue::WaitQueue;

/// 条件变量
///
/// 条件变量用于线程间的通知机制：
/// - wait: 释放锁并等待，被唤醒后重新获取锁
/// - notify_one: 唤醒一个等待的线程
/// - notify_all: 唤醒所有等待的线程
///
/// 典型用法：
/// ```
/// let mutex = Mutex::new(data);
/// let condvar = CondVar::new();
///
/// // 等待条件
/// let mut guard = mutex.lock();
/// while !condition {
///     condvar.wait(&mut guard);
/// }
///
/// // 通知条件满足
/// let guard = mutex.lock();
/// // ... 修改数据 ...
/// condvar.notify_one();
/// ```
pub struct CondVar {
    wait_queue: WaitQueue,
}

impl CondVar {
    /// 创建新的条件变量
    pub const fn new() -> Self {
        Self {
            wait_queue: WaitQueue::new(),
        }
    }

    /// 等待条件满足
    ///
    /// 操作步骤：
    /// 1. 释放互斥锁
    /// 2. 阻塞当前线程
    /// 3. 被唤醒后重新获取锁
    ///
    /// 为什么需要 Mutex：
    /// - 保护共享数据的一致性
    /// - 防止 lost wakeup 问题（信号在 wait 之前发送）
    pub fn wait<'a, T>(&self, _guard: &mut MutexGuard<'a, T>) {
        // 注意：这里需要手动释放锁并在唤醒后重新获取
        // 实际实现需要与 Mutex 深度集成

        // 暂时简化实现
        self.wait_queue.wait();
    }

    /// 唤醒一个等待的线程
    pub fn notify_one(&self) {
        self.wait_queue.wake_one();
    }

    /// 唤醒所有等待的线程
    pub fn notify_all(&self) {
        self.wait_queue.wake_all();
    }
}
