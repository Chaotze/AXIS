// ============================================================
// ACPI 表解析器
// ============================================================
// 把内存中的 ACPI 字节流解析成语义结构。
//
// 纯逻辑设计：所有解析函数都接收 &[u8] 字节片（表内容），
// 不直接访问物理内存——发现表（RSDP/扫描）属于装配层（mod.rs），
// 这里只负责"给定字节，解出结构"，因此可以在宿主环境单元测试。
//
// 读取约定：ACPI 表是紧密排列的字节流，用 read_unaligned 读取
// 任意对齐的字段；x86 上 ACPI 字段一律小端，解析与宿主无关。

use alloc::vec::Vec;

use super::tables::*;

/// 校验 ACPI 表校验和：所有字节之和模 256 应为 0
///
/// 为什么单独成函数：RSDP 头校验、扩展校验和、各 SDT 表校验
/// 都是同一算法，集中实现避免重复。
pub fn checksum_valid(data: &[u8]) -> bool {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0
}

/// 读取表头签名（4 字节 → u32）
pub fn header_signature(data: &[u8]) -> Option<u32> {
    if data.len() < core::mem::size_of::<SdtHeader>() {
        return None;
    }
    Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

/// 读取表长度字段（头偏移 4）
pub fn table_length(data: &[u8]) -> Option<u32> {
    if data.len() < 8 {
        return None;
    }
    Some(u32::from_le_bytes([data[4], data[5], data[6], data[7]]))
}

/// 校验一个 SDT 表的基本合法性：签名匹配、长度不超过数据、校验和通过
pub fn valid_sdt(data: &[u8], expect_signature: u32) -> bool {
    if header_signature(data) != Some(expect_signature) {
        return false;
    }
    let Some(len) = table_length(data) else { return false };
    if (len as usize) > data.len() || (len as usize) < core::mem::size_of::<SdtHeader>() {
        return false;
    }
    checksum_valid(&data[..len as usize])
}

/// 解析 RSDT 的表地址数组（ACPI 1.0，u32 条目）
///
/// 头部之后是若干 32 位物理地址；返回 Vec<u32> 物理地址表。
pub fn rsdt_entries(data: &[u8]) -> Option<Vec<u32>> {
    if !valid_sdt(data, sig::RSDT) {
        return None;
    }
    let len = table_length(data)? as usize;
    let mut entries = Vec::new();
    let mut off = core::mem::size_of::<SdtHeader>();
    while off + 4 <= len {
        let addr = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        entries.push(addr);
        off += 4;
    }
    Some(entries)
}

/// 解析 XSDT 的表地址数组（ACPI 2.0+，u64 条目）
pub fn xsdt_entries(data: &[u8]) -> Option<Vec<u64>> {
    if !valid_sdt(data, sig::XSDT) {
        return None;
    }
    let len = table_length(data)? as usize;
    let mut entries = Vec::new();
    let mut off = core::mem::size_of::<SdtHeader>();
    while off + 8 <= len {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[off..off + 8]);
        entries.push(u64::from_le_bytes(bytes));
        off += 8;
    }
    Some(entries)
}

/// MADT 解析结果
#[derive(Debug, Clone, Default)]
pub struct MadtInfo {
    /// Local APIC 物理地址
    pub lapic_address: u32,
    /// MADT 标志位
    pub flags: u32,
    /// 启用的 Local APIC 处理器数
    pub lapic_count: usize,
    /// I/O APIC 列表
    pub io_apics: Vec<MadtIoApic>,
    /// 中断源覆盖列表
    pub overrides: Vec<MadtInterruptOverride>,
}

/// 解析 MADT（签名 "APIC"）
///
/// 遍历条目数组，按类型收集 I/O APIC 与中断覆盖；Local APIC
/// 只统计数量（处理器是否启用由条目 flags 决定）。
pub fn parse_madt(data: &[u8]) -> Option<MadtInfo> {
    if !valid_sdt(data, sig::MADT) {
        return None;
    }
    let len = table_length(data)? as usize;
    if len < core::mem::size_of::<Madt>() {
        return None;
    }

    let mut info = MadtInfo {
        lapic_address: u32::from_le_bytes(data[36..40].try_into().ok()?),
        flags: u32::from_le_bytes(data[40..44].try_into().ok()?),
        ..Default::default()
    };

    let mut off = 44; // 头部后第一个条目
    while off + 2 <= len {
        let ty = data[off];
        let entry_len = data[off + 1] as usize;
        if entry_len < 2 || off + entry_len > len {
            break; // 数据异常，停止遍历
        }
        let entry = &data[off..off + entry_len];
        match ty {
            madt_type::LOCAL_APIC => {
                if entry_len >= 8 {
                    let flags =
                        u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
                    if flags & 1 != 0 {
                        info.lapic_count += 1;
                    }
                }
            }
            madt_type::IO_APIC => {
                if entry_len >= 12 {
                    info.io_apics.push(MadtIoApic {
                        header: MadtEntryHeader { ty, length: entry_len as u8 },
                        io_apic_id: entry[2],
                        _reserved: entry[3],
                        address: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
                        global_irq_base: u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]),
                    });
                }
            }
            madt_type::INTERRUPT_SOURCE_OVERRIDE => {
                if entry_len >= 10 {
                    info.overrides.push(MadtInterruptOverride {
                        header: MadtEntryHeader { ty, length: entry_len as u8 },
                        bus: entry[2],
                        source: entry[3],
                        global_irq: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
                        flags: u16::from_le_bytes([entry[8], entry[9]]),
                    });
                }
            }
            _ => {
                // 未知条目：跳过（长度字段始终有效）
            }
        }
        off += entry_len;
    }
    Some(info)
}

/// MCFG 解析结果
#[derive(Debug, Clone, Default)]
pub struct McfgInfo {
    /// ECAM 段描述符列表
    pub allocations: Vec<McfgAllocation>,
}

/// 解析 MCFG（签名 "MCFG"）
///
/// 头部（0x24 处保留 8 字节）之后是 16 字节的段描述符数组。
pub fn parse_mcfg(data: &[u8]) -> Option<McfgInfo> {
    if !valid_sdt(data, sig::MCFG) {
        return None;
    }
    let len = table_length(data)? as usize;
    if len < 44 {
        return None;
    }
    let mut info = McfgInfo::default();
    let mut off = 44;
    while off + 16 <= len {
        let base = u64::from_le_bytes(data[off..off + 8].try_into().ok()?);
        let seg = u16::from_le_bytes([data[off + 8], data[off + 9]]);
        info.allocations.push(McfgAllocation {
            base_address: base,
            pci_segment: seg,
            start_bus: data[off + 10],
            end_bus: data[off + 11],
            _reserved: u32::from_le_bytes([data[off + 12], data[off + 13], data[off + 14], data[off + 15]]),
        });
        off += 16;
    }
    Some(info)
}

/// FADT 解析结果（只保留常用字段）
#[derive(Debug, Clone, Default)]
pub struct FadtInfo {
    /// SCI 中断号
    pub sci_int: u16,
    /// SMI 命令端口
    pub smi_cmd: u32,
    /// ACPI 启用命令值
    pub acpi_enable: u8,
    /// ACPI 禁用命令值
    pub acpi_disable: u8,
    /// PM1a 事件块基址
    pub pm1a_evt_blk: u32,
    /// PM 定时器块基址
    pub pm_tmr_blk: u32,
    /// PM 定时器块长度（秒以下）
    pub pm_tmr_len: u8,
    /// FADT 标志
    pub flags: u32,
}

/// 解析 FADT（签名 "FACP"）
pub fn parse_fadt(data: &[u8]) -> Option<FadtInfo> {
    if !valid_sdt(data, sig::FADT) {
        return None;
    }
    let len = table_length(data)? as usize;
    let mut info = FadtInfo::default();

    // 仅当表足够长时读取对应字段（ACPI 1.0 表较短）
    let get16 = |off: usize| -> Option<u16> {
        (off + 2 <= len).then(|| u16::from_le_bytes([data[off], data[off + 1]]))
    };
    let get32 = |off: usize| -> Option<u32> {
        (off + 4 <= len).then(|| u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]))
    };

    info.sci_int = get16(0x2E).unwrap_or(0);
    info.smi_cmd = get32(0x30).unwrap_or(0);
    if len > 0x34 { info.acpi_enable = data[0x34]; }
    if len > 0x35 { info.acpi_disable = data[0x35]; }
    info.pm1a_evt_blk = get32(0x38).unwrap_or(0);
    info.pm_tmr_blk = get32(0x4C).unwrap_or(0);
    if len > 0x5B { info.pm_tmr_len = data[0x5B]; }
    info.flags = get32(0x70).unwrap_or(0);
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个带合法校验和的 SDT 表头字节
    fn make_header(signature: &[u8; 4], length: u32) -> Vec<u8> {
        let mut h = vec![0u8; 36];
        h[0..4].copy_from_slice(signature);
        h[4..8].copy_from_slice(&length.to_le_bytes());
        h[8] = 1; // revision
        h[9] = 0; // checksum，先占位
        h
    }

    /// 补齐校验和
    fn fix_checksum(data: &mut [u8]) {
        let sum: u8 = data.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        data[9] = data[9].wrapping_sub(sum);
    }

    #[test]
    fn test_checksum_valid() {
        let mut data = make_header(b"RSDT", 44);
        data.extend_from_slice(&[0u8; 8]); // 两个 4 字节指针
        fix_checksum(&mut data);
        assert!(checksum_valid(&data));
        data[0] ^= 0xFF;
        assert!(!checksum_valid(&data));
    }

    #[test]
    fn test_rsdt_entries() {
        let mut data = make_header(b"RSDT", 44);
        data.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        data.extend_from_slice(&0x9ABC_DEF0u32.to_le_bytes());
        fix_checksum(&mut data);
        let entries = rsdt_entries(&data).unwrap();
        assert_eq!(entries, vec![0x1234_5678, 0x9ABC_DEF0]);
    }

    #[test]
    fn test_xsdt_entries() {
        let mut data = make_header(b"XSDT", 52);
        data.extend_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        data.extend_from_slice(&0x8877_6655_4433_2211u64.to_le_bytes());
        fix_checksum(&mut data);
        let entries = xsdt_entries(&data).unwrap();
        assert_eq!(entries, vec![0x1122_3344_5566_7788, 0x8877_6655_4433_2211]);
    }

    #[test]
    fn test_parse_madt() {
        // 头部 36 + lapic_addr(4) + flags(4) + 两个条目
        let mut data = make_header(b"APIC", 36 + 4 + 4 + 12 + 12);
        data.extend_from_slice(&0xFEE0_0000u32.to_le_bytes()); // lapic
        data.extend_from_slice(&1u32.to_le_bytes());          // flags
        // Local APIC 条目（启用）
        data.push(madt_type::LOCAL_APIC);
        data.push(8);
        data.push(0); // acpi processor id
        data.push(0); // apic id
        data.extend_from_slice(&1u32.to_le_bytes()); // 启用
        // I/O APIC 条目
        data.push(madt_type::IO_APIC);
        data.push(12);
        data.push(1); // io apic id
        data.push(0);
        data.extend_from_slice(&0xFEC0_0000u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // gsi base
        fix_checksum(&mut data);

        let info = parse_madt(&data).unwrap();
        assert_eq!(info.lapic_address, 0xFEE0_0000);
        assert_eq!(info.lapic_count, 1);
        assert_eq!(info.io_apics.len(), 1);
        assert_eq!(info.io_apics[0].address, 0xFEC0_0000);
    }

    #[test]
    fn test_parse_mcfg() {
        let mut data = make_header(b"MCFG", 44 + 16);
        data.extend_from_slice(&0u64.to_le_bytes()); // reserved
        data.extend_from_slice(&0xE000_0000u64.to_le_bytes()); // base
        data.extend_from_slice(&0u16.to_le_bytes()); // segment
        data.push(0); // start bus
        data.push(255); // end bus
        data.extend_from_slice(&0u32.to_le_bytes()); // reserved
        fix_checksum(&mut data);

        let info = parse_mcfg(&data).unwrap();
        assert_eq!(info.allocations.len(), 1);
        assert_eq!(info.allocations[0].base_address, 0xE000_0000);
        assert_eq!(info.allocations[0].start_bus, 0);
        assert_eq!(info.allocations[0].end_bus, 255);
    }

    #[test]
    fn test_parse_fadt() {
        let mut data = make_header(b"FACP", 0x80);
        data.resize(0x80, 0);
        data[0x2E..0x30].copy_from_slice(&9u16.to_le_bytes()); // sci = 9
        data[0x30..0x34].copy_from_slice(&0xB2u32.to_le_bytes()); // smi = 0xB2
        data[0x5B] = 4; // pm_tmr_len
        fix_checksum(&mut data);

        let info = parse_fadt(&data).unwrap();
        assert_eq!(info.sci_int, 9);
        assert_eq!(info.smi_cmd, 0xB2);
        assert_eq!(info.pm_tmr_len, 4);
    }

    #[test]
    fn test_invalid_signature() {
        let mut data = make_header(b"WAIT", 44);
        fix_checksum(&mut data);
        assert_eq!(rsdt_entries(&data), None);
    }
}