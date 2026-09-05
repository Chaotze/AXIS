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
            snd_seq: 1000,  // 简化版，应该随机初始化
            rcv_seq: 0,
            recv_buffer: alloc::vec::Vec::new(),
            send_buffer: alloc::vec::Vec::new(),
        }
    }

    /// 获取连接标识（用于查找）
    /// 为什么用四元组：唯一标识一条TCP连接
    pub fn tuple(&self) -> ([u8; 4], u16, [u8; 4], u16) {
        (self.src_ip, self.src_port, self.dst_ip, self.dst_port)
    }

    /// 主动发起连接（客户端 SYN）
    /// 为什么需要：支持客户端主动发起TCP连接
    pub fn initiate_connection(&mut self) -> KernelResult<()> {
        if self.state != TcpState::Closed {
            return Err(KernelError::InvalidArgument);
        }

        // 转换到 SYN_SENT 状态，等待服务器的 SYN+ACK
        self.state = TcpState::SynSent;
        // snd_seq 已在创建时初始化
        Ok(())
    }

    /// 处理服务器的 SYN+ACK（客户端侧）
    /// 为什么需要：第二次握手的处理
    pub fn handle_synack(&mut self, ack_num: u32, recv_seq: u32) -> KernelResult<()> {
        if self.state != TcpState::SynSent {
            return Err(KernelError::InvalidArgument);
        }

        // 验证 ACK 号是否正确（应该是 snd_seq + 1）
        if ack_num != self.snd_seq + 1 {
            return Err(KernelError::InvalidArgument);
        }

        // 记录对方的初始序列号
        self.rcv_seq = recv_seq;
        // 准备发送 ACK
        self.snd_seq += 1;
        // 转换到 ESTABLISHED 状态
        self.state = TcpState::Established;
        Ok(())
    }

    /// 监听连接（服务器）
    /// 为什么需要：服务器端进入监听状态
    pub fn listen(&mut self) -> KernelResult<()> {
        if self.state != TcpState::Closed {
            return Err(KernelError::InvalidArgument);
        }

        self.state = TcpState::Listen;
        Ok(())
    }

    /// 处理客户端的 SYN（服务器侧）
    /// 为什么需要：第一次握手的处理
    pub fn handle_syn(&mut self, recv_seq: u32) -> KernelResult<()> {
        if self.state != TcpState::Listen {
            return Err(KernelError::InvalidArgument);
        }

        // 记录客户端的初始序列号
        self.rcv_seq = recv_seq;
        // 转换到 SYN_RCVD 状态，准备发送 SYN+ACK
        self.state = TcpState::SynRecvd;
        // snd_seq 已在创建时初始化
        Ok(())
    }

    /// 处理客户端的 ACK（服务器侧）
    /// 为什么需要：第三次握手的处理
    pub fn handle_ack(&mut self, ack_num: u32) -> KernelResult<()> {
        if self.state != TcpState::SynRecvd {
            return Err(KernelError::InvalidArgument);
        }

        // 验证 ACK 号是否正确（应该是 snd_seq + 1）
        if ack_num != self.snd_seq + 1 {
            return Err(KernelError::InvalidArgument);
        }

        // 更新发送序列号
        self.snd_seq += 1;
        // 连接建立
        self.state = TcpState::Established;
        Ok(())
    }

    /// 存储接收到的数据
    pub fn store_received_data(&mut self, data: &[u8]) {
        self.recv_buffer.extend_from_slice(data);
    }

    /// 获取接收缓冲区
    pub fn get_recv_buffer(&self) -> &[u8] {
        &self.recv_buffer
    }

    /// 清空接收缓冲区
    pub fn clear_recv_buffer(&mut self) {
        self.recv_buffer.clear();
    }

    /// 发起连接关闭（FIN）
    pub fn close_connection(&mut self) -> KernelResult<()> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                Ok(())
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                Ok(())
            }
            _ => Err(KernelError::InvalidArgument),
        }
    }

    /// 处理对方的 FIN
    pub fn handle_fin(&mut self) -> KernelResult<()> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::CloseWait;
                Ok(())
            }
            TcpState::FinWait1 | TcpState::FinWait2 => {
                self.state = TcpState::TimeWait;
                Ok(())
            }
            _ => Err(KernelError::InvalidArgument),
        }
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
    // 1. TCP 包头创建
    let header = TcpHeader::new(12345, 80, 1000, tcp_flags::SYN);
    assert_eq!(header.src_port(), 12345, "源端口不正确");
    assert_eq!(header.dst_port(), 80, "目标端口不正确");
    assert_eq!(header.seq_num(), 1000, "序列号不正确");
    assert!(header.has_syn(), "SYN 标志未设置");

    // 2. SYN+ACK 包创建
    let synack = create_synack(80, 12345, 2000, 1001);
    assert!(synack.has_syn(), "SYN 标志未设置");
    assert!(synack.has_ack(), "ACK 标志未设置");
    assert_eq!(synack.ack_num(), 1001, "确认号不正确");

    // 3. 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = TcpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("TCP 包头解析失败");
    });
    assert_eq!(parsed.src_port(), header.src_port(), "解析后源端口不匹配");

    // 4. TCP 连接创建
    let mut conn = TcpConnection::new(
        [192, 168, 1, 10],
        [8, 8, 8, 8],
        12345,
        80,
    );
    assert_eq!(conn.state, TcpState::Closed, "初始状态应为 CLOSED");

    // 5. 客户端三次握手测试
    // 第一步：客户端发起 SYN
    conn.initiate_connection().unwrap_or_else(|_| {
        panic!("initiate_connection 失败");
    });
    assert_eq!(conn.state, TcpState::SynSent, "应转换为 SYN_SENT");

    // 第二步：客户端收到服务器的 SYN+ACK（seq=2000, ack=1001）
    conn.handle_synack(1001, 2000).unwrap_or_else(|_| {
        panic!("handle_synack 失败");
    });
    assert_eq!(conn.state, TcpState::Established, "应转换为 ESTABLISHED");
    assert_eq!(conn.rcv_seq, 2000, "rcv_seq 应为 2000");

    // 6. 服务器三次握手测试
    let mut server_conn = TcpConnection::new(
        [8, 8, 8, 8],
        [192, 168, 1, 10],
        80,
        12345,
    );

    // 第一步：服务器监听
    server_conn.listen().unwrap_or_else(|_| {
        panic!("listen 失败");
    });
    assert_eq!(server_conn.state, TcpState::Listen, "应为 LISTEN");

    // 第二步：服务器收到客户端的 SYN（seq=1000）
    server_conn.handle_syn(1000).unwrap_or_else(|_| {
        panic!("handle_syn 失败");
    });
    assert_eq!(server_conn.state, TcpState::SynRecvd, "应转换为 SYN_RCVD");
    assert_eq!(server_conn.rcv_seq, 1000, "rcv_seq 应为 1000");

    // 第三步：服务器收到客户端的 ACK（ack=snd_seq+1）
    server_conn.handle_ack(server_conn.snd_seq + 1).unwrap_or_else(|_| {
        panic!("handle_ack 失败");
    });
    assert_eq!(server_conn.state, TcpState::Established, "应转换为 ESTABLISHED");

    // 7. 数据存储和接收测试
    let test_data = b"Hello TCP";
    server_conn.store_received_data(test_data);
    assert_eq!(server_conn.get_recv_buffer(), test_data, "接收数据不匹配");

    // 8. 连接关闭测试
    server_conn.close_connection().unwrap_or_else(|_| {
        panic!("close_connection 失败");
    });
    assert_eq!(server_conn.state, TcpState::FinWait1, "应转换为 FIN_WAIT_1");

    true
}
