// ============================================================
// I/O APIC 管理
// ============================================================
// 高级可编程中断控制器（I/O 部分）

/// I/O APIC 基地址（默认）
const IOAPIC_BASE: u64 = 0xFEC00000;

/// I/O APIC 寄存器
#[allow(dead_code)]
mod reg {
    pub const IOREGSEL: u32 = 0x00;  // I/O Register Select
    pub const IOWIN: u32 = 0x10;     // I/O Window (data)
    pub const ID: u8 = 0x00;         // I/O APIC ID
    pub const VERSION: u8 = 0x01;    // I/O APIC Version
    pub const REDTBL_BASE: u8 = 0x10; // Redirection Table 基址
}

/// 读 I/O APIC 寄存器
///
/// 两步读取：
/// 1. 写寄存器号到 IOREGSEL
/// 2. 从 IOWIN 读取数据
///
/// # Safety
/// 必须确保 I/O APIC 已映射
#[inline]
unsafe fn read_reg(reg: u8) -> u32 {
    unsafe {
        let ioregsel = IOAPIC_BASE as *mut u32;
        let iowin = (IOAPIC_BASE + 0x10) as *const u32;

        core::ptr::write_volatile(ioregsel, reg as u32);
        core::ptr::read_volatile(iowin)
    }
}

/// 写 I/O APIC 寄存器
///
/// # Safety
/// 必须确保 I/O APIC 已映射
#[inline]
unsafe fn write_reg(reg: u8, value: u32) {
    unsafe {
        let ioregsel = IOAPIC_BASE as *mut u32;
        let iowin = (IOAPIC_BASE + 0x10) as *mut u32;

        core::ptr::write_volatile(ioregsel, reg as u32);
        core::ptr::write_volatile(iowin, value);
    }
}

/// 初始化 I/O APIC
///
/// I/O APIC 负责将外部中断（如键盘、鼠标、磁盘）路由到 CPU
pub unsafe fn init() {
    unsafe {
        // 读取版本信息（包含最大重定向表项数）
        let version = read_reg(reg::VERSION);
        let max_entry = ((version >> 16) & 0xFF) as usize;

        println!("[IOAPIC] I/O APIC has {} redirection entries", max_entry + 1);

        // 暂时屏蔽所有中断
        for i in 0..=max_entry {
            set_entry(i, 0, 0x10000); // bit 16 = 屏蔽
        }

        println!("[IOAPIC] I/O APIC initialized");
    }
}

/// 设置重定向表项
///
/// 配置某个 IRQ 如何路由到 CPU
///
/// # 参数
/// - `entry`: IRQ 号
/// - `vector`: 中断向量号
/// - `flags`: 标志位（包含触发模式、极性等）
#[allow(dead_code)]
pub unsafe fn set_entry(entry: usize, vector: u8, flags: u32) {
    let low_reg = reg::REDTBL_BASE + (entry * 2) as u8;
    let high_reg = low_reg + 1;

    unsafe {
        // 低 32 位：向量号 + 标志
        write_reg(low_reg, vector as u32 | flags);

        // 高 32 位：目标 APIC ID（默认 0）
        write_reg(high_reg, 0);
    }
}
