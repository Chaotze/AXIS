// ============================================================
// UEFI 内存映射获取
// ============================================================
// 获取系统物理内存布局

use uefi::table::boot::{MemoryDescriptor, MemoryType};
use uefi::mem::memory_map::MemoryMapOwned;

/// 内存区域类型（与 common/boot_info.rs 保持一致）
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum MemoryRegionType {
    Usable = 1,
    Reserved = 2,
    AcpiReclaimable = 3,
    AcpiNvs = 4,
    Bad = 5,
    Bootloader = 0x1000,
    Kernel = 0x1001,
    Framebuffer = 0x1002,
}

/// 内存区域描述符
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct MemoryRegion {
    pub start: u64,
    pub len: u64,
    pub region_type: MemoryRegionType,
}

/// 获取 UEFI 内存映射
///
/// UEFI 提供详细的物理内存布局，包括：
/// - 可用内存（Conventional Memory）
/// - 保留内存（Reserved）
/// - ACPI 内存（ACPI Reclaim / ACPI NVS）
/// - 运行时服务代码和数据
/// - 等等
///
/// 返回的内存映射会在 ExitBootServices 后继续有效
pub fn get_memory_map(boot_services: &uefi::table::boot::BootServices) -> uefi::Result<MemoryMapOwned> {
    // 获取 UEFI 内存映射
    // 为什么需要 owned？
    //   - ExitBootServices 需要一个内存映射的 key
    //   - 获取后到调用 ExitBootServices 之间，内存映射可能发生变化
    //   - 使用 owned 版本可以在调用时再次获取最新的映射
    let memory_map = boot_services.memory_map(MemoryType::LOADER_DATA)?;

    Ok(memory_map)
}

/// 将 UEFI 内存类型转换为 AXIS 内存区域类型
///
/// UEFI 定义了多种内存类型，需要映射到 AXIS 的统一内存类型
#[allow(dead_code)]
pub fn convert_memory_type(uefi_type: MemoryType) -> MemoryRegionType {
    match uefi_type {
        // 可用内存
        MemoryType::CONVENTIONAL => MemoryRegionType::Usable,

        // ACPI 可回收内存
        MemoryType::ACPI_RECLAIM => MemoryRegionType::AcpiReclaimable,

        // ACPI NVS 内存
        MemoryType::ACPI_NON_VOLATILE => MemoryRegionType::AcpiNvs,

        // 引导加载程序使用的内存
        MemoryType::LOADER_CODE | MemoryType::LOADER_DATA => MemoryRegionType::Bootloader,

        // 所有其他类型都视为保留
        // 包括：
        //   - RESERVED: 保留内存
        //   - UNUSABLE: 不可用内存
        //   - RUNTIME_SERVICES_CODE/DATA: UEFI 运行时服务
        //   - BOOT_SERVICES_CODE/DATA: UEFI 引导服务（ExitBootServices 后释放）
        //   - MEMORY_MAPPED_IO: MMIO 区域
        //   - PERSISTENT_MEMORY: 持久化内存
        _ => MemoryRegionType::Reserved,
    }
}

/// 将 UEFI 内存描述符转换为 AXIS 内存区域
#[allow(dead_code)]
pub fn convert_memory_descriptor(desc: &MemoryDescriptor) -> MemoryRegion {
    let start = desc.phys_start;
    let len = desc.page_count * 4096; // UEFI 页面大小固定为 4KB
    let region_type = convert_memory_type(desc.ty);

    MemoryRegion {
        start,
        len,
        region_type,
    }
}
