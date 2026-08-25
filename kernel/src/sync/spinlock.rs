// ============================================================
// 自旋锁 (Spinlock)
// ============================================================
// 最基础的同步原语，其他锁都依赖它

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};
use core::ops::{Deref, DerefMut};
use core::hint;

/// 自旋锁
///
/// 特点：
/// - 忙等待（spin）：获取锁失败时持续尝试，不会睡眠
/// - 适用场景：临界区很小、持锁时间短
/// - 不适用场景：临界区大、可能长时间持有锁
///
/// 为什么需要自旋锁：
/// - 内核早期初始化时，调度器还未就绪，不能睡眠
/// - 中断处理程序中不能睡眠，只能用自旋锁
/// - 某些情况下自旋比睡眠唤醒的开销更小
pub struct Spinlock<T: ?Sized> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Spinlock<T> {}
unsafe impl<T: ?Sized + Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    /// 创建新的自旋锁
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// 获取锁
    ///
    /// 如果锁已被占用，会一直自旋等待直到获取成功。
    ///
    /// 为什么用 compare_exchange：
    /// - 原子地检查并设置锁状态
    /// - 防止竞争条件：如果用分离的 load 和 store，可能多个线程同时认为锁空闲
    ///
    /// Ordering 选择：
    /// - Acquire：确保临界区内的读写不会被重排到获取锁之前
    /// - Relaxed：失败时只是重试，不需要同步
    pub fn lock<'a>(&'a self) -> SpinlockGuard<'a, T> {
        while self.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            // 自旋等待
            // 为什么用 spin_loop：提示 CPU 这是自旋循环，可以优化功耗
            while self.locked.load(Ordering::Relaxed) {
                hint::spin_loop();
            }
        }

        SpinlockGuard {
            lock: self,
        }
    }

    /// 尝试获取锁（非阻塞）
    ///
    /// 如果锁已被占用，立即返回 None
    pub fn try_lock<'a>(&'a self) -> Option<SpinlockGuard<'a, T>> {
        if self.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok() {
            Some(SpinlockGuard { lock: self })
        } else {
            None
        }
    }

    /// 强制解锁（危险！）
    ///
    /// 仅在紧急情况使用，如 panic 处理中需要打印信息
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

/// 自旋锁守卫
///
/// RAII 模式：离开作用域时自动释放锁
///
/// 为什么需要守卫：
/// - 防止忘记解锁导致死锁
/// - 异常安全：即使发生 panic，析构函数仍会执行
/// - 类型系统保证：持有守卫才能访问数据
pub struct SpinlockGuard<'a, T: ?Sized + 'a> {
    lock: &'a Spinlock<T>,
}

impl<'a, T: ?Sized> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        // 释放锁
        // Ordering::Release：确保临界区内的写操作对后续获取锁的线程可见
        self.lock.locked.store(false, Ordering::Release);
    }
}

impl<'a, T: ?Sized> Deref for SpinlockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
