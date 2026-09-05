// ============================================================
// IOMMU 框架
// ============================================================
// I/O 内存管理单元：为 DMA 提供地址重映射与隔离。
//
// 现状：Intel VT-d（DMAR 表）与 AMD-Vi（IVRS 表）的完整实现依赖
// ACPI 表中对应签名表的解析（阶段 6 先接入 FADT/MADT/MCFG，DMAR/
// IVRS 留待后续）；本模块先提供统一的接口与「恒等映射」实现——
// 在没有 IOMMU 或未启用时，设备 DMA 地址与物理地址一致，翻译是
// 恒等函数，语义正确且性能无损。
//
// 为什么保留框架：启用 IOMMU 后，所有驱动应通过本模块申请 DMA
// 地址，而不是直接使用物理地址；现在埋好接口，后续替换实现时
// 驱动层无需改动。

/// IOMMU 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuType {
    /// 无 IOMMU（恒等映射）
    None,
    /// Intel VT-d（需要在 ACPI DMAR 表中发现）
    IntelVtd,
    /// AMD-Vi（需要在 ACPI IVRS 表中发现）
    AmdVi,
}

/// IOMMU 上下文
#[derive(Debug, Clone, Copy)]
pub struct Iommu {
    /// 类型
    pub ty: IommuType,
    /// 硬件寄存器基址（物理地址）
    pub base_address: u64,
}

/// 全局 IOMMU 状态（初始化后为固定值；VT-d/AMD-Vi 接入前恒为 None）
static IOMMU: crate::sync::Spinlock<Iommu> =
    crate::sync::Spinlock::new(Iommu { ty: IommuType::None, base_address: 0 });

/// 初始化 IOMMU（当前为恒等模式）
///
/// 为什么直接返回成功：恒等映射是"零配置"的安全实现，任何设备
/// 都能工作；真正的 DMAR/IVRS 解析与页表安装待后续阶段接入。
pub fn init() {
    *IOMMU.lock() = Iommu { ty: IommuType::None, base_address: 0 };
}

/// 获取当前 IOMMU 类型
pub fn ty() -> IommuType {
    IOMMU.lock().ty
}

/// 设备 DMA 地址翻译
///
/// device_id 为 PCI BDF（总线<<8 | 设备<<3 | 功能）；在恒等
/// 模式下直接返回 iova 本身。后续 VT-d 接入后在此查设备域与
/// 二级页表。
pub fn translate(_device_id: u16, iova: u64) -> u64 {
    iova
}

/// 是否启用 IOMMU 地址重映射
pub fn is_enabled() -> bool {
    ty() != IommuType::None
}