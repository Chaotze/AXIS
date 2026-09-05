// ============================================================
// 网络接口卡（NIC）驱动模块
// ============================================================
// 提供网络设备的统一抽象接口，支持多种 NIC 驱动
//
// 设计原则：
// - 所有 NIC 实现 NetworkDevice trait
// - 虚拟 NIC 用于测试，真实 NIC 驱动后续集成
// - 使用静态注册的单一网卡实例

pub mod virtual_nic;

use alloc::vec::Vec;
use crate::lib::result::KernelResult;
use crate::sync::Spinlock;

// ============================================================
// 网络设备抽象 Trait
// ============================================================

/// 网络设备操作接口
/// 所有 NIC 驱动必须实现此 trait
pub trait NetworkDevice: Send + Sync {
    /// 发送网络帧
    /// 参数：frame - 完整的帧数据（包含以太网头）
    /// 返回：发送的字节数
    fn send(&self, frame: &[u8]) -> KernelResult<usize>;

    /// 接收网络帧
    /// 返回：接收到的帧数据（包含以太网头）
    /// 注意：这是轮询方式，阻塞直到有数据
    fn recv(&self) -> KernelResult<Vec<u8>>;

    /// 获取 MAC 地址
    fn mac_address(&self) -> [u8; 6];

    /// 网络设备名称
    fn name(&self) -> &str;

    /// 启用网络设备
    fn enable(&self) -> KernelResult<()>;

    /// 禁用网络设备
    fn disable(&self) -> KernelResult<()>;
}

// ============================================================
// 全局网络设备管理
// ============================================================

/// 全局网络设备实例包装
/// 使用 Option 存储网络设备，初始为 None
/// 初始化后存储虚拟网卡的引用
static NETWORK_DEVICE: Spinlock<Option<&'static dyn NetworkDevice>> = Spinlock::new(None);

/// 全局虚拟网卡实例（单例）
/// 为什么是全局 static：
/// - 需要在整个内核生命周期内保持活跃
/// - 虚拟网卡实现了 NetworkDevice trait 且是 'static
static mut VNIC_INSTANCE: Option<virtual_nic::VirtualNic> = None;

/// 初始化全局网络设备
pub fn init_network_device() -> KernelResult<()> {
    println!("[DRIVERS-NIC] Initializing network device...");

    // 创建虚拟 NIC 用于测试
    unsafe {
        VNIC_INSTANCE = Some(virtual_nic::VirtualNic::new());

        // 启用虚拟网卡
        if let Some(ref vnic) = VNIC_INSTANCE {
            vnic.enable()?;

            // 将虚拟网卡注册为全局网络设备
            let mut device = NETWORK_DEVICE.lock();
            *device = Some(vnic as &'static dyn NetworkDevice);

            println!("[DRIVERS-NIC] Network device initialized: {}", vnic.name());
        }
    }

    Ok(())
}

/// 获取全局网络设备引用
fn get_network_device() -> Option<&'static dyn NetworkDevice> {
    let device = NETWORK_DEVICE.lock();
    *device
}

/// 发送帧（通过全局网络设备）
pub fn send_frame(frame: &[u8]) -> KernelResult<usize> {
    match get_network_device() {
        Some(dev) => dev.send(frame),
        None => Err(crate::prelude::KernelError::NotFound),
    }
}

/// 接收帧（通过全局网络设备）
pub fn recv_frame() -> KernelResult<Vec<u8>> {
    match get_network_device() {
        Some(dev) => dev.recv(),
        None => Err(crate::prelude::KernelError::NotFound),
    }
}

/// 获取本机 MAC 地址
pub fn get_mac_address() -> [u8; 6] {
    match get_network_device() {
        Some(dev) => dev.mac_address(),
        None => [0; 6],
    }
}
