// ============================================================
// MSI/MSI-X 支持
// ============================================================
// 消息信号中断，用于 PCIe 设备

/// MSI 地址寄存器格式
///
/// MSI 将中断转换为内存写操作，直接写到 APIC
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MsiAddress {
    pub value: u32,
}

impl MsiAddress {
    /// 创建 MSI 地址
    ///
    /// 格式：
    /// - bits 31-20: 0xFEE（固定，指向 APIC）
    /// - bits 19-12: Destination ID（目标 APIC ID）
    /// - bits 11-4: 保留
    /// - bit 3: Redirection Hint（0=不使用，1=使用）
    /// - bit 2: Destination Mode（0=物理，1=逻辑）
    /// - bits 1-0: 保留（必须为 0）
    pub fn new(dest_id: u8, dest_mode_logical: bool) -> Self {
        // MSI 地址必须是 Local APIC 的物理地址 0xFEE00000，
        // 与页表无关：设备在总线上以该地址发起写事务送达 APIC，
        // CPU 不对 MSI 地址做页表转换，因此**不能**加 PHYSICAL_MEMORY_OFFSET
        let mut value = 0xFEE00000;
        value |= (dest_id as u32) << 12;
        if dest_mode_logical {
            value |= 1 << 2;
        }
        Self { value }
    }
}

/// MSI 数据寄存器格式
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MsiData {
    pub value: u32,
}

impl MsiData {
    /// 创建 MSI 数据
    ///
    /// 格式：
    /// - bits 15: Trigger Mode（0=边沿，1=电平）
    /// - bit 14: Level（触发电平，仅电平触发有效）
    /// - bits 10-8: Delivery Mode（000=Fixed, 001=Lowest Priority, etc.）
    /// - bits 7-0: Vector（中断向量号）
    pub fn new(vector: u8, trigger_level: bool) -> Self {
        let mut value = vector as u32;
        if trigger_level {
            value |= 1 << 15; // 电平触发
            value |= 1 << 14; // Assert
        }
        // Delivery Mode = Fixed (000)
        Self { value }
    }
}

/// MSI 能力结构
///
/// 暂时简化，实际需要解析 PCI 配置空间
#[allow(dead_code)]
pub struct MsiCapability {
    pub address: MsiAddress,
    pub data: MsiData,
}

impl MsiCapability {
    /// 创建 MSI 配置
    ///
    /// # 参数
    /// - `dest_id`: 目标 CPU 的 APIC ID
    /// - `vector`: 中断向量号
    pub fn new(dest_id: u8, vector: u8) -> Self {
        Self {
            address: MsiAddress::new(dest_id, false),
            data: MsiData::new(vector, false),
        }
    }
}

/// 配置设备的 MSI
///
/// 实际实现需要：
/// 1. 在 PCI 配置空间中找到 MSI 能力结构
/// 2. 写入地址和数据寄存器
/// 3. 启用 MSI
///
/// 暂时为占位实现
#[allow(dead_code)]
pub fn configure_msi(_device: u32, _capability: &MsiCapability) -> Result<(), &'static str> {
    // TODO: 实现实际的 MSI 配置
    Ok(())
}
