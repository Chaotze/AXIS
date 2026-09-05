// ============================================================
// x86_64 端口 I/O
// ============================================================
// x86 平台特有的端口映射 I/O（Port-Mapped I/O）访问原语。
//
// 为什么单独成模块：
// - 串口（16550）、PS/2 控制器、PCI 配置空间等设备都通过
//   IN/OUT 指令访问端口，而端口访问是 x86 平台特有的寄存器
//   操作（RISC 等架构没有端口空间）
// - 按项目分层约定「平台特定的寄存器映射由 arch/ 层完全处理」，
//   drivers 层只调用 inb/outb 等抽象，不直接写内联汇编

use core::arch::asm;

/// 读取一个字节（8 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可读
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nostack, preserves_flags)
        );
    }
    value
}

/// 写入一个字节（8 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可写
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nostack, preserves_flags)
        );
    }
}

/// 读取一个字（16 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可读
#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in ax, dx",
            in("dx") port,
            out("ax") value,
            options(nostack, preserves_flags)
        );
    }
    value
}

/// 写入一个字（16 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可写
#[inline]
pub unsafe fn outw(port: u16, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nostack, preserves_flags)
        );
    }
}

/// 读取一个双字（32 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可读
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nostack, preserves_flags)
        );
    }
    value
}

/// 写入一个双字（32 位端口）
///
/// # Safety
/// 调用方必须保证 port 对应的设备存在且可写
#[inline]
pub unsafe fn outl(port: u16, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nostack, preserves_flags)
        );
    }
}

/// I/O 延迟（等待慢速设备完成端口操作）
///
/// 往 0x80（CMOS/diagnostic 端口）写一个哑字节是最常用的
/// I/O 屏障：IN/OUT 串行化后，随后的端口访问会等到设备就绪。
/// 为什么不用 CPU 空转：空转指令可能被 CPU 乱序越过，而端口
/// 访问本身具有严格顺序语义。
#[inline]
pub fn io_wait() {
    unsafe {
        outb(0x80, 0);
    }
}