// ============================================================
// TCP 协议（Transmission Control Protocol）
// ============================================================
// 实现 RFC 793 TCP 协议
//
// TCP 包头结构（20 字节最小）：
//   [SrcPort(16) | DstPort(16) | SeqNum(32) | AckNum(32) |
//    DataOffset(4) | Reserved(3) | Flags(9) | Window(16) | Checksum(16) |
//    UrgPtr(16) | Options(0-40)]
//
// TCP 状态机：
//   CLOSED → LISTEN → (SYN_RCVD → ESTABLISHED)
//   CLOSED → SYN_SENT → ESTABLISHED
//   ESTABLISHED → FIN_WAIT_1 / CLOSE_WAIT → ... → CLOSED

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use crate::net::config::{tcp_flags, TcpState};

// ============================================================
// TCP 包头
// ============================================================

/// TCP 包头（最小 20 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TcpHeader {
    /// 源端口（大端）
    pub src_port: [u8; 2],
    /// 目标端口（大端）
    pub dst_port: [u8; 2],
    /// 序列号（大端）
    pub seq_num: [u8; 4],
    /// 确认号（大端）
    pub ack_num: [u8; 4],
    /// 数据偏移(4) + 保留(4)
    pub data_offset_reserved: u8,
    /// 控制标志
    pub flags: u8,
    /// 窗口大小（大端）
    pub window: [u8; 2],
    /// 校验和（大端）
    pub checksum: [u8; 2],
    /// 紧急指针（大端）
    pub urgent_ptr: [u8; 2],
}

impl TcpHeader {
    /// 创建新的 TCP 包头
    pub fn new(src_port: u16, dst_port: u16, seq_num: u32, flags: u8) -> Self {
        TcpHeader {
            src_port: src_port.to_be_bytes(),
            dst_port: dst_port.to_be_bytes(),
            seq_num: seq_num.to_be_bytes(),
            ack_num: [0, 0, 0, 0],
            data_offset_reserved: 0x50,  // 数据偏移 5 (20 字节)
            flags,
            window: [0xFF, 0xFF],  // 最大窗口
            checksum: [0, 0],
            urgent_ptr: [0, 0],
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

    /// 获取序列号
    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(self.seq_num)
    }

    /// 获取确认号
    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }

    /// 获取数据偏移（32 位字为单位）
    pub fn data_offset(&self) -> u8 {
        (self.data_offset_reserved >> 4) & 0x0F
    }

    /// 获取包头长度（字节）
    pub fn header_length(&self) -> usize {
        (self.data_offset() as usize) * 4
    }

    /// 获取窗口大小
    pub fn window(&self) -> u16 {
        u16::from_be_bytes(self.window)
    }

    /// 检查 SYN 标志
    pub fn has_syn(&self) -> bool {
        (self.flags & tcp_flags::SYN) != 0
    }

    /// 检查 ACK 标志
    pub fn has_ack(&self) -> bool {
        (self.flags & tcp_flags::ACK) != 0
    }

    /// 检查 FIN 标志
    pub fn has_fin(&self) -> bool {
        (self.flags & tcp_flags::FIN) != 0
    }

    /// 检查 RST 标志
    pub fn has_rst(&self) -> bool {
        (self.flags & tcp_flags::RST) != 0
    }

    /// 从字节数组解析 TCP 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 20 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const TcpHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组（仅首部）
    pub fn to_bytes(&self) -> [u8; 20] {
        let mut bytes = [0u8; 20];
        bytes[0..2].copy_from_slice(&self.src_port);
        bytes[2..4].copy_from_slice(&self.dst_port);
        bytes[4..8].copy_from_slice(&self.seq_num);
        bytes[8..12].copy_from_slice(&self.ack_num);
        bytes[12] = self.data_offset_reserved;
        bytes[13] = self.flags;
        bytes[14..16].copy_from_slice(&self.window);
        bytes[16..18].copy_from_slice(&self.checksum);
        bytes[18..20].copy_from_slice(&self.urgent_ptr);
        bytes
    }
}

// ============================================================
// TCP 连接管理
// ============================================================

/// TCP 连接
#[derive(Debug, Clone)]
pub struct TcpConnection {
    /// 源 IP
    pub src_ip: [u8; 4],
    /// 目标 IP
    pub dst_ip: [u8; 4],
    /// 源端口
    pub src_port: u16,
    /// 目标端口
    pub dst_port: u16,
    /// 连接状态
    pub state: TcpState,
    /// 发送序列号
    pub snd_seq: u32,
    /// 接收序列号
    pub rcv_seq: u32,
    /// 接收缓冲区
    pub recv_buffer: alloc::vec::Vec<u8>,
    /// 发送缓冲区
    pub send_buffer: alloc::vec::Vec<u8>,
}

impl TcpConnection {
    /// 创建新的 TCP 连接
    pub fn new(
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Self {
        TcpConnection {
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            state: TcpState::Closed,
            snd_seq: 0,  // 应该随机初始化
            rcv_seq: 0,
            recv_buffer: alloc::vec::Vec::new(),
            send_buffer: alloc::vec::Vec::new(),
        }
    }

    /// 获取连接标识（用于查找）
    pub fn tuple(&self) -> ([u8; 4], u16, [u8; 4], u16) {
        (self.src_ip, self.src_port, self.dst_ip, self.dst_port)
    }
}

// ============================================================
// TCP 协议处理
// ============================================================

/// 发送 TCP 数据包
pub fn send_packet(src_port: u16, dst_port: u16, data: &[u8]) -> KernelResult<usize> {
    // 创建 TCP 包头
    let _header = TcpHeader::new(src_port, dst_port, 0, tcp_flags::ACK);

    // TODO: 查询连接表
    // TODO: 将 TCP 包交给 IP 层发送
    // 暂时返回成功
    Ok(data.len())
}

/// 接收 TCP 数据包
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    let _header = TcpHeader::from_bytes(data)?;

    // TODO: 查询连接表（使用四元组）
    // TODO: 根据连接状态和标志位处理（状态机）
    // TODO: 处理数据或发送 ACK

    Ok(())
}

/// 创建 SYN 包（TCP 连接请求）
pub fn create_syn(src_port: u16, dst_port: u16, seq_num: u32) -> TcpHeader {
    TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::SYN)
}

/// 创建 SYN+ACK 包（TCP 连接应答）
pub fn create_synack(src_port: u16, dst_port: u16, seq_num: u32, ack_num: u32) -> TcpHeader {
    let mut header = TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::SYN | tcp_flags::ACK);
    header.ack_num = ack_num.to_be_bytes();
    header
}

/// 创建 ACK 包
pub fn create_ack(src_port: u16, dst_port: u16, seq_num: u32, ack_num: u32) -> TcpHeader {
    let mut header = TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::ACK);
    header.ack_num = ack_num.to_be_bytes();
    header
}

/// 创建 FIN 包（TCP 连接关闭）
pub fn create_fin(src_port: u16, dst_port: u16, seq_num: u32) -> TcpHeader {
    TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::FIN | tcp_flags::ACK)
}

// ============================================================
// TCP 自测
// ============================================================

pub fn selftest() -> bool {
    // TCP 包头创建
    let header = TcpHeader::new(12345, 80, 1000, tcp_flags::SYN);
    assert_eq!(header.src_port(), 12345, "源端口不正确");
    assert_eq!(header.dst_port(), 80, "目标端口不正确");
    assert_eq!(header.seq_num(), 1000, "序列号不正确");
    assert!(header.has_syn(), "SYN 标志未设置");

    // SYN+ACK 包创建
    let synack = create_synack(80, 12345, 2000, 1001);
    assert!(synack.has_syn(), "SYN 标志未设置");
    assert!(synack.has_ack(), "ACK 标志未设置");
    assert_eq!(synack.ack_num(), 1001, "确认号不正确");

    // 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = TcpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("TCP 包头解析失败");
    });
    assert_eq!(parsed.src_port(), header.src_port(), "解析后源端口不匹配");

    // TCP 连接创建
    let conn = TcpConnection::new(
        [192, 168, 1, 10],
        [8, 8, 8, 8],
        12345,
        80,
    );
    assert_eq!(conn.state, TcpState::Closed, "初始状态应为 CLOSED");

    true
}
