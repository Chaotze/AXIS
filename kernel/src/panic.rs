// ============================================================
// AXIS 内核 Panic 处理器
// ============================================================
// 内核 panic 时的处理逻辑，提供详细的错误信息和调试支持

use core::fmt::Write;
use core::panic::PanicInfo;

use axis_kernel::libcore::vga::VgaWriter;

/// Panic 处理器
///
/// no_std 环境必须提供自定义的 panic 处理器。
/// 当内核发生 panic 时，会调用此函数，输出错误信息并停机。
///
/// 为什么要详细输出信息：
/// - 帮助开发者快速定位问题
/// - 记录现场信息用于后续分析
/// - 防止系统在错误状态下继续运行造成数据损坏
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // 禁用中断，防止在 panic 处理中被打断
    unsafe {
        core::arch::asm!("cli");
    }

    // 使用共享的 VGA 写入器直接输出
    // 为什么不用 println!：panic 可能正发生在持锁路径上（如自旋锁内部），
    // 再取一次锁会自死锁；此处中断已禁用，独占式裸写即可可靠输出
    let mut writer = VgaWriter::new();

    // 红色背景标题
    writer.set_color(0x4f); // 红底白字
    let _ = writeln!(writer, "!!! KERNEL PANIC !!!");

    // 恢复正常颜色输出详细信息
    writer.set_color(0x0f); // 黑底亮白

    // 输出 panic 消息
    if let Some(message) = info.message().as_str() {
        let _ = writeln!(writer, "Message: {}", message);
    } else {
        let _ = writeln!(writer, "Message: {}", info.message());
    }

    // 输出 panic 位置
    if let Some(location) = info.location() {
        let _ = writeln!(writer, "Location: {}:{}:{}",
            location.file(), location.line(), location.column());
    }

    let _ = writeln!(writer, "System halted.");

    // 无限循环并停机
    // 为什么用 hlt 而不是空循环：hlt 会让 CPU 进入低功耗状态，减少发热
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}