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
