// ============================================================
// 传输层（Transport Layer）根模块
// ============================================================
// 聚合传输层协议实现（TCP、UDP、Socket）
// 负责进程间的数据通信

pub mod socket;
pub mod udp;
pub mod tcp;

use crate::lib::result::KernelResult;

// ============================================================
// 传输层公开接口
// ============================================================

/// 发送传输层数据
pub fn send_data(
    protocol: u8,
    src_port: u16,
    dst_port: u16,
    data: &[u8],
) -> KernelResult<usize> {
    match protocol {
        crate::net::config::ip_protocol::UDP => {
            udp::send_packet(src_port, dst_port, data)
        }
        crate::net::config::ip_protocol::TCP => {
            tcp::send_packet(src_port, dst_port, data)
        }
        _ => Err(crate::prelude::KernelError::Unsupported),
    }
}

/// 接收传输层数据
pub fn recv_data(protocol: u8, data: &[u8]) -> KernelResult<()> {
    match protocol {
        crate::net::config::ip_protocol::UDP => udp::recv_packet(data),
        crate::net::config::ip_protocol::TCP => tcp::recv_packet(data),
        _ => Err(crate::prelude::KernelError::Unsupported),
    }
}

// ============================================================
// 传输层自测
// ============================================================

pub fn selftest() -> bool {
    socket::selftest() && udp::selftest() && tcp::selftest()
}
