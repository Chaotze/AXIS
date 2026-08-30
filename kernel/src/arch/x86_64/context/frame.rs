// ============================================================
// 中断/陷阱帧
// ============================================================
// 保存 CPU 寄存器状态

/// 陷阱帧（Trap Frame）
///
/// 保存任务切换或中断时的 CPU 状态
///
/// 布局必须与 interrupt/entry.asm 中的保存顺序一致
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TrapFrame {
    // 通用寄存器（由中断存根保存）
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // 异常相关（由存根或 CPU 压栈）
    pub vector: u64,
    pub error_code: u64,

    // 中断帧（由 CPU 自动保存）
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TrapFrame {
    /// 创建新的陷阱帧
    ///
    /// 用于初始化新任务的上下文
    pub fn new(entry: u64, stack: u64) -> Self {
        Self {
            // 通用寄存器初始化为 0
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,

            // 无异常
            vector: 0,
            error_code: 0,

            // 中断帧
            rip: entry,
            cs: 0x08,     // 内核代码段
            rflags: 0x202, // IF=1（启用中断），保留位=1
            rsp: stack,
            ss: 0x10,     // 内核数据段
        }
    }
}

// ============================================================
// 中断返回式切换帧（SwitchFrame）
// ============================================================
//
// 与上面的 TrapFrame（r15 开头，配合 switch.asm 的协作式
// 直接切换）不同，SwitchFrame 严格镜像 interrupt/entry.asm
// 的 irq_common 在栈上的保存布局（rax 最深、r15 最浅），
// 供"定时器中断内换栈 + iretq 返回新任务"的抢占式切换使用。
//
// 为什么需要两种帧：
// - 抢占式切换的入口是中断存根：CPU 把 rip/cs/rflags(/rsp/ss)
//   压栈，存根再压 vector/error/通用寄存器；要"换到新任务"
//   只需把 RSP 指向新任务的同布局帧，存根统一弹栈 + iretq
//   即可落入新任务——新帧必须与存根保存顺序逐字节一致
// - switch.asm 的协作式切换不经过中断门，其 TrapFrame 是
//   自定布局；两者语义不同，分开定义避免互相迁就
//
// 内存布局（低地址 → 高地址，RSP 指向 r15 槽）：
//   [r15, r14, ..., rax, vector, error_code,
//    rip, cs, rflags, rsp, ss]
// 共 22 个槽 = 176 字节（16 字节对齐）。
//
// 为什么 r15 在最低地址：
// - entry.asm 的保存序列依次 push rax..r15，最后 push 的
//   r15 落在栈顶（最低地址）；恢复序列 pop r15 开始、
//   地址递增——结构体字段序必须与之一一镜像，
//   否则弹回的寄存器全部错位（iretq 落点亦错乱）
#[repr(C)]
pub struct SwitchFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// RSP 相对帧基址的偏移：r15 在最低地址，偏移为 0
const R15_OFFSET: usize = 0;
impl SwitchFrame {
    /// 在任务栈顶初始化"首次运行帧"，返回应写入的 RSP 值
    ///
    /// # 参数
    /// - `entry`: 任务入口函数地址（iretq 后从该处开始执行）
    /// - `stack_top`: 任务内核栈顶（16 字节对齐）
    /// - `arg0`: 传入入口函数的第一个参数（经 RDI）
    ///
    /// 为什么帧放在栈顶之下：
    /// - 帧占 176 字节，入口函数从 stack_top 向下用栈；
    ///   帧在 [stack_top-176, stack_top)，两者互不侵犯
    ///
    /// 为什么 rsp/ss 槽对内核态 iretq 是"冗余但保留"：
    /// - 内核态中断不压 rsp/ss（无特权级切换），iretq 只弹
    ///   rip/cs/rflags；保留两槽使布局与将来用户态切换
    ///   （特权级变化、CPU 压满 5 槽）完全兼容
    pub fn init_stack(entry: usize, stack_top: usize, arg0: u64) -> usize {
        let base = (stack_top - core::mem::size_of::<SwitchFrame>()) & !0xF;
        let frame = SwitchFrame {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: arg0, rsi: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            vector: 0, // iretq 不读，存根弹栈时跳过
            error_code: 0,
            rip: entry as u64,
            cs: 0x08,     // 内核代码段
            rflags: 0x202, // IF=1：任务以开中断状态运行，可被再次抢占
            rsp: stack_top as u64,
            ss: 0x10,     // 内核数据段
        };
        unsafe {
            core::ptr::write_volatile(base as *mut SwitchFrame, frame);
        }
        // RSP 指向 r15 槽（最低地址）：存根从 pop r15 开始恢复
        base + R15_OFFSET
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_switch_frame_layout() {
        // 帧大小必须 16 字节对齐（栈对齐要求）且为 22 槽
        assert_eq!(core::mem::size_of::<SwitchFrame>(), 22 * 8);
        assert_eq!(core::mem::size_of::<SwitchFrame>() % 16, 0);
        // r15 在最低地址：RSP 偏移为 0
        assert_eq!(R15_OFFSET, 0);
    }

    #[test]
    fn test_init_stack_returns_r15_slot() {
        // 用 16 对齐的泄漏缓冲区模拟任务栈（宿主测试环境；
        // 真实任务栈由 kmalloc(.., 16) 保证 16 字节对齐）
        #[repr(align(16))]
        struct StackBuf([u8; 4096]);
        let stack: &'static mut StackBuf = Box::leak(Box::new(StackBuf([0; 4096])));
        let top = (stack as *mut StackBuf as usize) + 4096;
        let rsp = SwitchFrame::init_stack(0x1234_5678, top, 42);

        // RSP 指向帧基址（r15 槽在最低地址）：base = top - 176
        assert_eq!(rsp + 176, top);
        assert_eq!(rsp % 16, 0);
        // 帧内容核对：rsp 槽 = 栈顶、rdi = 参数、rip = 入口
        unsafe {
            let f = &*(rsp as *const SwitchFrame);
            assert_eq!(f.r15, 0);
            assert_eq!(f.rdi, 42);
            assert_eq!(f.rip, 0x1234_5678);
            assert_eq!(f.rsp, top as u64);
            assert_eq!(f.rflags, 0x202);
            assert_eq!(f.cs, 0x08);
            assert_eq!(f.ss, 0x10);
        }
    }
}
