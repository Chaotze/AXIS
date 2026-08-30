// ============================================================
// 无锁队列（Michael-Scott 队列）
// ============================================================
// 基于 CAS 的并发 FIFO 队列，支持多生产者多消费者（MPMC）。
//
// 为什么内核需要无锁队列：
// - 工作队列、中断下半部投递、跨核消息等生产者/消费者
//   场景；与无锁栈互补（FIFO 语义）
//
// 为什么采用哨兵节点：
// - 队列恒有一个不携带数据的哨兵节点，使"判空"退化为
//   单指针判断（head.next == null），避免头尾指针同为
//   null 时的多情况处理；这也是 Michael-Scott 原文的设计
// - 哨兵由调用方提供（无锁队列节点同样不负责回收，
//   可从静态数组或 slab 池取出），dequeue 永远不会
//   返回哨兵本身
//
// 为什么 tail 允许滞后：
// - enqueue 分为两步：把新节点链到队尾、再推进 tail；
//   两步之间若被并发操作打断，tail 可能指向非最后一个
//   节点——这是算法允许的松弛，后续操作看到滞后 tail
//   会"顺手"帮它推进，从而摊还了开销
//
// # 安全性约定（调用方必须遵守）
// - 节点（含哨兵）在出队前必须保持存活；ABA 风险同
//   lockfree/stack.rs 的说明

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

/// 无锁队列节点
pub struct Node<T> {
    /// 后继节点指针（由队列维护）
    pub next: AtomicPtr<Node<T>>,
    /// 节点携带的数据（哨兵节点的值无意义）
    pub value: T,
}

impl<T> Node<T> {
    /// 创建节点
    pub const fn new(value: T) -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            value,
        }
    }
}

/// Michael-Scott 无锁队列
pub struct Queue<T> {
    /// 队头（指向哨兵节点）
    head: AtomicPtr<Node<T>>,
    /// 队尾（可能滞后于实际最后一个节点）
    tail: AtomicPtr<Node<T>>,
}

impl<T> Queue<T> {
    /// 以哨兵节点创建空队列
    ///
    /// 哨兵节点的 next 必须为 null（Node::new 保证）。
    pub const fn new(sentinel: *mut Node<T>) -> Self {
        Self {
            head: AtomicPtr::new(sentinel),
            tail: AtomicPtr::new(sentinel),
        }
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        unsafe { (*head).next.load(Ordering::Acquire).is_null() }
    }

    /// 入队（把节点追加到队尾）
    pub fn enqueue(&self, node: *mut Node<T>) {
        unsafe {
            (*node).next.store(ptr::null_mut(), Ordering::Relaxed);
        }

        loop {
            // 每轮重读 tail：CAS 失败或 tail 滞后时，必须从
            // 最新队尾重新出发（否则会解引用已被别人推后/出队的
            // 节点，造成悬垂访问）
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*tail).next.load(Ordering::Acquire) };
            if next.is_null() {
                // 尾节点没有后继：尝试把新节点挂上
                if unsafe { &(*tail).next }
                    .compare_exchange_weak(
                        ptr::null_mut(),
                        node,
                        Ordering::Release,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    // 挂载成功；尽力推进 tail，失败也无妨
                    // （后续操作看到滞后 tail 会帮忙推进）
                    let _ = self.tail.compare_exchange_weak(
                        tail,
                        node,
                        Ordering::Release,
                        Ordering::Relaxed,
                    );
                    return;
                }
                // 挂载失败：有并发者抢先，重读 tail 重试
            } else {
                // tail 滞后：帮忙推进再重试
                let _ = self.tail.compare_exchange_weak(
                    tail,
                    next,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            }
        }
    }

    /// 出队；队列为空返回 None（不返回哨兵）
    pub fn dequeue(&self) -> Option<*mut Node<T>> {
        let mut head = self.head.load(Ordering::Acquire);
        loop {
            let next = unsafe { (*head).next.load(Ordering::Acquire) };
            if next.is_null() {
                return None; // 只有哨兵：队列为空
            }
            // 推进 head 到后继；失败说明并发者已推进，必须用
            // 失败返回的实际 head 重试——否则会继续解引用已被
            // 出队的旧 head（其内容可能已被回收，悬垂访问）
            match self
                .head
                .compare_exchange_weak(head, next, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => {
                    // 返回数据节点；哨兵节点留在队中
                    // （旧哨兵变成"垃圾节点"，由调用方在合适时机
                    // 复用或回收——内核无锁队列的惯例）
                    return Some(next);
                }
                Err(actual) => head = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::boxed::Box;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;

    #[test]
    fn test_fifo_order() {
        let sentinel = Box::leak(Box::new(Node::new(0)));
        let queue: Queue<i32> = Queue::new(sentinel);
        assert!(queue.is_empty());
        assert_eq!(queue.dequeue(), None);

        let nodes = [
            Box::leak(Box::new(Node::new(1))),
            Box::leak(Box::new(Node::new(2))),
            Box::leak(Box::new(Node::new(3))),
        ];
        // 按值迭代：Box::leak 返回 &'static mut，按值移交给 enqueue
        for n in nodes {
            queue.enqueue(n);
        }
        assert!(!queue.is_empty());

        // FIFO：1, 2, 3
        for expected in [1, 2, 3] {
            let node = queue.dequeue().unwrap();
            assert_eq!(unsafe { (*node).value }, expected);
        }
        assert_eq!(queue.dequeue(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_concurrent_enqueue_dequeue() {
        // 2 个生产者各入队 300 个，2 个消费者全部出队；
        // 总数守恒 + 队列最终为空即正确性的烟雾测试
        let sentinel = Box::leak(Box::new(Node::new(0usize)));
        let queue: Arc<Queue<usize>> = Arc::new(Queue::new(sentinel));
        let dequeued = Arc::new(AtomicUsize::new(0));

        let mut producers = std::vec::Vec::new();
        for t in 0..2 {
            let queue = Arc::clone(&queue);
            producers.push(thread::spawn(move || {
                for i in 0..300usize {
                    let node = Box::leak(Box::new(Node::new(t * 1000 + i)));
                    queue.enqueue(node);
                }
            }));
        }
        for p in producers {
            p.join().unwrap();
        }

        let mut consumers = std::vec::Vec::new();
        for _ in 0..2 {
            let queue = Arc::clone(&queue);
            let dequeued = Arc::clone(&dequeued);
            // 出队的节点先收集，不立即回收：无锁结构的延迟回收
            // 约定——其他线程可能仍持有指向该节点的旧队头指针
            consumers.push(thread::spawn(move || -> std::vec::Vec<usize> {
                let mut collected = std::vec::Vec::new();
                while let Some(node) = queue.dequeue() {
                    collected.push(node as usize);
                    dequeued.fetch_add(1, Ordering::AcqRel);
                }
                collected
            }));
        }
        // 先等待全部出队线程结束（join 同时同步计数），
        // 再断言：此时节点仍存活，判空可安全解引用队首 next
        let mut drained = std::vec::Vec::new();
        for c in consumers {
            drained.extend(c.join().unwrap());
        }
        assert_eq!(dequeued.load(Ordering::Acquire), 600);
        assert!(queue.is_empty());

        // 最后统一回收节点（无锁结构的延迟回收约定）
        for addr in drained {
            unsafe { drop(Box::from_raw(addr as *mut Node<usize>)) };
        }
    }
}

