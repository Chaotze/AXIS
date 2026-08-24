// ============================================================
// 上下文切换
// ============================================================
// 任务/线程上下文保存和切换

pub mod frame;

use frame::TrapFrame;

/// 执行上下文切换
///
/// 从 old 任务切换到 new 任务
///
/// # Safety
/// 调用者必须确保：
/// - old 和 new 指针有效
/// - new 任务的栈和寄存器状态正确
///
/// 实际实现在 switch.asm 中
#[allow(dead_code)]
pub unsafe fn switch(old: *mut TrapFrame, new: *const TrapFrame) {
    // 实际的切换由汇编实现
    // 这里是占位函数
    let _ = (old, new);
}
