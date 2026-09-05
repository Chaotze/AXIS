// ============================================================
// ACPI 数据表定义
// ============================================================
// 定义 ACPI（高级配置与电源接口）规范中的核心数据结构。
//
// 布局说明：ACPI 表在内存中是逐字节紧密排列的（无填充），
// 以下结构全部使用 #[repr(C, packed)]，读取时用 read_unaligned
// 避免对齐陷阱（表基址可能只按 4 字节对齐）。
//
// 字段偏移以 ACPI 6.x 规范为准，注释标注十六进制偏移便于核对。

/// RSDP（Root System Description Pointer）精简版（rev 0/1）
///
/// 偏移：BIOS 在 EBDA 或 0xE0000-0xFFFFF 区域以 16 字节对齐存放，
/// 签名固定为 "RSD PTR "。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    /// 签名："RSD PTR "（8 字节）
    pub signature: [u8; 8],
    /// 校验和（前 20 字节之和模 256 等于 0）
    pub checksum: u8,
    /// OEM 标识
    pub oem_id: [u8; 6],
    /// 修订号：0 = ACPI 1.0（仅 RSDT），2 = ACPI 2.0+（含 XSDT）
    pub revision: u8,
    /// RSDT 物理地址（32 位）
    pub rsdt_address: u32,
}

/// RSDP 扩展版（rev 2 以上才有后续字段）
///
/// 注意：XSDT 地址是 64 位，位于偏移 24；扩展校验和覆盖整个表。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp2 {
    /// 与 rev 0/1 相同的头部（24 字节）
    pub legacy: Rsdp,
    /// 整个 RSDP 的长度（>= 36）
    pub length: u32,
    /// XSDT 物理地址（64 位）
    pub xsdt_address: u64,
    /// 扩展校验和（覆盖整个表）
    pub extended_checksum: u8,
    /// 保留
    pub reserved: [u8; 3],
}

/// ACPI 系统描述表头（System Description Table Header）
///
/// 所有 ACPI 数据表（RSDT/XSDT/FADT/MADT/MCFG...）都以该头部开始。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SdtHeader {
    /// 表签名（4 个 ASCII 字符，如 "FACP"、"APIC"、"MCFG"）
    pub signature: [u8; 4],
    /// 整个表（含头部）的长度
    pub length: u32,
    /// 修订号
    pub revision: u8,
    /// 整个表的校验和（所有字节之和模 256 等于 0）
    pub checksum: u8,
    /// OEM 标识
    pub oem_id: [u8; 6],
    /// OEM 表标识
    pub oem_table_id: [u8; 8],
    /// OEM 修订号
    pub oem_revision: u32,
    /// 创建者 ID
    pub creator_id: u32,
    /// 创建者修订号
    pub creator_revision: u32,
}

impl SdtHeader {
    /// 表签名（4 字节转 u32，便于匹配常量）
    pub const fn signature_u32(&self) -> u32 {
        // 先拷贝到局部数组再转：打包结构体字段未对齐，直接取引用
        // 是未定义行为（E0793）
        let sig = self.signature;
        u32::from_le_bytes(sig)
    }
}

/// RSDT（Root System Description Table）
///
/// 头部之后是一个 u32 数组，每个元素是一个 ACPI 表的物理地址。
/// 仅 ACPI 1.0 使用；ACPI 2.0+ 使用 XSDT。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdt {
    pub header: SdtHeader,
}

/// XSDT（eXtended System Description Table）
///
/// 头部之后是一个 u64 数组，每个元素是一个 ACPI 表的物理地址。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Xsdt {
    pub header: SdtHeader,
}

/// FADT（Fixed ACPI Description Table），签名 "FACP"
///
/// 本结构只保留与电源管理、定时器等常用功能相关的字段；
/// 完整定义见 ACPI 规范第 5.2.9 节。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    pub header: SdtHeader,
    /// 固件控制结构地址（0x24）
    pub firmware_ctrl: u32,
    /// DSDT 地址（0x28）
    pub dsdt: u32,
    /// 保留（0x2C）
    pub _reserved: u8,
    /// 首选电源管理配置（0x2D）
    pub preferred_pm_profile: u8,
    /// SCI 中断号（0x2E）
    pub sci_int: u16,
    /// SMI 命令端口（0x30）
    pub smi_cmd: u32,
    /// ACPI 启用命令值（0x34）
    pub acpi_enable: u8,
    /// ACPI 禁用命令值（0x35）
    pub acpi_disable: u8,
    /// S4BIOS 请求命令值（0x36）
    pub s4bios_req: u8,
    /// P-State 控制命令值（0x37）
    pub pstate_cnt: u8,
    /// PM1a 事件寄存器块基址（0x38）
    pub pm1a_evt_blk: u32,
    /// PM1b 事件寄存器块基址（0x3C）
    pub pm1b_evt_blk: u32,
    /// PM1a 控制寄存器块基址（0x40）
    pub pm1a_cnt_blk: u32,
    /// PM1b 控制寄存器块基址（0x44）
    pub pm1b_cnt_blk: u32,
    /// PM2 控制寄存器块基址（0x48）
    pub pm2_cnt_blk: u32,
    /// PM 定时器寄存器块基址（0x4C）
    pub pm_tmr_blk: u32,
    /// GPE0 寄存器块基址（0x50）
    pub gpe0_blk: u32,
    /// GPE1 寄存器块基址（0x54）
    pub gpe1_blk: u32,
    /// PM1 事件寄存器块长度（0x58）
    pub pm1_evt_len: u8,
    /// PM1 控制寄存器块长度（0x59）
    pub pm1_cnt_len: u8,
    /// PM2 控制寄存器块长度（0x5A）
    pub pm2_cnt_len: u8,
    /// PM 定时器寄存器块长度（0x5B）
    pub pm_tmr_len: u8,
    /// GPE0 寄存器块长度（0x5C）
    pub gpe0_blk_len: u8,
    /// GPE1 寄存器块长度（0x5D）
    pub gpe1_blk_len: u8,
    /// GPE1 基址偏移（0x5E）
    pub gpe1_base: u8,
    /// C 状态控制命令值（0x5F）
    pub cst_cnt: u8,
    /// P-LVL2 延迟（0x60）
    pub p_lvl2_lat: u16,
    /// P-LVL3 延迟（0x62）
    pub p_lvl3_lat: u16,
    /// 刷新大小（0x64）
    pub flush_size: u16,
    /// 刷新策略（0x66）
    pub flush_stride: u16,
    /// Duty 偏移（0x68）
    pub duty_offset: u8,
    /// Duty 宽度（0x69）
    pub duty_width: u8,
    /// Daylight 闹钟命令值（0x6A）
    pub day_alrm: u8,
    /// 月报警命令值（0x6B）
    pub mon_alrm: u8,
    /// 世纪命令值（0x6C）
    pub century: u8,
    /// IAPC 启动架构标志（0x6D）
    pub iapc_boot_arch: u16,
    /// 保留（0x6F）
    pub _reserved2: u8,
    /// 标志位（0x70）
    pub flags: u32,
    /// 重置寄存器（0x74，GAS 结构 12 字节）
    pub reset_reg: [u8; 12],
    /// 重置命令值（0x80）
    pub reset_value: u8,
}

/// GAS（Generic Address Structure）
///
/// 供 ACPI 表内嵌的寄存器地址描述使用（FADT 的 X_PM1A_* 等）。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GenericAddress {
    /// 地址空间 ID：0 = 系统内存，1 = 系统 I/O，... 
    pub address_space: u8,
    /// 位宽
    pub bit_width: u8,
    /// 位偏移
    pub bit_offset: u8,
    /// 访问宽度
    pub access_width: u8,
    /// 64 位地址
    pub address: u64,
}

/// MADT（Multiple APIC Description Table），签名 "APIC"
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Madt {
    pub header: SdtHeader,
    /// Local APIC 物理地址（0x24）
    pub lapic_address: u32,
    /// 标志位（0x28）：bit0 = PCAT 兼容
    pub flags: u32,
    /// 随后是可变长的 APIC 结构体数组（0x2C 起）
    pub entries: [u8; 0],
}

/// MADT 条目头（每个 APIC 结构体前 2 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtEntryHeader {
    /// 条目类型
    pub ty: u8,
    /// 条目长度（含头部）
    pub length: u8,
}

/// MADT 条目类型常量
pub mod madt_type {
    pub const LOCAL_APIC: u8 = 0;
    pub const IO_APIC: u8 = 1;
    pub const INTERRUPT_SOURCE_OVERRIDE: u8 = 2;
    pub const NMI_SOURCE: u8 = 3;
    pub const LOCAL_APIC_NMI: u8 = 4;
    pub const LOCAL_APIC_ADDRESS_OVERRIDE: u8 = 5;
    pub const IO_SAPIC: u8 = 6;
    pub const LOCAL_SAPIC: u8 = 7;
    pub const PLATFORM_INTERRUPT_SOURCE: u8 = 8;
}

/// MADT Local APIC 条目（type 0）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApic {
    pub header: MadtEntryHeader,
    /// ACPI 处理器 ID
    pub acpi_processor_id: u8,
    /// APIC ID
    pub apic_id: u8,
    /// 标志：bit0 = 已启用
    pub flags: u32,
}

/// MADT I/O APIC 条目（type 1）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIoApic {
    pub header: MadtEntryHeader,
    /// I/O APIC ID
    pub io_apic_id: u8,
    /// 保留
    pub _reserved: u8,
    /// I/O APIC 物理地址
    pub address: u32,
    /// 全局中断号基址
    pub global_irq_base: u32,
}

/// MADT 中断源覆盖条目（type 2）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtInterruptOverride {
    pub header: MadtEntryHeader,
    /// 总线号（通常为 0 = ISA）
    pub bus: u8,
    /// 源 IRQ（ISA 中断号）
    pub source: u8,
    /// 全局系统中断号
    pub global_irq: u32,
    /// 标志（触发模式/极性）
    pub flags: u16,
}

/// MCFG（PCI Express Memory Mapped Configuration），签名 "MCFG"
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Mcfg {
    pub header: SdtHeader,
    /// 保留（0x24）
    pub _reserved: u64,
    /// 随后是可变长的 ECAM 段描述符数组（0x2C 起）
    pub allocations: [u8; 0],
}

/// MCFG 中描述的一个 ECAM 段
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct McfgAllocation {
    /// ECAM 基址（物理地址）
    pub base_address: u64,
    /// PCI 段号
    pub pci_segment: u16,
    /// 起始总线号
    pub start_bus: u8,
    /// 结束总线号
    pub end_bus: u8,
    /// 保留
    pub _reserved: u32,
}

/// 常用 ACPI 表签名（转成 u32 便于匹配）
pub mod sig {
    pub const RSDT: u32 = 0x5444_5352; // "RSDT"
    pub const XSDT: u32 = 0x5444_5358; // "XSDT"
    pub const FADT: u32 = 0x5043_4146; // "FACP"
    pub const MADT: u32 = 0x4349_5041; // "APIC"
    pub const MCFG: u32 = 0x4746_434d; // "MCFG"
    pub const DSDT: u32 = 0x5444_5344; // "DSDT"
}