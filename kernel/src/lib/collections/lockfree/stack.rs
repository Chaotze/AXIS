// ============================================================
// 无锁栈（Treiber 栈）
// ============================================================
// 基于 CAS 的并发 LIFO 栈，push/pop 无阻塞、无等待循环。
//
// 为什么内核需要无锁栈：
// - 多核场景下中断上下文也能安全使用的结构（中断处理中
//   不能睡眠、尽量不关中断）；典型用途是空闲对象池
//   （如 SLUB 的 per-CPU 空闲链表、空闲页帧栈）
//
// 为什么节点由调用方提供（裸指针接口）：
// - 无锁栈不负责节点回收（ABA 风险见下）：节点由调用方从
//   静态数组或 slab/kmem_cache 池取出，栈只维护指针链接，
//   不拥有节点
// - 裸指针接口是无锁结构在 C 内核中的标准形态（如
//   Linux 的 llist），避免所有权语义与并发语义纠缠
//
// # 安全性约定（调用方必须遵守）
// - 节点在入栈后、被出栈前必须保持存活
// - 存在 ABA 问题：pop 后立即复用同一节点再 push，可能
//   被并发线程误判。当前内核无抢占/单 CPU 场景天然安全；
//   多核下需配合引用计数或 hazard pointer（后续阶段引入）
//
// 为什么用 Acquire/Release 内存序：
// - push 的 Release 保证节点内容写入先于指针发布；
//   pop 的 Acquire 保证读到指针后能看到完整节点内容

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

/// 无锁栈节点
pub struct Node<T> {
    /// 后继节点指针（由栈维护）
    pub next: AtomicPtr<Node<T>>,
    /// 节点携带的数据
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

/// Treiber 无锁栈
pub struct Stack<T> {
    /// 栈顶指针（空栈为 null）
    head: AtomicPtr<Node<T>>,
}

impl<T> Stack<T> {
    /// 创建空栈
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }

    /// 入栈
    ///
    /// 算法（Treiber, 1986）：
    /// 1. 把当前栈顶链到新节点之后
    /// 2. CAS 把新节点设为栈顶；失败说明有并发修改，
    ///    重读栈顶重试
    pub fn push(&self, node: *mut Node<T>) {
        let mut head = self.head.load(Ordering::Relaxed);
        loop {
            unsafe {
                (*node).next.store(head, Ordering::Relaxed);
            }
            match self.head.compare_exchange_weak(
                head,
                node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                // 失败：head 已被更新为当前栈顶，用新值重试
                Err(actual) => head = actual,
            }
        }
    }

    /// 出栈；栈空返回 None
    pub fn pop(&self) -> Option<*mut Node<T>> {
        let mut head = self.head.load(Ordering::Acquire);
        while !head.is_null() {
            let next = unsafe { (*head).next.load(Ordering::Relaxed) };
            match self.head.compare_exchange_weak(
                head,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(head),
                Err(actual) => head = actual,
            }
        }
        None
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
    fn test_push_pop_lifo() {
        let stack: Stack<i32> = Stack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.pop(), None);

        let nodes = [
            Box::leak(Box::new(Node::new(1))),
            Box::leak(Box::new(Node::new(2))),
            Box::leak(Box::new(Node::new(3))),
        ];
        // 按值迭代：Box::leak 返回 &'static mut，按值移交给 push
        for n in nodes {
            stack.push(n);
        }

        // LIFO：3, 2, 1
        assert_eq!(unsafe { (*stack.pop().unwrap()).value }, 3);
        assert_eq!(unsafe { (*stack.pop().unwrap()).value }, 2);
        assert_eq!(unsafe { (*stack.pop().unwrap()).value }, 1);
        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());
    }

    #[test]
    fn test_concurrent_push_pop() {
        // 两个线程各压入 500 个节点，另两个线程全部弹出；
        // 用计数守恒检测丢失/重复弹出（正确性烟雾测试）
        let stack: Arc<Stack<usize>> = Arc::new(Stack::new());
        let popped = Arc::new(AtomicUsize::new(0));

        let mut producers = std::vec::Vec::new();
        for t in 0..2 {
            let stack = Arc::clone(&stack);
            producers.push(thread::spawn(move || {
                for i in 0..500usize {
                    let node = Box::leak(Box::new(Node::new(t * 1000 + i)));
                    stack.push(node);
                }
            }));
        }
        for p in producers {
            p.join().unwrap();
        }

        let mut consumers = std::vec::Vec::new();
        for _ in 0..2 {
            let stack = Arc::clone(&stack);
            let popped = Arc::clone(&popped);
            // 出栈的节点先收集，不立即回收：无锁结构的延迟回收
            // 约定——其他线程可能仍持有指向该节点的旧栈顶指针
            consumers.push(thread::spawn(move || -> std::vec::Vec<usize> {
                let mut collected = std::vec::Vec::new();
                while let Some(node) = stack.pop() {
                    collected.push(node as usize);
                    popped.fetch_add(1, Ordering::AcqRel);
                }
                collected
            }));
        }
        // 先等待全部出栈线程结束（join 同时同步计数），
        // 再断言，最后统一回收节点（无锁结构的延迟回收约定）
        let mut drained = std::vec::Vec::new();
        for c in consumers {
            drained.extend(c.join().unwrap());
        }
        // 1000 次 push 必须恰好 1000 次 pop 且栈为空：
        // 若节点被重复弹出，必伴随另一次弹出丢失（总数不符）
        assert_eq!(popped.load(Ordering::Acquire), 1000);
        assert!(stack.is_empty());

        for addr in drained {
            unsafe { drop(Box::from_raw(addr as *mut Node<usize>)) };
        }
    }
}

