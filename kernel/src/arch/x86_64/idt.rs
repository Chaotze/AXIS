// ============================================================
// x86_64 IDT (Interrupt Descriptor Table) 管理
// ============================================================
// 设置中断描述符表，处理异常和中断

use core::arch::asm;
use core::mem;
use crate::config::IDT_ENTRIES;

/// IDT 表项（中断门）
///
/// x86_64 中断门格式（128 位）：
/// - bits 0-15: 偏移低 16 位
/// - bits 16-31: 段选择子
/// - bits 32-39: IST（中断栈表索引，0 表示不使用）
/// - bits 40-47: 类型和属性
/// - bits 48-63: 偏移中 16 位
/// - bits 64-95: 偏移高 32 位
/// - bits 96-127: 保留
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// 创建空表项
    const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// 创建中断门
    ///
    /// 中断门 vs 陷阱门：
    /// - 中断门：进入时自动清除 IF（禁用中断），用于硬件中断
    /// - 陷阱门：保持 IF 不变，用于异常和系统调用
    ///
    /// 为什么需要指定 IST：
    /// - 某些异常（如双重故障、NMI）需要独立的栈
    /// - 防止栈溢出时无法处理异常
    fn new(handler: u64, selector: u16, ist: u8, dpl: u8, is_trap: bool) -> Self {
        let type_bits = if is_trap { 0xF } else { 0xE }; // 陷阱门 = 0xF, 中断门 = 0xE
        let type_attr = 0x80 | ((dpl & 0x3) << 5) | type_bits; // P=1, DPL, Type

        Self {
            offset_low: (handler & 0xFFFF) as u16,
            selector,
            ist,
            type_attr,
            offset_mid: ((handler >> 16) & 0xFFFF) as u16,
            offset_high: ((handler >> 32) & 0xFFFF_FFFF) as u32,
            reserved: 0,
        }
    }
}

/// IDT 表
static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::null(); IDT_ENTRIES];

/// IDT 指针（用于 lidt 指令）
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

/// 中断处理函数类型
///
/// x86_64 中断帧结构（CPU 自动压栈）：
/// - RIP（返回地址）
/// - CS（代码段）
/// - RFLAGS（标志寄存器）
/// - RSP（栈指针，特权级切换时）
/// - SS（栈段，特权级切换时）
#[repr(C)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// 带错误码的中断帧
#[repr(C)]
pub struct InterruptStackFrameWithError {
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// 初始化 IDT
///
/// 设置所有中断和异常的处理程序
pub fn init() {
    unsafe {
        // CPU 异常（0-31）
        set_handler(0, exception_0_handler as *const () as u64, 0, false);   // #DE 除零错误
        set_handler(1, exception_1_handler as *const () as u64, 0, false);   // #DB 调试
        set_handler(2, exception_2_handler as *const () as u64, 1, false);   // NMI（使用 IST1）
        set_handler(3, exception_3_handler as *const () as u64, 0, true);    // #BP 断点（陷阱门）
        set_handler(4, exception_4_handler as *const () as u64, 0, true);    // #OF 溢出
        set_handler(5, exception_5_handler as *const () as u64, 0, false);   // #BR 越界
        set_handler(6, exception_6_handler as *const () as u64, 0, false);   // #UD 无效操作码
        set_handler(7, exception_7_handler as *const () as u64, 0, false);   // #NM 设备不可用
        set_handler(8, exception_8_handler as *const () as u64, 2, false);   // #DF 双重故障（使用 IST2）
        set_handler(10, exception_10_handler as *const () as u64, 0, false); // #TS 无效 TSS
        set_handler(11, exception_11_handler as *const () as u64, 0, false); // #NP 段不存在
        set_handler(12, exception_12_handler as *const () as u64, 0, false); // #SS 栈段错误
        set_handler(13, exception_13_handler as *const () as u64, 0, false); // #GP 一般保护错误
        set_handler(14, exception_14_handler as *const () as u64, 0, false); // #PF 页错误
        set_handler(16, exception_16_handler as *const () as u64, 0, false); // #MF x87 浮点异常
        set_handler(17, exception_17_handler as *const () as u64, 0, false); // #AC 对齐检查
        set_handler(18, exception_18_handler as *const () as u64, 3, false); // #MC 机器检查（使用 IST3）
        set_handler(19, exception_19_handler as *const () as u64, 0, false); // #XM SIMD 浮点异常
        set_handler(20, exception_20_handler as *const () as u64, 0, false); // #VE 虚拟化异常

        // 硬件中断（32-255）
        // 由中断控制器模块设置具体的处理程序

        // 加载 IDT
        let idt_addr = core::ptr::addr_of!(IDT) as u64;
        let idt_ptr = IdtPointer {
            limit: (mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: idt_addr,
        };

        asm!("lidt [{}]", in(reg) &idt_ptr, options(nostack, preserves_flags));

        println!("[IDT] Interrupt Descriptor Table initialized");
    }
}

/// 设置中断处理程序
///
/// # 参数
/// - `index`: 中断向量号
/// - `handler`: 处理函数地址
/// - `ist`: 中断栈表索引（0 表示不使用）
/// - `is_trap`: 是否为陷阱门
unsafe fn set_handler(index: usize, handler: u64, ist: u8, is_trap: bool) {
    assert!(index < IDT_ENTRIES, "IDT index out of range");

    // 内核代码段选择子 = 0x08
    let selector = 0x08;
    // DPL = 0（内核级）
    let dpl = 0;

    unsafe {
        IDT[index] = IdtEntry::new(handler, selector, ist, dpl, is_trap);
    }
}

/// 设置用户可调用的中断门（如系统调用）
#[allow(dead_code)]
pub unsafe fn set_user_handler(index: usize, handler: u64) {
    assert!(index < IDT_ENTRIES, "IDT index out of range");

    let selector = 0x08;
    let dpl = 3; // DPL = 3（用户级）
    let ist = 0;

    unsafe {
        IDT[index] = IdtEntry::new(handler, selector, ist, dpl, true);
    }
}


// ============================================================
// 异常处理程序入口
// ============================================================
// 这些是汇编存根，保存寄存器后调用 Rust 处理函数

unsafe extern "C" {
    // 异常处理程序（由 entry.asm 提供）
    fn exception_0_handler();
    fn exception_1_handler();
    fn exception_2_handler();
    fn exception_3_handler();
    fn exception_4_handler();
    fn exception_5_handler();
    fn exception_6_handler();
    fn exception_7_handler();
    fn exception_8_handler();
    fn exception_10_handler();
    fn exception_11_handler();
    fn exception_12_handler();
    fn exception_13_handler();
    fn exception_14_handler();
    fn exception_16_handler();
    fn exception_17_handler();
    fn exception_18_handler();
    fn exception_19_handler();
    fn exception_20_handler();
}

/// 公开的处理函数（供汇编调用）
///
/// 为什么需要汇编存根：
/// - CPU 自动压栈的内容有限（只有 RIP、CS、RFLAGS 等）
/// - 需要手动保存通用寄存器
/// - 需要对齐栈（x86_64 ABI 要求 16 字节对齐）
/// - 需要处理错误码（某些异常有，某些没有）

#[unsafe(no_mangle)]
pub extern "C" fn handle_exception(vector: u64, error_code: u64, frame: &InterruptStackFrame) {
    // 调用具体的异常处理函数
    super::interrupt::handle_exception(vector as usize, error_code, frame);
}
