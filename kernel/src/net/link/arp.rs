// ============================================================
// ARP 协议（Address Resolution Protocol）
// ============================================================
// 实现 ARP 协议用于 IP 地址到 MAC 地址的映射
//
// ARP 包结构：
//   [HwType(2) | ProtoType(2) | HwLen(1) | ProtoLen(1) | Op(2) |
//    SHA(6) | SPA(4) | THA(6) | TPA(4)]
//   HwType: 硬件类型（1 = 以太网）
//   ProtoType: 协议类型（0x0800 = IPv4）
//   Op: 操作码（1 = 请求，2 = 应答）
//   SHA: 源硬件地址（发送者 MAC）
//   SPA: 源协议地址（发送者 IP）
//   THA: 目标硬件地址（目标 MAC，请求时为 0）
//   TPA: 目标协议地址（目标 IP）
//
// 为什么分离 arp 模块：
// - ARP 是独立的协议，可能被多个上层调用
// - ARP 缓存管理相对复杂，单独模块便于维护
// - ARP 请求/应答的处理逻辑清晰，易于测试

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use super::ethernet::{EthernetFrame, EthernetHeader};
use super::super::types::{MacAddress, Ipv4Address};

/// ARP 操作码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpOp {
    /// ARP 请求
    Request = 1,
    /// ARP 应答
    Reply = 2,
}

/// ARP 包头（28 字节，针对以太网 + IPv4）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ArpHeader {
    /// 硬件类型（1 = 以太网）
    pub hw_type: [u8; 2],
    /// 协议类型（0x0800 = IPv4）
    pub proto_type: [u8; 2],
    /// 硬件地址长度（6 for Ethernet）
    pub hw_len: u8,
    /// 协议地址长度（4 for IPv4）
    pub proto_len: u8,
    /// 操作码（1 = 请求，2 = 应答）
    pub op: [u8; 2],
    /// 发送者硬件地址（MAC）
    pub sender_hw: [u8; 6],
    /// 发送者协议地址（IP）
    pub sender_proto: [u8; 4],
    /// 目标硬件地址（MAC）
    pub target_hw: [u8; 6],
    /// 目标协议地址（IP）
    pub target_proto: [u8; 4],
}

impl ArpHeader {
    /// 创建新的 ARP 包头
    pub fn new(
        op: ArpOp,
        sender_mac: MacAddress,
        sender_ip: Ipv4Address,
        target_mac: MacAddress,
        target_ip: Ipv4Address,
    ) -> Self {
        ArpHeader {
            hw_type: (1u16).to_be_bytes(),  // 以太网
            proto_type: (0x0800u16).to_be_bytes(),  // IPv4
            hw_len: 6,
            proto_len: 4,
            op: (op as u16).to_be_bytes(),
            sender_hw: *sender_mac.as_bytes(),
            sender_proto: *sender_ip.as_bytes(),
            target_hw: *target_mac.as_bytes(),
            target_proto: *target_ip.as_bytes(),
        }
    }

    /// 获取操作码
    pub fn op(&self) -> KernelResult<ArpOp> {
        match u16::from_be_bytes(self.op) {
            1 => Ok(ArpOp::Request),
            2 => Ok(ArpOp::Reply),
            _ => Err(KernelError::InvalidArgument),
        }
    }

    /// 获取发送者 MAC 地址
    pub fn sender_mac(&self) -> MacAddress {
        MacAddress::from_bytes(self.sender_hw)
    }

    /// 获取发送者 IP 地址
    pub fn sender_ip(&self) -> Ipv4Address {
        Ipv4Address::from_bytes(self.sender_proto)
    }

    /// 获取目标 MAC 地址
    pub fn target_mac(&self) -> MacAddress {
        MacAddress::from_bytes(self.target_hw)
    }

    /// 获取目标 IP 地址
    pub fn target_ip(&self) -> Ipv4Address {
        Ipv4Address::from_bytes(self.target_proto)
    }

    /// 从字节数组解析 ARP 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < core::mem::size_of::<ArpHeader>() {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const ArpHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组
    pub fn to_bytes(&self) -> [u8; 28] {
        let mut bytes = [0u8; 28];
        bytes[0..2].copy_from_slice(&self.hw_type);
        bytes[2..4].copy_from_slice(&self.proto_type);
        bytes[4] = self.hw_len;
        bytes[5] = self.proto_len;
        bytes[6..8].copy_from_slice(&self.op);
        bytes[8..14].copy_from_slice(&self.sender_hw);
        bytes[14..18].copy_from_slice(&self.sender_proto);
        bytes[18..24].copy_from_slice(&self.target_hw);
        bytes[24..28].copy_from_slice(&self.target_proto);
        bytes
    }
}

// ============================================================
// ARP 缓存表项
// ============================================================

/// ARP 缓存条目状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpEntryState {
    /// 待决（已发送请求，等待应答）
    Incomplete,
    /// 可用（已解析）
    Reachable,
    /// 不可达（多次失败）
    Failed,
}

/// ARP 缓存条目
#[derive(Debug, Clone)]
pub struct ArpEntry {
    /// IPv4 地址
    pub ip: Ipv4Address,
    /// MAC 地址
    pub mac: MacAddress,
    /// 状态
    pub state: ArpEntryState,
    /// 创建时间戳（秒）
    pub timestamp: u64,
    /// 失败次数（超过阈值则丢弃）
    pub failures: u8,
}

impl ArpEntry {
    /// 创建新的 ARP 缓存条目
    pub fn new(ip: Ipv4Address, mac: MacAddress) -> Self {
        ArpEntry {
            ip,
            mac,
            state: ArpEntryState::Reachable,
            timestamp: 0,  // 后续由外层设置时间戳
            failures: 0,
        }
    }

    /// 检查条目是否已过期
    pub fn is_expired(&self, current_time: u64, timeout: u64) -> bool {
        (current_time - self.timestamp) > timeout
    }
}

// ============================================================
// ARP 协议处理
// ============================================================

/// 创建 ARP 请求包
pub fn create_request(
    sender_mac: MacAddress,
    sender_ip: Ipv4Address,
    target_ip: Ipv4Address,
) -> EthernetFrame {
    // ARP 请求的目标 MAC 为全零（或广播）
    let _target_mac = MacAddress::broadcast();

    let arp_header = ArpHeader::new(
        ArpOp::Request,
        sender_mac,
        sender_ip,
        MacAddress::zero(),
        target_ip,
    );

    let eth_header = EthernetHeader::new(
        MacAddress::broadcast(),  // 以太网广播
        sender_mac,
        0x0806,  // ARP EtherType
    );

    EthernetFrame::new(eth_header, arp_header.to_bytes().to_vec())
}

/// 创建 ARP 应答包
pub fn create_reply(
    sender_mac: MacAddress,
    sender_ip: Ipv4Address,
    target_mac: MacAddress,
    target_ip: Ipv4Address,
) -> EthernetFrame {
    let arp_header = ArpHeader::new(
        ArpOp::Reply,
        sender_mac,
        sender_ip,
        target_mac,
        target_ip,
    );

    let eth_header = EthernetHeader::new(target_mac, sender_mac, 0x0806);

    EthernetFrame::new(eth_header, arp_header.to_bytes().to_vec())
}

/// 解析 ARP 包
pub fn parse_arp_frame(frame: &EthernetFrame) -> KernelResult<ArpHeader> {
    if frame.payload.len() < core::mem::size_of::<ArpHeader>() {
        return Err(KernelError::InvalidArgument);
    }
    ArpHeader::from_bytes(&frame.payload)
}

// ============================================================
// ARP 自测
// ============================================================

pub fn selftest() -> bool {
    // IPv4 地址测试
    let ip1 = Ipv4Address::from_parts(192, 168, 1, 1);
    assert_eq!(ip1.as_bytes(), &[192, 168, 1, 1], "IPv4 地址创建失败");

    // ARP 请求创建
    let sender_mac = MacAddress::from_bytes([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    let sender_ip = Ipv4Address::from_parts(192, 168, 1, 10);
    let target_ip = Ipv4Address::from_parts(192, 168, 1, 1);

    let request = create_request(sender_mac, sender_ip, target_ip);
    assert_eq!(request.payload.len(), 28, "ARP 请求大小不正确");

    // ARP 应答创建
    let target_mac = MacAddress::from_bytes([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    let reply = create_reply(target_mac, target_ip, sender_mac, sender_ip);
    assert_eq!(reply.payload.len(), 28, "ARP 应答大小不正确");

    // ARP 包解析
    let parsed_request = parse_arp_frame(&request).unwrap_or_else(|_| {
        panic!("ARP 请求解析失败");
    });
    assert_eq!(parsed_request.sender_ip(), sender_ip, "发送者 IP 不匹配");
    assert_eq!(parsed_request.target_ip(), target_ip, "目标 IP 不匹配");

    true
}
