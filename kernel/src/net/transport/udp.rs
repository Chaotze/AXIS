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

/// UDP 套接字状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpSocketState {
    /// 已创建，未绑定
    Created,
    /// 已绑定本地地址和端口
    Bound,
    /// 已关闭
    Closed,
}

/// UDP 套接字
/// 为什么使用 Vec 而非固定缓冲区：灵活支持不同大小的数据包
#[derive(Debug, Clone)]
pub struct UdpSocket {
    /// 本地端口
    pub local_port: u16,
    /// 绑定的接口 IP（0.0.0.0 表示任意接口）
    pub local_ip: [u8; 4],
    /// 套接字状态
    pub state: UdpSocketState,
    /// 接收缓冲区（存储接收到的数据）
    pub recv_buffer: alloc::vec::Vec<u8>,
    /// 最后一个包的源地址（IP + 端口）
    pub last_src_addr: ([u8; 4], u16),
}

impl UdpSocket {
    /// 创建新的 UDP 套接字
    /// 初始状态为 Created，需要先 bind() 才能使用
    pub fn new() -> Self {
        UdpSocket {
            local_port: 0,
            local_ip: [0, 0, 0, 0],
            state: UdpSocketState::Created,
            recv_buffer: alloc::vec::Vec::new(),
            last_src_addr: ([0, 0, 0, 0], 0),
        }
    }

    /// 绑定到指定的本地地址和端口
    /// 为什么需要 bind：建立套接字和本地地址的关联，便于接收时查表
    pub fn bind(&mut self, local_ip: [u8; 4], local_port: u16) -> KernelResult<()> {
        if self.state != UdpSocketState::Created {
            return Err(KernelError::InvalidArgument);
        }

        // 检查端口是否有效（1-65535）
        if local_port == 0 {
            return Err(KernelError::InvalidArgument);
        }

        self.local_ip = local_ip;
        self.local_port = local_port;
        self.state = UdpSocketState::Bound;
        Ok(())
    }

    /// 关闭套接字
    pub fn close(&mut self) {
        self.state = UdpSocketState::Closed;
        self.recv_buffer.clear();
    }

    /// 检查套接字是否已绑定
    pub fn is_bound(&self) -> bool {
        self.state == UdpSocketState::Bound
    }

    /// 存入接收到的数据
    /// 为什么简化为一个包：第一版本的简化设计，后续可扩展为循环缓冲区
    pub fn store_received_data(&mut self, src_addr: ([u8; 4], u16), data: &[u8]) {
        self.last_src_addr = src_addr;
        self.recv_buffer.clear();
        self.recv_buffer.extend_from_slice(data);
    }

    /// 获取接收到的数据
    pub fn get_received_data(&self) -> &[u8] {
        &self.recv_buffer
    }

    /// 获取最后一个包的源地址
    pub fn get_last_src_addr(&self) -> ([u8; 4], u16) {
        self.last_src_addr
    }
}

impl Default for UdpSocket {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// UDP 协议处理
// ============================================================

/// 发送 UDP 数据包
/// 为什么分离发送：便于上层协议（应用层）直接调用
pub fn send_packet(
    src_ip: [u8; 4],
    src_port: u16,
    dst_ip: [u8; 4],
    dst_port: u16,
    data: &[u8],
) -> KernelResult<usize> {
    // 验证端口号
    if src_port == 0 || dst_port == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // 创建 UDP 包头
    let header = UdpHeader::new(src_port, dst_port, data.len() as u16);

    // 验证总长度不超过限制
    let total_len = 8 + data.len();
    if total_len > crate::net::config::MTU {
        return Err(KernelError::InvalidArgument);
    }

    // TODO: 调用 IP 层发送
    // 当前返回成功（占位符）
    let _ = (src_ip, dst_ip, header);
    Ok(data.len())
}

/// 接收 UDP 数据包（由IP层调用）
/// 为什么需要此函数：IP层识别协议号后分发给UDP处理
pub fn recv_packet(
    src_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    data: &[u8],
) -> KernelResult<()> {
    // 解析UDP包头
    let header = UdpHeader::from_bytes(data)?;

    // 验证包的完整性
    if data.len() < header.length() as usize {
        return Err(KernelError::InvalidArgument);
    }

    // 验证端口号匹配
    if header.dst_port() != dst_port {
        return Err(KernelError::InvalidArgument);
    }

    // 获取载荷数据（跳过8字节的UDP头）
    let payload_len = header.length() as usize - 8;
    if data.len() < 8 + payload_len {
        return Err(KernelError::InvalidArgument);
    }

    let _payload = &data[8..8 + payload_len];

    // TODO: 查询本地 UDP 套接字表（SOCKET_TABLE）
    // TODO: 将数据存入对应套接字的接收缓冲区
    // TODO: 通知应用层有数据可读（设置事件标志）

    let _ = (src_ip, src_port);
    Ok(())
}

// ============================================================
// UDP 套接字表管理
// ============================================================

/// UDP 套接字表项（用于管理多个UDP套接字）
/// 为什么需要套接字表：在网络栈中维护全局的套接字映射
pub struct UdpSocketEntry {
    /// 套接字文件描述符（或ID）
    pub fd: u32,
    /// 套接字本身
    pub socket: UdpSocket,
}

// ============================================================
// UDP 自测
// ============================================================

pub fn selftest() -> bool {
    // 1. UDP 包头创建和验证
    let header = UdpHeader::new(12345, 80, 100);
    assert_eq!(header.src_port(), 12345, "源端口不正确");
    assert_eq!(header.dst_port(), 80, "目标端口不正确");
    assert_eq!(header.length(), 108, "长度不正确 (8 + 100)");

    // 2. 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = UdpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("UDP 包头解析失败");
    });
    assert_eq!(parsed.src_port(), header.src_port(), "解析后源端口不匹配");
    assert_eq!(parsed.dst_port(), header.dst_port(), "解析后目标端口不匹配");
    assert_eq!(parsed.length(), header.length(), "解析后长度不匹配");

    // 3. UDP 套接字创建
    let mut socket = UdpSocket::new();
    assert_eq!(socket.state, UdpSocketState::Created, "初始状态应为 Created");
    assert!(!socket.is_bound(), "新套接字不应已绑定");

    // 4. UDP 套接字绑定
    socket.bind([192, 168, 1, 10], 5353).unwrap_or_else(|_| {
        panic!("绑定套接字失败");
    });
    assert_eq!(socket.local_port, 5353, "本地端口不正确");
    assert_eq!(socket.local_ip, [192, 168, 1, 10], "本地 IP 不正确");
    assert_eq!(socket.state, UdpSocketState::Bound, "绑定后状态应为 Bound");
    assert!(socket.is_bound(), "绑定后应返回 true");

    // 5. 端口号验证（0 为无效）
    let mut invalid_socket = UdpSocket::new();
    let result = invalid_socket.bind([127, 0, 0, 1], 0);
    assert!(result.is_err(), "端口 0 应被拒绝");

    // 6. 接收数据存储测试
    let mut rx_socket = UdpSocket::new();
    let src_addr = ([10, 0, 0, 1], 12345);
    let test_data = b"Hello UDP";
    rx_socket.store_received_data(src_addr, test_data);
    assert_eq!(rx_socket.get_received_data(), test_data, "接收数据不匹配");
    assert_eq!(rx_socket.get_last_src_addr(), src_addr, "源地址不匹配");

    // 7. 套接字关闭
    socket.close();
    assert_eq!(socket.state, UdpSocketState::Closed, "关闭后状态应为 Closed");
    assert!(socket.recv_buffer.is_empty(), "关闭后缓冲区应清空");

    true
}
