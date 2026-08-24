// ============================================================
// 互斥锁 (Mutex)
// ============================================================
// 可阻塞的互斥锁，获取失败时线程会睡眠

use super::wait_queue::WaitQueue;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// 互斥锁
///
/// Mutex vs Spinlock：
/// - Mutex：获取失败时线程睡眠，适合长时间持有锁的场景
/// - Spinlock：获取失败时忙等待，适合短时间持有锁的场景
///
/// 实现方式：
/// - 使用自旋锁保护内部状态（快速路径）
/// - 使用等待队列实现阻塞（慢速路径）
/// - 基于 futex 思想：快速的用户态操作 + 必要时的内核态睡眠
pub struct Mutex<T: ?Sized> {
    locked: AtomicBool,
    wait_queue: WaitQueue,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    /// 创建新的互斥锁
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            wait_queue: WaitQueue::new(),
            data: UnsafeCell::new(data),
        }
    }

    /// 获取锁
    ///
    /// 两阶段获取策略：
    /// 1. 快速路径：尝试原子交换，成功则立即返回
    /// 2. 慢速路径：进入等待队列，阻塞直到被唤醒
    ///
    /// 为什么这样设计：
    /// - 大部分情况下锁都是空闲的，快速路径避免不必要的开销
    /// - 只有竞争时才进入慢速路径，减少 CPU 浪费
    pub fn lock<'a>(&'a self) -> MutexGuard<'a, T> {
        // 快速路径：尝试获取锁
        if self.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok() {
            return MutexGuard { mutex: self };
        }

        // 慢速路径：阻塞等待
        loop {
            // 在等待队列上睡眠
            self.wait_queue.wait();

            // 被唤醒后尝试获取锁
            if self.locked.compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed
            ).is_ok() {
                return MutexGuard { mutex: self };
            }
        }
    }

    /// 尝试获取锁（非阻塞）
    pub fn try_lock<'a>(&'a self) -> Option<MutexGuard<'a, T>> {
        if self.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok() {
            Some(MutexGuard { mutex: self })
        } else {
            None
        }
    }
}

/// 互斥锁守卫
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    mutex: &'a Mutex<T>,
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        // 释放锁
        self.mutex.locked.store(false, Ordering::Release);

        // 唤醒一个等待的线程
        self.mutex.wait_queue.wake_one();
    }
}

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}
