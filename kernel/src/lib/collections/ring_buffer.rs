// ============================================================
// 环形缓冲区（定长循环缓冲）
// ============================================================
// 固定容量的 FIFO 循环队列，满时覆盖最旧元素。
//
// 为什么需要环形缓冲区：
// - 串口收发缓冲、内核日志环形缓冲、键盘输入队列等
//   生产者/消费者异步场景的标准结构
// - 与普通数组队列相比，入队出队都只需移动指针，
//   无需搬移数据，O(1) 且无内存碎片
//
// 为什么设计为定长（const 泛型 N）：
// - 内核启动早期没有动态分配器，定长数组直接内嵌在
//   使用方结构体中（如"32 项键盘缓冲"），零堆开销
//
// 为什么用 MaybeUninit<[T; N]> 而不是 [T; N]：
// - 后者要求 T: Copy/Default，且所有槽位在构造时就被
//   "假初始化"；MaybeUninit 使槽位只在 push/pop 时
//   显式初始化与析构，支持任意类型且无额外开销
//
// 为什么满时覆盖最旧（push 返回被挤出的元素）：
// - 日志/遥测语义下"丢最旧保最新"是正确的取舍；
//   需要"满则拒绝"的调用方使用 try_push

use core::mem::MaybeUninit;

/// 定长环形缓冲区
pub struct RingBuffer<T, const N: usize> {
    /// 槽位数组（未初始化内存，按 head/tail/count 管理）
    buf: MaybeUninit<[T; N]>,
    /// 队头下标：最旧元素的存放位置
    head: usize,
    /// 队尾下标：下一个元素的写入位置
    tail: usize,
    /// 当前元素个数
    count: usize,
}

impl<T, const N: usize> RingBuffer<T, N> {
    /// 创建空缓冲区
    ///
    /// # 为什么断言 N > 0：
    /// - 容量为 0 时头尾指针退化为同一位置，满/空状态
    ///   无法区分（除非额外引入布尔标志，徒增复杂度）
    pub const fn new() -> Self {
        assert!(N > 0, "环形缓冲区容量必须大于 0");
        Self {
            buf: MaybeUninit::uninit(),
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// 容量
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// 当前元素个数
    #[inline]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// 是否为空
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 是否已满
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.count == N
    }

    /// 入队；缓冲区已满时覆盖最旧元素并返回它
    ///
    /// 返回 `Some(被挤出的旧元素)` 或 `None`（未满，无挤出）。
    pub fn push(&mut self, value: T) -> Option<T> {
        if self.is_full() {
            // 满：读走最旧元素（其槽位将被新值覆盖）
            let evicted = unsafe { self.read_at(self.head) };
            unsafe { self.write_at(self.tail, value) };
            // 头尾同时前移：队列整体滑动一个位置
            self.head = (self.head + 1) % N;
            self.tail = (self.tail + 1) % N;
            Some(evicted)
        } else {
            unsafe { self.write_at(self.tail, value) };
            self.tail = (self.tail + 1) % N;
            self.count += 1;
            None
        }
    }

    /// 入队；缓冲区已满时拒绝并原样返回元素
    pub fn try_push(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        unsafe { self.write_at(self.tail, value) };
        self.tail = (self.tail + 1) % N;
        self.count += 1;
        Ok(())
    }

    /// 出队最旧元素
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let value = unsafe { self.read_at(self.head) };
        self.head = (self.head + 1) % N;
        self.count -= 1;
        Some(value)
    }

    /// 查看队头（最旧）元素而不出队
    pub fn peek(&self) -> Option<&T> {
        if self.is_empty() {
            return None;
        }
        // 安全前提：count > 0 时 head 指向已初始化的槽位
        Some(unsafe { &*self.slot_ptr(self.head) })
    }

    /// 读取指定下标槽位中的元素（并转移所有权）
    ///
    /// # Safety
    /// 调用方必须保证该槽位当前存放着已初始化的元素。
    #[inline]
    unsafe fn read_at(&self, index: usize) -> T {
        unsafe { self.slot_ptr(index).read() }
    }

    /// 向指定下标槽位写入元素
    ///
    /// # Safety
    /// 调用方必须保证该槽位当前没有存活的元素。
    #[inline]
    unsafe fn write_at(&mut self, index: usize, value: T) {
        unsafe { self.slot_ptr(index).write(value) }
    }

    /// 取得槽位数组第 index 个元素的裸指针
    ///
    /// 为什么用裸指针而非 MaybeUninit 引用：
    /// - 槽位的初始化状态是运行时信息（head/tail/count），
    ///   类型系统无法表达"部分初始化"，裸指针 + 调用方
    ///   维护不变式是最直接也最透明的表达方式
    #[inline]
    fn slot_ptr(&self, index: usize) -> *mut T {
        unsafe { (self.buf.as_ptr() as *mut T).add(index) }
    }
}

impl<T, const N: usize> Drop for RingBuffer<T, N> {
    fn drop(&mut self) {
        // 析构尚未出队的存活元素，防止泄漏
        // 为什么必须手动实现：槽位是 MaybeUninit，
        // 编译器不知道哪些元素存活，默认不会调用它们的 Drop
        for _ in 0..self.count {
            // 读出元素得到所有权，语句结束时由编译器析构它
            unsafe {
                self.read_at(self.head);
            }
            self.head = (self.head + 1) % N;
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::rc::Rc;
    use std::cell::Cell;

    use super::*;

    #[test]
    fn test_push_pop_roundtrip() {
        let mut rb: RingBuffer<i32, 4> = RingBuffer::new();
        assert!(rb.is_empty());
        assert_eq!(rb.pop(), None);
        assert_eq!(rb.peek(), None);

        for v in 1..=4 {
            assert_eq!(rb.push(v), None);
        }
        assert!(rb.is_full());
        assert_eq!(rb.peek(), Some(&1));

        for v in 1..=4 {
            assert_eq!(rb.pop(), Some(v));
        }
        assert!(rb.is_empty());
    }

    #[test]
    fn test_wraparound() {
        // 容量 4，写入 6 个元素，验证环形回绕与覆盖语义
        let mut rb: RingBuffer<i32, 4> = RingBuffer::new();
        assert_eq!(rb.push(1), None);
        assert_eq!(rb.push(2), None);
        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        // 此时 head=tail=2，再写 4 个会回绕
        for v in 3..=6 {
            assert_eq!(rb.push(v), None);
        }
        assert_eq!(rb.pop(), Some(3));
        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), Some(5));
        assert_eq!(rb.pop(), Some(6));
    }

    #[test]
    fn test_evict_oldest_on_full() {
        let mut rb: RingBuffer<i32, 4> = RingBuffer::new();
        for v in 1..=4 {
            assert_eq!(rb.push(v), None);
        }
        // 满：push 5 挤出 1，push 6 挤出 2
        assert_eq!(rb.push(5), Some(1));
        assert_eq!(rb.push(6), Some(2));
        let remaining: std::vec::Vec<i32> = std::iter::from_fn(|| rb.pop()).collect();
        assert_eq!(remaining, [3, 4, 5, 6]);
    }

    #[test]
    fn test_try_push_rejects_when_full() {
        let mut rb: RingBuffer<i32, 2> = RingBuffer::new();
        assert_eq!(rb.try_push(1), Ok(()));
        assert_eq!(rb.try_push(2), Ok(()));
        assert_eq!(rb.try_push(3), Err(3));
        assert_eq!(rb.len(), 2);
    }

    #[test]
    fn test_drop_releases_remaining_elements() {
        // 验证 Drop 实现：销毁缓冲区时必须析构存活的元素
        struct DropCounter {
            count: Rc<Cell<usize>>,
        }
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.count.set(self.count.get() + 1);
            }
        }

        let counter = Rc::new(Cell::new(0));
        {
            let mut rb: RingBuffer<DropCounter, 4> = RingBuffer::new();
            for _ in 0..3 {
                rb.push(DropCounter {
                    count: counter.clone(),
                });
            }
            assert_eq!(counter.get(), 0);
        } // rb 离开作用域，3 个存活元素应被析构
        assert_eq!(counter.get(), 3);
    }
}
