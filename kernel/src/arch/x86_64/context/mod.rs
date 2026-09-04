// ============================================================
// 上下文切换
// ============================================================
// 任务/线程上下文保存和切换。
//
// 两种切换机制并存（职责不同）：
// 1. 协作式直接切换：本模块的 switch()，走 switch.asm 的
//    context_switch，直接保存/恢复两个 TrapFrame 的寄存器；
//    供任务主动 yield/阻塞换入（WaitQueue 接线后用）
// 2. 抢占式切换：定时器中断内换栈 + iretq（见 interrupt/
//    entry.asm 与 task::tick_hook），切换点在中断存根中，
//    本模块的 frame::SwitchFrame 为其提供首次运行帧布局
//
// 为什么主用抢占式切换机制：
// - CFS 的抢占发生在 tick 中断上下文，iretq 式切换
//    "保存中断现场 = 保存任务现场"，零额外存根开销；
//    协作式切换机制保留给不经过中断的主动让出路径

pub mod frame;

use frame::TrapFrame;

// switch.asm 提供的汇编实现（TrapFrame 布局，见 frame.rs）
unsafe extern "C" {
    fn context_switch(old: *mut TrapFrame, new: *const TrapFrame);
}

/// 执行上下文切换：保存 old 现场、恢复 new 现场
///
/// # Safety
/// 调用者必须确保：
/// - old 指向当前任务的 TrapFrame（切换后其内容被覆盖保存）
/// - new 指向目标任务的 TrapFrame（其 rsp/rip 必须有效）
/// - 调用期间中断已屏蔽（切换过程不可重入）
#[allow(dead_code)]
pub unsafe fn switch(old: *mut TrapFrame, new: *const TrapFrame) {
    unsafe {
        context_switch(old, new);
    }
}
