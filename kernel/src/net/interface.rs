// ============================================================
// 网络接口配置（Network Interface Configuration）
// ============================================================
// 管理本机网络接口的 IP 地址、子网掩码等配置
//
// 设计原则：
// - 支持多个网络接口（当前简化为单个）
// - 每个接口可配置多个 IPv4/IPv6 地址
// - 提供一致的地址查询接口

use crate::lib::result::KernelResult;
use super::types::{Ipv4Address, Ipv6Address, MacAddress};
use alloc::vec::Vec;

// ============================================================
// 网络接口配置
// ============================================================

/// IPv4 接口地址配置
#[derive(Debug, Clone, Copy)]
pub struct Ipv4InterfaceConfig {
    /// IP 地址
    pub addr: Ipv4Address,
    /// 子网掩码（CIDR 前缀长度）
    pub prefix_len: u8,
    /// 网关地址
    pub gateway: Ipv4Address,
}

impl Ipv4InterfaceConfig {
    /// 创建新的 IPv4 接口配置
    pub fn new(addr: Ipv4Address, prefix_len: u8, gateway: Ipv4Address) -> Self {
        Ipv4InterfaceConfig {
            addr,
            prefix_len,
            gateway,
        }
    }

    /// 获取子网掩码
    pub fn netmask(&self) -> Ipv4Address {
        if self.prefix_len == 0 {
            Ipv4Address::from_u32(0)
        } else if self.prefix_len == 32 {
            Ipv4Address::from_u32(0xFFFFFFFF)
        } else {
            let mask = (0xFFFFFFFFu32 << (32 - self.prefix_len)) & 0xFFFFFFFF;
            Ipv4Address::from_u32(mask)
        }
    }

    /// 获取网络地址
    pub fn network_addr(&self) -> Ipv4Address {
        let addr_u32 = self.addr.as_u32();
        let mask = self.netmask().as_u32();
        Ipv4Address::from_u32(addr_u32 & mask)
    }

    /// 获取广播地址
    pub fn broadcast_addr(&self) -> Ipv4Address {
        let addr_u32 = self.addr.as_u32();
        let mask = self.netmask().as_u32();
        Ipv4Address::from_u32(addr_u32 | !mask)
    }

    /// 检查 IP 是否在同一子网内
    pub fn is_on_same_subnet(&self, ip: Ipv4Address) -> bool {
        self.network_addr() == Ipv4Address::from_u32(ip.as_u32() & self.netmask().as_u32())
    }
}

/// 网络接口配置
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// 接口名称（如 eth0、vnic0）
    pub name: alloc::string::String,
    /// MAC 地址
    pub mac: MacAddress,
    /// IPv4 地址配置
    pub ipv4_config: Option<Ipv4InterfaceConfig>,
    /// IPv6 地址配置（简化，后续支持）
    pub ipv6_addrs: Vec<Ipv6Address>,
    /// 接口启用状态
    pub enabled: bool,
    /// MTU（最大传输单元）
    pub mtu: u16,
}

impl NetworkInterface {
    /// 创建新的网络接口
    pub fn new(name: alloc::string::String, mac: MacAddress) -> Self {
        NetworkInterface {
            name,
            mac,
            ipv4_config: None,
            ipv6_addrs: Vec::new(),
            enabled: true,
            mtu: 1500,
        }
    }

    /// 配置 IPv4 地址
    pub fn set_ipv4(&mut self, config: Ipv4InterfaceConfig) {
        self.ipv4_config = Some(config);
    }

    /// 获取 IPv4 地址
    pub fn ipv4_addr(&self) -> Option<Ipv4Address> {
        self.ipv4_config.map(|cfg| cfg.addr)
    }
}

// ============================================================
// 全局接口表
// ============================================================

use crate::sync::Spinlock;

/// 全局网络接口表
static INTERFACES: Spinlock<Vec<NetworkInterface>> = Spinlock::new(Vec::new());

/// 初始化网络接口表
pub fn init_interfaces() -> KernelResult<()> {
    let mut interfaces = INTERFACES.lock();

    // 创建默认虚拟网卡接口
    let mac = crate::drivers::nic::get_mac_address();
    let mut vnic_iface = NetworkInterface::new(
        alloc::string::String::from("vnic0"),
        MacAddress::from_bytes(mac),
    );

    // 配置默认 IPv4 地址（192.168.1.10/24）
    // 为什么使用这个地址：
    // - 常见的私有网络地址段
    // - 便于测试和开发
    // - 可通过配置系统调用修改
    let ipv4_cfg = Ipv4InterfaceConfig::new(
        Ipv4Address::from_parts(192, 168, 1, 10),
        24,
        Ipv4Address::from_parts(192, 168, 1, 1),
    );
    vnic_iface.set_ipv4(ipv4_cfg);

    interfaces.push(vnic_iface);

    println!("[NET-IF] Network interfaces initialized");
    println!("[NET-IF]   vnic0: 192.168.1.10/24");

    Ok(())
}

/// 获取默认网络接口的 IPv4 地址
pub fn get_default_ipv4_addr() -> Option<Ipv4Address> {
    let interfaces = INTERFACES.lock();
    interfaces.first().and_then(|iface| iface.ipv4_addr())
}

/// 根据目标 IP 查找出接口和下一跳地址
pub fn find_outgoing_interface(dest_ip: Ipv4Address) -> Option<(usize, Ipv4Address)> {
    let interfaces = INTERFACES.lock();

    for (idx, iface) in interfaces.iter().enumerate() {
        if !iface.enabled {
            continue;
        }

        if let Some(cfg) = iface.ipv4_config {
            // 如果目标 IP 在同一子网内，直接发送
            if cfg.is_on_same_subnet(dest_ip) {
                return Some((idx, dest_ip));
            }
        }
    }

    // 默认使用网关
    if let Some(iface) = interfaces.first() {
        if let Some(cfg) = iface.ipv4_config {
            return Some((0, cfg.gateway));
        }
    }

    None
}

/// 获取指定接口的 MAC 地址
pub fn get_interface_mac(iface_idx: usize) -> Option<MacAddress> {
    let interfaces = INTERFACES.lock();
    interfaces.get(iface_idx).map(|iface| iface.mac)
}
