// ============================================================
// 读写锁 (RwLock)
// ============================================================
// 允许多个读者或一个写者

use super::wait_queue::WaitQueue;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicIsize, Ordering};

/// 读写锁
///
/// 特点：
/// - 多个线程可以同时持有读锁
/// - 只有一个线程可以持有写锁
/// - 读锁和写锁互斥
///
/// 适用场景：
/// - 读多写少的数据结构
/// - 提高并发读取性能
///
/// 状态表示：
/// - state > 0: 有 state 个读者
/// - state = 0: 无锁
/// - state = -1: 有一个写者
pub struct RwLock<T: ?Sized> {
    state: AtomicIsize,
    read_queue: WaitQueue,
    write_queue: WaitQueue,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for RwLock<T> {}
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}

impl<T> RwLock<T> {
    /// 创建新的读写锁
    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicIsize::new(0),
            read_queue: WaitQueue::new(),
            write_queue: WaitQueue::new(),
            data: UnsafeCell::new(data),
        }
    }

    /// 获取读锁
    ///
    /// 策略：
    /// - 如果没有写者，增加读者计数
    /// - 如果有写者，等待写者释放
    pub fn read<'a>(&'a self) -> RwLockReadGuard<'a, T> {
        loop {
            let state = self.state.load(Ordering::Acquire);

            // 如果没有写者（state >= 0），尝试增加读者计数
            if state >= 0 {
                if self.state.compare_exchange(
                    state,
                    state + 1,
                    Ordering::Acquire,
                    Ordering::Relaxed
                ).is_ok() {
                    return RwLockReadGuard { lock: self };
                }
            } else {
                // 有写者，等待
                self.read_queue.wait();
            }
        }
    }

    /// 获取写锁
    ///
    /// 策略：
    /// - 只有在没有读者和写者时才能获取
    pub fn write<'a>(&'a self) -> RwLockWriteGuard<'a, T> {
        loop {
            // 尝试从 0 变为 -1
            if self.state.compare_exchange(
                0,
                -1,
                Ordering::Acquire,
                Ordering::Relaxed
            ).is_ok() {
                return RwLockWriteGuard { lock: self };
            }

            // 等待所有读者和写者释放
            self.write_queue.wait();
        }
    }
}

/// 读锁守卫
pub struct RwLockReadGuard<'a, T: ?Sized + 'a> {
    lock: &'a RwLock<T>,
}

impl<'a, T: ?Sized> Drop for RwLockReadGuard<'a, T> {
    fn drop(&mut self) {
        // 减少读者计数
        let old = self.lock.state.fetch_sub(1, Ordering::Release);

        // 如果是最后一个读者，唤醒等待的写者
        if old == 1 {
            self.lock.write_queue.wake_one();
        }
    }
}

impl<'a, T: ?Sized> Deref for RwLockReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

/// 写锁守卫
pub struct RwLockWriteGuard<'a, T: ?Sized + 'a> {
    lock: &'a RwLock<T>,
}

impl<'a, T: ?Sized> Drop for RwLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        // 重置状态
        self.lock.state.store(0, Ordering::Release);

        // 唤醒等待的线程
        // 优先唤醒写者（避免写者饥饿）
        self.lock.write_queue.wake_one();
        self.lock.read_queue.wake_all();
    }
}

impl<'a, T: ?Sized> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
