// ============================================================
// IP 层（Internet Layer）根模块
// ============================================================
// 聚合 IP 层协议实现（IPv4、IPv6、路由、ICMP、分片）
// 负责数据包的路由转发

pub mod ipv4;
pub mod ipv6;
pub mod routing;
pub mod icmp;
pub mod icmpv6;
pub mod fragment;

use crate::lib::result::KernelResult;

// ============================================================
// IP 层公开接口
// ============================================================

/// 发送 IP 包（由上层协议调用）
pub fn send_packet(dest_ip: &[u8], protocol: u8, data: &[u8]) -> KernelResult<usize> {
    // 根据目标 IP 的版本（IPv4 或 IPv6）调用相应的发送函数
    if dest_ip.len() == 4 {
        // IPv4
        ipv4::send_packet(dest_ip, protocol, data)
    } else if dest_ip.len() == 16 {
        // IPv6
        ipv6::send_packet(dest_ip, protocol, data)
    } else {
        Err(crate::prelude::KernelError::InvalidArgument)
    }
}

/// 接收 IP 包（由链路层调用）
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    // 检查 IP 版本号，调用相应的接收处理
    if data.is_empty() {
        return Err(crate::prelude::KernelError::InvalidArgument);
    }

    let version = (data[0] >> 4) & 0x0F;
    match version {
        4 => ipv4::recv_packet(data),
        6 => ipv6::recv_packet(data),
        _ => Err(crate::prelude::KernelError::Unsupported),
    }
}

// ============================================================
// IP 层自测
// ============================================================

pub fn selftest() -> bool {
    ipv4::selftest() && ipv6::selftest() && routing::selftest() && icmp::selftest()
}
