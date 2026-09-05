// ============================================================
// ACPI 子系统
// ============================================================
// ACPI（高级配置与电源接口）核心驱动：发现 RSDP、读取 RSDT/XSDT、
// 解析 FADT/MADT/MCFG 等常见表，供 PCI（ECAM）、中断（MADT）、
// 电源管理（FADT）等子系统使用。
//
// 分层：
//   tables.rs —— 表结构定义（纯数据结构）
//   parse.rs  —— 字节流解析器（纯逻辑，可宿主单元测试）
//   mod.rs    —— 装配层：早期拷贝（init_early）+ 后期解析（init）
//
// 为什么需要「早期拷贝」：
// - QEMU/实机的 ACPI 表放在 RAM 顶部（如 128MB 内存时在
//   0x7FE00000 附近），而内核 PMM 的伙伴系统元数据也从被管理
//   内存顶部划分——mm::init 一运行就会覆盖这些固件表
// - 因此必须在 PMM 接管内存前（main.rs 在 arch::init 之后、
//   mm::init 之前调用 init_early）把需要的表复制进静态缓存；
//   真正的解析（init）在堆就绪后基于缓存进行
// - 早期阶段禁止任何堆分配（Vec/Box），只能使用定长数组

pub mod parse;
pub mod tables;

use core::sync::atomic::{AtomicBool, Ordering};

use crate::config::PHYSICAL_MEMORY_OFFSET;
use crate::prelude::KernelResult;
use crate::sync::Spinlock;

use self::parse::{FadtInfo, MadtInfo, McfgInfo};
use self::tables::sig;

/// 静态缓存大小（QEMU 表的集合通常在几 KB 内；64KB 留足余量）
const ACPI_CACHE_SIZE: usize = 64 * 1024;
/// RSDT/XSDT 中表指针的最大数量（64 张足够任何固件）
const MAX_TABLE_POINTERS: usize = 64;
/// RSDP 副本的最大数量（不同区域可能有多份残留）
const MAX_RSDP_CANDIDATES: usize = 8;

/// 早期拷贝阶段的状态（堆就绪前使用）
struct EarlyAcpi {
    /// 是否成功发现 RSDP
    found: bool,
    /// RSDP 修订号
    revision: u8,
    /// 表缓存（原始字节拷贝，供后期解析）
    cache: [u8; ACPI_CACHE_SIZE],
    /// 缓存已用长度
    cache_len: usize,
    /// 各表在缓存中的 [start, end) 范围
    fadt: Option<(usize, usize)>,
    madt: Option<(usize, usize)>,
    mcfg: Option<(usize, usize)>,
}

impl EarlyAcpi {
    const fn empty() -> Self {
        Self {
            found: false,
            revision: 0,
            cache: [0; ACPI_CACHE_SIZE],
            cache_len: 0,
            fadt: None,
            madt: None,
            mcfg: None,
        }
    }
}

/// 早期状态（init_early 填充；init 读取）
static EARLY: Spinlock<EarlyAcpi> = Spinlock::new(EarlyAcpi::empty());
/// 是否已执行早期拷贝（防止重复初始化）
static EARLY_DONE: AtomicBool = AtomicBool::new(false);

/// 解析得到的 ACPI 信息汇总
#[derive(Debug, Clone, Default)]
pub struct AcpiTables {
    /// RSDP 修订号
    pub revision: u8,
    /// RSDT 物理地址（可能为 0）
    pub rsdt_address: u64,
    /// XSDT 物理地址（可能为 0）
    pub xsdt_address: u64,
    /// FADT（电源管理信息）
    pub fadt: Option<FadtInfo>,
    /// MADT（APIC 拓扑）
    pub madt: Option<MadtInfo>,
    /// MCFG（PCIe ECAM 段）
    pub mcfg: Option<McfgInfo>,
}

/// ACPI 全局状态（init 解析后为 Some）
static ACPI: Spinlock<Option<AcpiTables>> = Spinlock::new(None);

// ---------------------------------------------------------------------
// 底层内存读取
// ---------------------------------------------------------------------

/// 读取物理内存的一段字节（经直接映射区）
///
/// # Safety
/// 调用方必须保证 [addr, addr+len) 位于已映射且可读的物理内存
unsafe fn read_phys_bytes(addr: u64, len: usize) -> &'static [u8] {
    unsafe {
        core::slice::from_raw_parts((PHYSICAL_MEMORY_OFFSET + addr) as *const u8, len)
    }
}

/// 按物理地址读取一张 ACPI 表（校验长度字段的合理性）
///
/// 返回指向直接映射区的只读切片；长度从表头偏移 4 读取。
/// 该函数不分配内存，可在早期初始化中使用。
unsafe fn read_acpi_table(phys: u64) -> Option<&'static [u8]> {
    if phys == 0 || phys >= 0x1_0000_0000 {
        return None;
    }
    let virt = PHYSICAL_MEMORY_OFFSET + phys;
    let len = unsafe { core::ptr::read_unaligned((virt + 4) as *const u32) };
    // 表长至少是 SDT 头（36），最大 1MB（避免越界读取制造假表）
    if !(36..=0x10_0000).contains(&len) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(virt as *const u8, len as usize) })
}

// ---------------------------------------------------------------------
// RSDP 发现（早期，无堆分配）
// ---------------------------------------------------------------------

/// 查找 RSDP 指针
///
/// 规范要求扫描两个区域（顺序无强制要求）：
/// 1. EBDA（扩展 BIOS 数据区）前 1KB
/// 2. 0xE0000 - 0xFFFFF 的 BIOS 区域
/// 每 16 字节对齐检查 "RSD PTR " 签名与校验和。
///
/// 固件可能留下多个 RSDP 副本（旧版本残留），因此收集全部候选后
/// 再挑选「指向真实表」的那个：仅校验和通过还不够，还要确认其
/// 指向的 RSDT/XSDT 真实存在（QEMU 实测出现过 F 段残留 rev 0
/// RSDP 指向全 0xFF 的情形）。
///
/// 为什么不用 Vec：本函数在 PMM/堆就绪前调用，只能使用定长数组。
fn find_rsdp() -> Option<u64> {
    // 先读取 BDA 中的 EBDA 段号（物理 0x40E 处 16 位）
    let ebda_seg = unsafe {
        let bytes = read_phys_bytes(0x40E, 2);
        u16::from_le_bytes([bytes[0], bytes[1]]) as u64
    };
    let ebda_base = ebda_seg << 4;

    // 候选区域：EBDA（若有效）→ BIOS 区域
    let mut regions = [(0u64, 0u64); 2];
    let mut nregions = 0;
    if ebda_base >= 0x80000 && ebda_base < 0xA0000 {
        regions[nregions] = (ebda_base, ebda_base + 1024);
        nregions += 1;
    }
    regions[nregions] = (0x000E_0000, 0x0010_0000);
    nregions += 1;

    // 收集通过校验和检查的候选
    let mut candidates = [0u64; MAX_RSDP_CANDIDATES];
    let mut ncandidates = 0;
    for (start, end) in &regions[..nregions] {
        let (mut addr, end) = (*start, *end);
        while addr + 36 <= end {
            let bytes = unsafe { read_phys_bytes(addr, 36) };
            if &bytes[..8] == b"RSD PTR " {
                // 校验：rev 0/1 校验前 20 字节；rev 2+ 校验整个结构
                let revision = bytes[15];
                let len = if revision >= 2 {
                    u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize
                } else {
                    20
                };
                if len >= 20 && len <= 36 && parse::checksum_valid(&bytes[..len]) {
                    if ncandidates < MAX_RSDP_CANDIDATES {
                        candidates[ncandidates] = addr;
                        ncandidates += 1;
                    }
                }
            }
            addr += 16;
        }
    }

    // 挑选指向真实表的候选：优先 rev 2+（XSDT），回退 rev 0（RSDT）
    for &addr in &candidates[..ncandidates] {
        let bytes = unsafe { read_phys_bytes(addr, 36) };
        let revision = bytes[15];
        if revision >= 2 {
            let xsdt = u64::from_le_bytes([
                bytes[24], bytes[25], bytes[26], bytes[27],
                bytes[28], bytes[29], bytes[30], bytes[31],
            ]);
            if xsdt != 0 && unsafe { read_acpi_table(xsdt) }.is_some() {
                return Some(addr);
            }
        } else {
            let rsdt = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as u64;
            if rsdt != 0 && unsafe { read_acpi_table(rsdt) }.is_some() {
                return Some(addr);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------
// 早期拷贝
// ---------------------------------------------------------------------

/// 把一张表复制进缓存，返回 [start, end)；缓存不足或表非法返回 None
fn copy_table_into_cache(cache: &mut EarlyAcpi, table: &[u8], sig: u32) -> Option<(usize, usize)> {
    let len = table.len();
    let start = cache.cache_len;
    if start + len > ACPI_CACHE_SIZE {
        return None;
    }
    cache.cache[start..start + len].copy_from_slice(table);
    cache.cache_len = start + len;
    let range = (start, start + len);
    // 同时登记签名对应的槽位
    match sig {
        sig::FADT => cache.fadt = Some(range),
        sig::MADT => cache.madt = Some(range),
        sig::MCFG => cache.mcfg = Some(range),
        _ => {}
    }
    Some(range)
}

/// ACPI 早期初始化：把固件表复制进静态缓存
///
/// 必须在 mm::init（PMM 接管内存）之前调用；成功后 is_early_ready()
/// 为 true，drivers::init 中的 acpi::init() 再从缓存解析。
///
/// 为什么不在早期直接解析：FadtInfo/MadtInfo 含有 Vec 等堆容器，
/// 早期堆未就绪；先拷贝原始字节，解析推迟到 drivers::init。
pub fn init_early() {
    // 防止重复执行（早期阶段可能被多处调用）
    if EARLY_DONE.swap(true, Ordering::SeqCst) {
        return;
    }

    let Some(rsdp_addr) = find_rsdp() else {
        return;
    };
    let rsdp = unsafe { read_phys_bytes(rsdp_addr, 36) };
    let revision = rsdp[15];
    let rsdt_address = u32::from_le_bytes([rsdp[16], rsdp[17], rsdp[18], rsdp[19]]) as u64;
    let xsdt_address = if revision >= 2 {
        u64::from_le_bytes([
            rsdp[24], rsdp[25], rsdp[26], rsdp[27],
            rsdp[28], rsdp[29], rsdp[30], rsdp[31],
        ])
    } else {
        0
    };

    // 收集表地址（无堆：定长数组）。优先 XSDT（64 位地址），回退 RSDT。
    let mut addrs = [0u64; MAX_TABLE_POINTERS];
    let mut naddrs = 0;

    if xsdt_address != 0 {
        if let Some(xsdt) = unsafe { read_acpi_table(xsdt_address) } {
            let len = parse::table_length(xsdt).unwrap_or(0) as usize;
            let mut off = core::mem::size_of::<self::tables::SdtHeader>();
            while off + 8 <= len && naddrs < MAX_TABLE_POINTERS {
                addrs[naddrs] = u64::from_le_bytes(xsdt[off..off + 8].try_into().unwrap());
                naddrs += 1;
                off += 8;
            }
        }
    }
    if naddrs == 0 {
        if let Some(rsdt) = unsafe { read_acpi_table(rsdt_address) } {
            let len = parse::table_length(rsdt).unwrap_or(0) as usize;
            let mut off = core::mem::size_of::<self::tables::SdtHeader>();
            while off + 4 <= len && naddrs < MAX_TABLE_POINTERS {
                addrs[naddrs] = u32::from_le_bytes(rsdt[off..off + 4].try_into().unwrap()) as u64;
                naddrs += 1;
                off += 4;
            }
        }
    }

    // 把感兴趣的表现在就复制（PMM 还没接管，固件表仍然完好）
    let mut early = EarlyAcpi {
        found: false,
        revision,
        cache: [0u8; ACPI_CACHE_SIZE],
        cache_len: 0,
        fadt: None,
        madt: None,
        mcfg: None,
    };
    for &addr in &addrs[..naddrs] {
        let Some(table) = (unsafe { read_acpi_table(addr) }) else { continue };
        let Some(sig) = parse::header_signature(table) else { continue };
        copy_table_into_cache(&mut early, table, sig);
    }
    early.found = true;

    *EARLY.lock() = early;
}

/// 早期拷贝是否成功（RSDP 找到且已复制表）
pub fn is_early_ready() -> bool {
    EARLY.lock().found
}

// ---------------------------------------------------------------------
// 解析（drivers::init 阶段，堆已就绪）
// ---------------------------------------------------------------------

/// ACPI 解析入口（drivers::init 调用）
///
/// 基于 init_early 复制到静态缓存的原始表字节，用 parse::* 解析
/// 成语义结构并保存到全局状态。无 ACPI 时保持 None，调用方通过
/// is_ready() 判断。
pub fn init() -> KernelResult<()> {
    let early = EARLY.lock();
    if !early.found {
        println!("[ACPI] tables not found (early copy missing)");
        return Err(crate::lib::result::KernelError::NotFound);
    }

    let acpi = AcpiTables {
        revision: early.revision,
        rsdt_address: 0, // 早期阶段未保存地址，保持 0（仅作版本佐证）
        xsdt_address: 0,
        fadt: early.fadt.map(|(s, e)| parse::parse_fadt(&early.cache[s..e]).unwrap_or_default()),
        madt: early.madt.map(|(s, e)| parse::parse_madt(&early.cache[s..e]).unwrap_or_default()),
        mcfg: early.mcfg.map(|(s, e)| parse::parse_mcfg(&early.cache[s..e]).unwrap_or_default()),
    };

    println!("[ACPI] RSDP rev {}: {} bytes of tables in cache", early.revision, early.cache_len);
    if let Some(f) = &acpi.fadt {
        println!("[ACPI] FADT: SCI={} SMI=0x{:X} flags=0x{:X}", f.sci_int, f.smi_cmd, f.flags);
    }
    if let Some(m) = &acpi.madt {
        println!("[ACPI] MADT: LAPIC=0x{:X} cpus={} IOAPICs={} overrides={}",
            m.lapic_address, m.lapic_count, m.io_apics.len(), m.overrides.len());
    }
    if let Some(m) = &acpi.mcfg {
        for a in &m.allocations {
            // 打包结构体字段未对齐，先拷贝到局部变量再打印（E0793）
            let (seg, start, end, base) = (a.pci_segment, a.start_bus, a.end_bus, a.base_address);
            println!("[ACPI] MCFG: seg={} bus {}-{} ECAM=0x{:X}", seg, start, end, base);
        }
    } else {
        println!("[ACPI] MCFG: not present (PCI port I/O fallback)");
    }

    *ACPI.lock() = Some(acpi);
    Ok(())
}

// ---------------------------------------------------------------------
// 公开接口
// ---------------------------------------------------------------------

/// ACPI 是否已成功解析
pub fn is_ready() -> bool {
    ACPI.lock().is_some()
}

/// 获取 MADT 信息（克隆返回，避免持有锁）
pub fn madt() -> Option<MadtInfo> {
    ACPI.lock().as_ref().and_then(|a| a.madt.clone())
}

/// 获取 MCFG 信息
pub fn mcfg() -> Option<McfgInfo> {
    ACPI.lock().as_ref().and_then(|a| a.mcfg.clone())
}

/// 获取 FADT 信息
pub fn fadt() -> Option<FadtInfo> {
    ACPI.lock().as_ref().and_then(|a| a.fadt.clone())
}

// ---------------------------------------------------------------------
// 启动自测
// ---------------------------------------------------------------------

/// ACPI 子系统自测
///
/// 依赖固件提供的真实 ACPI 表；无 ACPI 的环境直接判为 SKIP（不算
/// 失败），有表时校验解析出的关键字段。
pub fn selftest() -> bool {
    let Some(info) = ACPI.lock().clone() else {
        println!("    [SKIP] ACPI not present");
        return true;
    };
    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    all &= t("RSDP revision", info.revision == 0 || info.revision == 2);

    if let Some(f) = &info.fadt {
        all &= t("FADT SCI non-zero", f.sci_int != 0);
    } else {
        all &= t("FADT parsed", false);
    }

    if let Some(m) = &info.madt {
        all &= t("MADT LAPIC base", m.lapic_address != 0);
        all &= t("MADT has cpus", m.lapic_count > 0);
        all &= t("MADT has IOAPIC", !m.io_apics.is_empty());
    } else {
        all &= t("MADT parsed", false);
    }

    // MCFG 在纯 PCI（非 PCIe）平台上可能不存在，不强制
    if let Some(m) = &info.mcfg {
        all &= t("MCFG allocations", !m.allocations.is_empty());
    } else {
        println!("    [SKIP] MCFG not present (PCIe 平台才提供)");
    }

    all
}