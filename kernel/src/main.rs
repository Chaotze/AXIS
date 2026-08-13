// ============================================================
// AXIS 内核主入口
// ============================================================

#![no_std]
#![no_main]

mod panic;

use core::arch::asm;

/// 内核入口点
///
/// 由引导加载程序跳转到此函数
/// 按照 Multiboot2 协议：
///   - EAX = 0x36d76289（Multiboot2 魔数）
///   - EBX = 引导信息结构地址
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // 验证 Multiboot2 魔数
    let magic: u32;
    unsafe {
        asm!("mov {0:e}, eax", out(reg) magic);
    }

    if magic != 0x36d76289 {
        // 魔数不匹配，挂起
        loop {}
    }

    // 使用 VGA 文本模式输出欢迎信息
    print_string(b"AXIS: AXIS eXecute Instructions Steadily\n");
    print_string(b"Kernel loaded successfully!\n");

    // 挂起（暂时没有其他功能）
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

/// VGA 文本模式输出
///
/// 简单的字符串打印函数，用于验证内核启动
fn print_string(s: &[u8]) {
    const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;
    static mut COLUMN: usize = 0;
    static mut ROW: usize = 0;
    const WIDTH: usize = 80;
    const HEIGHT: usize = 25;

    unsafe {
        for &byte in s {
            match byte {
                b'\n' => {
                    COLUMN = 0;
                    ROW += 1;
                    if ROW >= HEIGHT {
                        ROW = HEIGHT - 1;
                    }
                }
                byte => {
                    if COLUMN >= WIDTH {
                        COLUMN = 0;
                        ROW += 1;
                        if ROW >= HEIGHT {
                            ROW = HEIGHT - 1;
                        }
                    }

                    let pos = ROW * WIDTH + COLUMN;
                    let color = 0x0f00; // 白色文本，黑色背景
                    VGA_BUFFER.add(pos).write_volatile(color | byte as u16);
                    COLUMN += 1;
                }
            }
        }
    }
}
