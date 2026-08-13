// ============================================================
// Panic 处理器
// ============================================================
// 内核 panic 时的处理逻辑

use core::panic::PanicInfo;

/// Panic 处理器
///
/// no_std 环境必须提供自定义的 panic 处理器
/// 当内核发生 panic 时，会调用此函数
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // 简单处理：打印 PANIC 并挂起
    // 实际实现应该输出更详细的调试信息

    // 使用 VGA 文本模式输出（简化版本）
    const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;

    unsafe {
        // 在屏幕顶部显示 "KERNEL PANIC"
        let panic_msg = b"KERNEL PANIC";
        let color = 0x4f00; // 红色背景，白色文本

        for (i, &byte) in panic_msg.iter().enumerate() {
            VGA_BUFFER.add(i).write_volatile(color | byte as u16);
        }
    }

    // 无限循环并停机
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
