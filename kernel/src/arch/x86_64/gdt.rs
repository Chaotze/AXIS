// ============================================================
// x86_64 GDT (Global Descriptor Table) 管理
// ============================================================
// 设置全局描述符表和任务状态段

use core::arch::asm;
use core::mem;

/// GDT 表项（64位模式）
///
/// 在 x86_64 长模式下，段的大部分功能已经不再使用，
/// 但 GDT 仍然是必需的，原因：
/// - CPU 需要至少一个有效的代码段和数据段
/// - 系统调用机制（syscall/sysret）需要特定的段布局
/// - TSS（任务状态段）需要通过 GDT 加载
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    /// 创建空段（用于第一个表项，CPU 要求必须为 0）
    const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    /// 创建代码段
    ///
    /// 在 64 位模式下，段基址和段界限基本被忽略，
    /// 但 access 字段仍然重要，标识段的类型和权限
    const fn code_segment(dpl: u8) -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            // access: P(1) | DPL(2) | S(1) | E(1) | DC(0) | RW(1) | A(0)
            // P = Present, S = 1(code/data), E = Executable, RW = Readable
            access: 0b1001_1010 | ((dpl & 0b11) << 5),
            // G(1) | D/B(0) | L(1) | AVL(0) | Limit[19:16](0xF)
            // G = Granularity(4KB), L = Long mode
            granularity: 0b1010_1111,
            base_high: 0,
        }
    }

    /// 创建数据段
    const fn data_segment(dpl: u8) -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            // access: P(1) | DPL(2) | S(1) | E(0) | DC(0) | RW(1) | A(0)
            // E = 0 表示数据段
            access: 0b1001_0010 | ((dpl & 0b11) << 5),
            granularity: 0b1100_1111,
            base_high: 0,
        }
    }

    /// 创建 TSS 段描述符（低 64 位）
    ///
    /// TSS 在 x86_64 中的作用：
    /// - 保存特权级切换时的栈指针（用于系统调用、中断）
    /// - 保存 I/O 权限位图基址（如果需要）
    /// - 中断栈表（IST），为特定中断提供独立的栈
    fn tss_segment(tss: &TaskStateSegment) -> Self {
        let ptr = tss as *const _ as u64;
        let limit = (mem::size_of::<TaskStateSegment>() - 1) as u16;

        Self {
            limit_low: limit,
            base_low: (ptr & 0xFFFF) as u16,
            base_middle: ((ptr >> 16) & 0xFF) as u8,
            // access: P(1) | DPL(0) | S(0) | Type(0b1001 = 64-bit TSS available)
            access: 0b1000_1001,
            granularity: ((ptr >> 24) & 0xF) as u8,
            base_high: ((ptr >> 24) & 0xFF) as u8,
        }
    }
}

/// TSS 高 64 位（在 64 位模式下，TSS 描述符占用两个 GDT 表项）
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct TssHigh {
    base_upper: u32,
    reserved: u32,
}

impl TssHigh {
    fn new(tss: &TaskStateSegment) -> Self {
        let ptr = tss as *const _ as u64;
        Self {
            base_upper: (ptr >> 32) as u32,
            reserved: 0,
        }
    }
}

/// 任务状态段（TSS）
///
/// 为什么需要 TSS：
/// 1. 特权级切换：从 Ring 3 进入 Ring 0 时，CPU 自动从 TSS 加载新的栈指针
/// 2. 中断栈表（IST）：为特殊中断（如双重故障）提供独立的栈，防止栈溢出
#[derive(Debug)]
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved_1: u32,
    /// 特权级 0-2 的栈指针（RSP0-RSP2）
    /// 当从低特权级切换到对应特权级时使用
    pub rsp: [u64; 3],
    reserved_2: u64,
    /// 中断栈表（IST1-IST7）
    /// 某些中断可以配置使用 IST 中的独立栈
    pub ist: [u64; 7],
    reserved_3: u64,
    reserved_4: u16,
    /// I/O 权限位图基址
    pub iomap_base: u16,
}

impl TaskStateSegment {
    const fn new() -> Self {
        Self {
            reserved_1: 0,
            rsp: [0; 3],
            reserved_2: 0,
            ist: [0; 7],
            reserved_3: 0,
            reserved_4: 0,
            iomap_base: mem::size_of::<TaskStateSegment>() as u16,
        }
    }
}

/// GDT 表
///
/// 布局：
/// 0: 空段（CPU 要求）
/// 1: 内核代码段
/// 2: 内核数据段
/// 3: 用户代码段
/// 4: 用户数据段
/// 5-6: TSS 段（占用 2 个表项）
static mut GDT: [u64; 7] = [0; 7];
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// GDT 指针（用于 lgdt 指令）
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// 初始化 GDT
///
/// 设置并加载全局描述符表，配置段选择子
pub fn init() {
    unsafe {
        // 构建 GDT 表项
        let tss_ptr = core::ptr::addr_of!(TSS);
        let entries = [
            GdtEntry::null(),                    // 0x00: 空段
            GdtEntry::code_segment(0),           // 0x08: 内核代码段
            GdtEntry::data_segment(0),           // 0x10: 内核数据段
            GdtEntry::data_segment(3),           // 0x18: 用户数据段（syscall 需要在代码段前）
            GdtEntry::code_segment(3),           // 0x20: 用户代码段
            GdtEntry::tss_segment(&*tss_ptr),    // 0x28: TSS 段（低 64 位）
        ];

        // 将表项复制到 GDT（以 u64 格式）
        for (i, entry) in entries.iter().enumerate() {
            GDT[i] = mem::transmute(*entry);
        }

        // TSS 高 64 位
        let tss_high = TssHigh::new(&*tss_ptr);
        GDT[6] = mem::transmute(tss_high);

        // 加载 GDT
        let gdt_addr = core::ptr::addr_of!(GDT) as u64;
        let gdt_ptr = GdtPointer {
            limit: (mem::size_of::<[u64; 7]>() - 1) as u16,
            base: gdt_addr,
        };

        asm!("lgdt [{}]", in(reg) &gdt_ptr, options(nostack, preserves_flags));

        // 重新加载段寄存器
        // 为什么需要重新加载：lgdt 只更新了 GDTR，但段选择子还是旧的
        reload_segments();

        // 加载 TSS
        // TSS 选择子 = 0x28（第 5 个表项，RPL=0）
        asm!("ltr ax", in("ax") 0x28u16, options(nostack, preserves_flags));

        println!("[GDT] Global Descriptor Table initialized");
    }
}

/// 重新加载段寄存器
///
/// 为什么需要这个函数：
/// - lgdt 只更新了 GDTR 寄存器，段选择子缓存还是旧的
/// - 必须通过 far jmp/ret 重新加载 CS
/// - 其他段寄存器需要显式重新加载
unsafe fn reload_segments() {
    unsafe {
        // 重新加载 CS：通过 far return
        // 我们压入新的 CS 和返回地址，然后 retfq
        asm!(
            "push {seg}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            seg = in(reg) 0x08u64,  // 内核代码段选择子
            tmp = lateout(reg) _,
            options(preserves_flags)
        );

        // 重新加载数据段寄存器
        // 在 64 位模式下，DS/ES/SS 的基址和界限被忽略，但选择子仍需正确
        asm!(
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            in("ax") 0x10u16,  // 内核数据段选择子
            options(nostack, preserves_flags)
        );

        // FS 和 GS 用于线程局部存储，暂时置零
        asm!(
            "mov fs, ax",
            "mov gs, ax",
            in("ax") 0u16,
            options(nostack, preserves_flags)
        );
    }
}

/// 设置内核栈（用于中断处理）
///
/// 当从用户态进入内核态时，CPU 会从 TSS.RSP0 加载新的栈指针
#[allow(dead_code)]
pub fn set_kernel_stack(stack_top: u64) {
    unsafe {
        TSS.rsp[0] = stack_top;
    }
}

/// 设置中断栈（IST）
///
/// 为特定中断提供独立的栈，防止栈溢出导致的问题
#[allow(dead_code)]
pub fn set_interrupt_stack(ist_index: usize, stack_top: u64) {
    assert!(ist_index > 0 && ist_index <= 7, "IST index must be 1-7");
    unsafe {
        TSS.ist[ist_index - 1] = stack_top;
    }
}
