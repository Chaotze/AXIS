// ============================================================
// UDP 协议（User Datagram Protocol）
// ============================================================
// 实现 RFC 768 UDP 协议
//
// UDP 包头结构（8 字节）：
//   [SrcPort(16) | DstPort(16) | Length(16) | Checksum(16) | Data(variable)]
//
// UDP 特点：
// - 无连接
// - 不可靠（不保证到达）
// - 低开销、低延迟

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;

// ============================================================
// UDP 包头
// ============================================================

/// UDP 包头（8 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UdpHeader {
    /// 源端口（大端）
    pub src_port: [u8; 2],
    /// 目标端口（大端）
    pub dst_port: [u8; 2],
    /// 长度（包括包头，大端）
    pub length: [u8; 2],
    /// 校验和（大端，0 表示不计算）
    pub checksum: [u8; 2],
}

impl UdpHeader {
    /// 创建新的 UDP 包头
    pub fn new(src_port: u16, dst_port: u16, payload_len: u16) -> Self {
        UdpHeader {
            src_port: src_port.to_be_bytes(),
            dst_port: dst_port.to_be_bytes(),
            length: (8u16 + payload_len).to_be_bytes(),
            checksum: [0, 0],  // 可选，设为 0 表示不检查
        }
    }

    /// 获取源端口
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }

    /// 获取目标端口
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }

    /// 获取长度
    pub fn length(&self) -> u16 {
        u16::from_be_bytes(self.length)
    }

    /// 从字节数组解析 UDP 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 8 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const UdpHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0..2].copy_from_slice(&self.src_port);
        bytes[2..4].copy_from_slice(&self.dst_port);
        bytes[4..6].copy_from_slice(&self.length);
        bytes[6..8].copy_from_slice(&self.checksum);
        bytes
    }
}

// ============================================================
// UDP 套接字管理
// ============================================================

/// UDP 套接字
#[derive(Debug, Clone)]
pub struct UdpSocket {
    /// 本地端口
    pub local_port: u16,
    /// 绑定的接口 IP
    pub local_ip: [u8; 4],
    /// 接收缓冲区（简化版，存储最后一个包）
    pub recv_buffer: alloc::vec::Vec<u8>,
    /// 最后一个包的源地址
    pub last_src_addr: ([u8; 4], u16),
}

impl UdpSocket {
    /// 创建新的 UDP 套接字
    pub fn new(local_ip: [u8; 4], local_port: u16) -> Self {
        UdpSocket {
            local_port,
            local_ip,
            recv_buffer: alloc::vec::Vec::new(),
            last_src_addr: ([0, 0, 0, 0], 0),
        }
    }
}

// ============================================================
// UDP 协议处理
// ============================================================

/// 发送 UDP 数据包
pub fn send_packet(src_port: u16, dst_port: u16, data: &[u8]) -> KernelResult<usize> {
    // 创建 UDP 包头
    let _header = UdpHeader::new(src_port, dst_port, data.len() as u16);

    // TODO: 将 UDP 包交给 IP 层发送
    // TODO: 查询目标 IP 地址（需要上层提供）
    // 暂时返回成功
    Ok(data.len())
}

/// 接收 UDP 数据包
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    let header = UdpHeader::from_bytes(data)?;

    // 验证包的完整性
    if data.len() < header.length() as usize {
        return Err(KernelError::InvalidArgument);
    }

    // TODO: 查询本地 UDP 套接字表
    // TODO: 将数据存入接收缓冲区
    // TODO: 通知应用层

    Ok(())
}

// ============================================================
// UDP 自测
// ============================================================

pub fn selftest() -> bool {
    // UDP 包头创建
    let header = UdpHeader::new(12345, 80, 10);
    assert_eq!(header.src_port(), 12345, "源端口不正确");
    assert_eq!(header.dst_port(), 80, "目标端口不正确");
    assert_eq!(header.length(), 18, "长度不正确 (8 + 10)");

    // 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = UdpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("UDP 包头解析失败");
    });
    assert_eq!(parsed.src_port(), header.src_port(), "解析后源端口不匹配");
    assert_eq!(parsed.dst_port(), header.dst_port(), "解析后目标端口不匹配");

    // UDP 套接字创建
    let socket = UdpSocket::new([192, 168, 1, 10], 5353);
    assert_eq!(socket.local_port, 5353, "本地端口不正确");

    true
}
