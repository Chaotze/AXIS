// ============================================================
// IPv6 协议实现
// ============================================================
// 实现 RFC 2460 IPv6 协议
//
// IPv6 包头结构（40 字节固定）：
//   [Ver(4) | TrafficClass(8) | FlowLabel(20) | PayloadLen(16) |
//    NextHdr(8) | HopLimit(8) | SrcAddr(128) | DstAddr(128)]

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use super::super::types::Ipv6Address;

// ============================================================
// IPv6 包头
// ============================================================

/// IPv6 包头（40 字节固定）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv6Header {
    /// 版本(4) + 流量类(8) + 流标签(20)
    pub version_traffic_flow: [u8; 4],
    /// 有效负载长度（大端，不包括 40 字节头）
    pub payload_length: [u8; 2],
    /// 下一个头类型（协议号）
    pub next_header: u8,
    /// 跳数限制（类似 IPv4 的 TTL）
    pub hop_limit: u8,
    /// 源 IPv6 地址
    pub src_addr: [u8; 16],
    /// 目标 IPv6 地址
    pub dst_addr: [u8; 16],
}

impl Ipv6Header {
    /// 创建新的 IPv6 包头
    /// 为什么这样设计：IPv6 固定 40 字节头，简化了头部处理逻辑
    pub fn new(
        src_addr: Ipv6Address,
        dst_addr: Ipv6Address,
        next_header: u8,
        payload_len: u16,
    ) -> Self {
        // 版本 6、流量类 0、流标签 0
        let version_traffic_flow = [0x60, 0x00, 0x00, 0x00];

        Ipv6Header {
            version_traffic_flow,
            payload_length: payload_len.to_be_bytes(),
            next_header,
            hop_limit: 64,  // 初始跳数限制
            src_addr: *src_addr.as_bytes(),
            dst_addr: *dst_addr.as_bytes(),
        }
    }

    /// 获取版本号（应为 6）
    pub fn version(&self) -> u8 {
        (self.version_traffic_flow[0] >> 4) & 0x0F
    }

    /// 获取有效负载长度
    pub fn payload_length(&self) -> u16 {
        u16::from_be_bytes(self.payload_length)
    }

    /// 获取下一个头类型
    pub fn next_header(&self) -> u8 {
        self.next_header
    }

    /// 获取跳数限制
    pub fn hop_limit(&self) -> u8 {
        self.hop_limit
    }

    /// 获取源地址
    pub fn src_addr(&self) -> Ipv6Address {
        Ipv6Address::from_bytes(self.src_addr)
    }

    /// 获取目标地址
    pub fn dst_addr(&self) -> Ipv6Address {
        Ipv6Address::from_bytes(self.dst_addr)
    }

    /// 从字节数组解析 IPv6 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 40 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const Ipv6Header)
        };

        // 验证版本
        if header.version() != 6 {
            return Err(KernelError::InvalidArgument);
        }

        Ok(header)
    }

    /// 转换为字节数组
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut bytes = [0u8; 40];
        bytes[0..4].copy_from_slice(&self.version_traffic_flow);
        bytes[4..6].copy_from_slice(&self.payload_length);
        bytes[6] = self.next_header;
        bytes[7] = self.hop_limit;
        bytes[8..24].copy_from_slice(&self.src_addr);
        bytes[24..40].copy_from_slice(&self.dst_addr);
        bytes
    }
}

// ============================================================
// IPv6 包处理
// ============================================================

/// 发送 IPv6 包
pub fn send_packet(_dest_ip: &[u8], _protocol: u8, data: &[u8]) -> KernelResult<usize> {
    if _dest_ip.len() != 16 {
        return Err(KernelError::InvalidArgument);
    }

    let _dest = Ipv6Address::from_bytes({
        let mut addr = [0u8; 16];
        addr.copy_from_slice(_dest_ip);
        addr
    });

    // TODO: 查询本机 IPv6 地址
    // TODO: 处理 IPv6 扩展头
    // TODO: 通过链路层发送

    // 占位符：返回成功
    Ok(data.len())
}

/// 接收 IPv6 包
/// 关键处理路径：验证 → Hop Limit 检查 → 上层分发
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    if data.len() < 40 {
        return Err(KernelError::InvalidArgument);
    }

    let header = Ipv6Header::from_bytes(data)?;

    // 1. 检查跳数限制（Hop Limit，等同于 IPv4 的 TTL）
    // 为什么需要检查：防止无限转发，确保数据包最终被丢弃
    if header.hop_limit() == 0 {
        // TODO: 发送 ICMPv6 超时通知给源地址
        return Err(KernelError::Other("Hop limit exceeded"));
    }

    // 2. 处理 IPv6 扩展头（当前简化实现，暂不处理）
    // 为什么需要扩展头：IPv6 支持多种扩展头（路由头、Hop-by-Hop 选项等）
    // TODO: 解析下一个头，处理扩展头链

    // 3. 获取有效负载（跳过 40 字节的 IPv6 头）
    let payload_start = 40;
    if data.len() < payload_start {
        return Err(KernelError::InvalidArgument);
    }

    let payload = &data[payload_start..];
    let protocol = header.next_header();

    // 4. 根据协议号分发给上层处理
    match protocol {
        // ICMPv6 处理
        crate::net::config::ip_protocol::ICMPV6 => {
            // TODO: 调用 icmpv6::recv_icmpv6()
        }
        // TCP 处理
        crate::net::config::ip_protocol::TCP => {
            // TODO: 调用传输层接收函数
        }
        // UDP 处理
        crate::net::config::ip_protocol::UDP => {
            // TODO: 调用传输层接收函数
        }
        _ => {
            // 不支持的协议号
            return Err(KernelError::Unsupported);
        }
    }

    let _ = payload;  // 占位符，避免未使用警告
    Ok(())
}

// ============================================================
// IPv6 自测
// ============================================================

pub fn selftest() -> bool {
    // IPv6 地址测试
    let loopback = Ipv6Address::loopback();
    assert_eq!(loopback.as_bytes()[15], 1, "IPv6 环回地址错误");

    let unspecified = Ipv6Address::unspecified();
    let unspec_bytes = unspecified.as_bytes();
    assert!(unspec_bytes.iter().all(|&b| b == 0), "未指定地址应全为零");

    // IPv6 包头创建和验证
    let src = Ipv6Address::from_bytes([
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    let dst = Ipv6Address::from_bytes([
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 2,
    ]);

    let header = Ipv6Header::new(src, dst, 6, 1000);  // 协议 6 = TCP

    assert_eq!(header.version(), 6, "IPv6 版本应为 6");
    assert_eq!(header.payload_length(), 1000, "有效负载长度不匹配");
    assert_eq!(header.next_header(), 6, "下一个头类型不匹配");
    assert_eq!(header.hop_limit(), 64, "跳数限制默认值错误");

    // 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = Ipv6Header::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("IPv6 包头解析失败");
    });

    assert_eq!(parsed.src_addr(), src, "源地址不匹配");
    assert_eq!(parsed.dst_addr(), dst, "目标地址不匹配");

    true
}
