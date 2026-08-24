// ============================================================
// x86_64 中断处理
// ============================================================
// 中断和异常的高层处理逻辑

pub mod apic;
pub mod handler;
pub mod timer;
pub mod ioapic;
pub mod msi;

use crate::arch::x86_64::idt::InterruptStackFrame;

/// 初始化中断系统
///
/// 初始化顺序：
/// 1. 禁用 PIC（8259A）- 使用 APIC 代替
/// 2. 映射 Local APIC MMIO 区域
/// 3. 初始化 Local APIC
/// 4. 初始化 I/O APIC
/// 5. 初始化定时器
/// 6. 启用中断
pub fn init() {
    // 禁用传统的 8259A PIC
    // 为什么要禁用：APIC 是更现代的中断控制器，性能更好
    unsafe {
        disable_pic();
    }

    // 映射 APIC 设备的 MMIO 区域（设备寄存器）
    // 必须在访问 APIC 寄存器之前完成
    unsafe {
        map_mmio_identity(0xFEC0_0000); // I/O APIC
        map_mmio_identity(0xFEE0_0000); // Local APIC
    }

    // 初始化 APIC
    unsafe {
        apic::init();
        ioapic::init();
    }

    // 初始化定时器
    timer::init();

    // 启用中断
    // 为什么现在才启用：前面的初始化必须在中断禁用状态下完成
    unsafe {
        core::arch::asm!("sti", options(nostack, preserves_flags));
    }

    println!("[INTERRUPT] Interrupt system initialized");
}

/// 早期页表帧（4KB 对齐的页表缓冲区）
///
/// 作为过渡方案，早期 MMIO 标识映射需要物理页帧作为中间页表，
/// 在虚拟内存管理（阶段 3）接管页表前从内核 .bss 中借用。
#[repr(align(4096))]
#[allow(dead_code)]
struct AlignedPage([u64; 512]);

static mut EARLY_PD: AlignedPage = AlignedPage([0; 512]);

/// 早期 MMIO 标识映射（Identity Mapping）
///
/// 当前内核运行在引导器提供的低 2GB 标识映射页表上（虚拟地址=物理地址），
/// 尚未建立 config 中规划的高半核映射（PHYSICAL_MEMORY_OFFSET 暂未生效），
/// 因此 `PhysAddr::to_virt()` / `PageTableMapper` 等依赖高半核映射的接口
/// 暂时不可用。在访问 Local APIC / I/O APIC 等 MMIO 设备寄存器前，
/// 需要直接在其物理地址处建立 2MB 巨页标识映射。
///
/// 注意：这是过渡方案，待虚拟内存管理（阶段 3）接管页表后应移除，
/// 改用统一的 MMIO 映射机制。
///
/// # Safety
/// 仅应在早期引导阶段、页表所有权尚未移交时调用一次。
/// `phys` 必须是 2MB 对齐的 MMIO 基地址。
unsafe fn map_mmio_identity(phys: u64) {
    use core::arch::asm;

    // 页表项标志：P=1, RW=1, PCD=1（禁用缓存，MMIO 不可缓存）, PS=1（2MB 巨页）
    const HUGE_PAGE_FLAGS: u64 = 0x1 | 0x2 | 0x10 | 0x80;

    // 计算页表索引
    let p3_index = ((phys >> 30) & 0x1FF) as usize;
    let p2_index = ((phys >> 21) & 0x1FF) as usize;
    assert_eq!(phys & 0x1F_FFFF, 0, "MMIO 基地址必须 2MB 对齐");

    // 读取当前 PML4（引导器页表，物理=虚拟）
    let cr3: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
    }

    // 当前页表使用 PML4[0] -> PDPT（引导器已建立）
    let pml4_entry = unsafe { (cr3 as *const u64).read_volatile() };
    assert!(pml4_entry & 1 != 0, "PML4[0] 不存在，页表结构异常");
    let pdpt_phys = pml4_entry & 0x000F_FFFF_FFFF_F000;

    // PDPT[p3_index]：不存在则分配一个新的 PD 页表帧
    let pdpt_entry = unsafe { (pdpt_phys as *const u64).add(p3_index).read_volatile() };
    let pd_phys = if pdpt_entry & 1 != 0 {
        pdpt_entry & 0x000F_FFFF_FFFF_F000
    } else {
        // 从内核 .bss 中借用一页作为 PD（内核当前按标识映射链接，
        // 静态变量的虚拟地址即物理地址）。
        // 注意：必须 4KB 对齐！CPU 会按页对齐掩码解释页表项中的地址位，
        // 未对齐的帧会导致 CPU 读取错误的页表偏移。
        let frame = core::ptr::addr_of_mut!(EARLY_PD) as u64;
        assert_eq!(frame & 0xFFF, 0, "EARLY_PD 必须页对齐");
        unsafe {
            core::ptr::write_bytes(frame as *mut u8, 0, core::mem::size_of::<AlignedPage>());
            (pdpt_phys as *mut u64).add(p3_index).write_volatile(frame | 0x3); // P | RW
        }
        frame
    };

    // PD[p2_index]：建立 2MB 巨页标识映射（若尚未建立）
    let pd_entry = unsafe { (pd_phys as *const u64).add(p2_index).read_volatile() };
    if pd_entry & 1 == 0 {
        unsafe {
            (pd_phys as *mut u64).add(p2_index).write_volatile(phys | HUGE_PAGE_FLAGS);
        }
    }

    // 刷新 TLB，使新映射立即生效
    unsafe {
        asm!("invlpg [{}]", in(reg) phys, options(nostack, preserves_flags));
    }
}

/// 禁用 8259A PIC
///
/// 为什么需要禁用：
/// - 现代系统使用 APIC，不需要 PIC
/// - PIC 和 APIC 同时启用会造成冲突
/// - PIC 的中断向量范围与 CPU 异常重叠
///
/// 禁用方式：
/// - 将所有 IRQ 屏蔽掉（写入 0xFF 到 IMR）
unsafe fn disable_pic() {
    use core::arch::asm;

    unsafe {
        // PIC1（主片）IMR = 0x21
        // PIC2（从片）IMR = 0xA1
        // 写入 0xFF 屏蔽所有中断

        // 主片
        asm!(
            "mov al, 0xFF",
            "out 0x21, al",
            options(nostack, preserves_flags)
        );

        // 从片
        asm!(
            "mov al, 0xFF",
            "out 0xA1, al",
            options(nostack, preserves_flags)
        );
    }
}

/// 处理异常
///
/// 从汇编存根调用，统一处理所有 CPU 异常
pub fn handle_exception(vector: usize, error_code: u64, frame: &InterruptStackFrame) {
    handler::handle_exception(vector, error_code, frame);
}

/// 处理硬件中断
///
/// 从汇编存根调用，统一处理所有硬件中断
#[unsafe(no_mangle)]
pub extern "C" fn handle_irq(vector: u64, frame: &InterruptStackFrame) {
    handler::handle_irq(vector as usize, frame);
}
