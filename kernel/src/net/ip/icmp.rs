// ============================================================
// ICMP 协议（Internet Control Message Protocol）
// ============================================================
// 实现 RFC 792 ICMP 协议
//
// ICMP 消息类型：
//   Type 0: Echo Reply（回显应答）
//   Type 3: Destination Unreachable（目标不可达）
//   Type 8: Echo Request（回显请求 / ping）
//   Type 11: Time Exceeded（超时）
//
// ICMP 包结构：
//   [Type(8) | Code(8) | Checksum(16) | Rest(32) | Data(variable)]

use crate::lib::result::KernelResult;
use super::super::types::Ipv4Address;

// ============================================================
// ICMP 消息类型和代码
// ============================================================

/// ICMP 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmpType {
    /// 回显应答
    EchoReply = 0,
    /// 目标不可达
    DestinationUnreachable = 3,
    /// 回显请求（ping）
    EchoRequest = 8,
    /// 超时
    TimeExceeded = 11,
}

impl IcmpType {
    /// 从数值创建类型
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(IcmpType::EchoReply),
            3 => Some(IcmpType::DestinationUnreachable),
            8 => Some(IcmpType::EchoRequest),
            11 => Some(IcmpType::TimeExceeded),
            _ => None,
        }
    }
}

/// ICMP 目标不可达的代码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestUnreachCode {
    /// 网络不可达
    NetworkUnreachable = 0,
    /// 主机不可达
    HostUnreachable = 1,
    /// 协议不可达
    ProtocolUnreachable = 2,
    /// 端口不可达
    PortUnreachable = 3,
}

// ============================================================
// ICMP 包头
// ============================================================

/// ICMP 包头（8 字节最小）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    /// 消息类型
    pub msg_type: u8,
    /// 代码
    pub code: u8,
    /// 校验和（大端）
    pub checksum: [u8; 2],
    /// Rest of Header（用途取决于消息类型）
    pub rest: [u8; 4],
}

impl IcmpHeader {
    /// 创建回显请求（ping 请求）
    pub fn echo_request(sequence: u16, id: u16) -> Self {
        IcmpHeader {
            msg_type: IcmpType::EchoRequest as u8,
            code: 0,
            checksum: [0, 0],
            rest: {
                let mut rest = [0u8; 4];
                rest[0..2].copy_from_slice(&id.to_be_bytes());
                rest[2..4].copy_from_slice(&sequence.to_be_bytes());
                rest
            },
        }
    }

    /// 创建回显应答（ping 应答）
    pub fn echo_reply(sequence: u16, id: u16) -> Self {
        IcmpHeader {
            msg_type: IcmpType::EchoReply as u8,
            code: 0,
            checksum: [0, 0],
            rest: {
                let mut rest = [0u8; 4];
                rest[0..2].copy_from_slice(&id.to_be_bytes());
                rest[2..4].copy_from_slice(&sequence.to_be_bytes());
                rest
            },
        }
    }

    /// 获取消息类型
    pub fn msg_type(&self) -> Option<IcmpType> {
        IcmpType::from_u8(self.msg_type)
    }

    /// 获取序列号（用于 echo 请求/应答）
    pub fn sequence(&self) -> u16 {
        u16::from_be_bytes([self.rest[2], self.rest[3]])
    }

    /// 获取标识符（用于 echo 请求/应答）
    pub fn id(&self) -> u16 {
        u16::from_be_bytes([self.rest[0], self.rest[1]])
    }

    /// 计算校验和
    pub fn compute_checksum(header: &[u8]) -> [u8; 2] {
        let mut sum: u32 = 0;

        let mut i = 0;
        while i < header.len() {
            if i + 1 < header.len() {
                let word = u16::from_be_bytes([header[i], header[i + 1]]);
                sum += word as u32;
                i += 2;
            } else {
                sum += (header[i] as u32) << 8;
                i += 1;
            }
        }

        // 处理进位
        while (sum >> 16) > 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        // 取反
        let checksum = !sum as u16;
        checksum.to_be_bytes()
    }

    /// 从字节数组解析 ICMP 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 8 {
            return Err(crate::prelude::KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const IcmpHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = self.msg_type;
        bytes[1] = self.code;
        bytes[2..4].copy_from_slice(&self.checksum);
        bytes[4..8].copy_from_slice(&self.rest);
        bytes
    }
}

// ============================================================
// ICMP 处理
// ============================================================

/// 处理接收到的 ICMP 包
/// 为什么需要这个函数：IP层接收到ICMP协议的包后分发到此处理
pub fn handle_icmp(
    src_ip: Ipv4Address,
    _dst_ip: Ipv4Address,
    data: &[u8],
) -> KernelResult<()> {
    let header = IcmpHeader::from_bytes(data)?;

    match header.msg_type() {
        Some(IcmpType::EchoRequest) => {
            // 处理 ping 请求：回复 echo reply
            // 为什么需要回复：这是 ICMP echo 协议的要求
            let reply_header = IcmpHeader::echo_reply(header.sequence(), header.id());

            // 获取 ping 请求的数据（跳过 8 字节的 ICMP 头）
            let payload = if data.len() > 8 {
                &data[8..]
            } else {
                &[]
            };

            // 构建回复包（头 + 原始数据）
            let mut reply_packet = alloc::vec::Vec::new();
            reply_packet.extend_from_slice(&reply_header.to_bytes());
            reply_packet.extend_from_slice(payload);

            // 计算校验和
            let mut header_with_payload = alloc::vec::Vec::new();
            header_with_payload.extend_from_slice(&reply_header.to_bytes());
            header_with_payload.extend_from_slice(payload);
            let checksum = IcmpHeader::compute_checksum(&header_with_payload);

            // 更新校验和到包中
            if reply_packet.len() >= 2 {
                reply_packet[2..4].copy_from_slice(&checksum);
            }

            // 发送 ICMP 回显应答
            // 为什么交换源目地址：应答包需要发给请求者
            let _ = super::ipv4::send_packet(
                src_ip.as_bytes(),
                crate::net::config::ip_protocol::ICMP,
                &reply_packet,
            );
        }
        Some(IcmpType::EchoReply) => {
            // 处理 ping 应答
            // 为什么需要处理：应用层可能在等待 ping 回应
            println!(
                "[ICMP] Echo Reply from {}: id={}, seq={}, bytes={}",
                src_ip,
                header.id(),
                header.sequence(),
                data.len()
            );
            // 后续可扩展为通知应用层（通过事件队列或回调）
        }
        Some(IcmpType::TimeExceeded) => {
            // 处理超时错误
            println!("[ICMP] Time Exceeded from {}", src_ip);
        }
        Some(IcmpType::DestinationUnreachable) => {
            // 处理目标不可达错误
            println!("[ICMP] Destination Unreachable from {}", src_ip);
        }
        None => {
            // 不支持的 ICMP 类型
        }
    }

    Ok(())
}

/// 接收 ICMP 包的公开函数（供 IPv4 调用）
/// 为什么需要这个函数：IPv4层需要一个统一的接收入口
pub fn recv_icmp(
    data: &[u8],
    src_ip: Ipv4Address,
    dst_ip: Ipv4Address,
) -> KernelResult<()> {
    handle_icmp(src_ip, dst_ip, data)
}
/// 为什么需要：当 TTL 为 0 时，需要发送 Time Exceeded 通知给源地址
pub fn create_time_exceeded(original_header: &[u8]) -> (IcmpHeader, alloc::vec::Vec<u8>) {
    let mut header = IcmpHeader {
        msg_type: IcmpType::TimeExceeded as u8,
        code: 0,  // TTL 超时
        checksum: [0, 0],
        rest: [0; 4],
    };

    // 计算校验和
    let mut packet = alloc::vec::Vec::new();
    packet.extend_from_slice(&header.to_bytes());
    // 原始包头（前 28 字节或更少）
    let header_len = original_header.len().min(28);
    packet.extend_from_slice(&original_header[..header_len]);

    header.checksum = IcmpHeader::compute_checksum(&packet);

    // 返回修正后的头和完整包
    let mut result_packet = alloc::vec::Vec::new();
    result_packet.extend_from_slice(&header.to_bytes());
    result_packet.extend_from_slice(&original_header[..header_len]);

    (header, result_packet)
}

/// 创建目标不可达错误通知
pub fn create_destination_unreachable(
    code: u8,
    original_header: &[u8],
) -> (IcmpHeader, alloc::vec::Vec<u8>) {
    let mut header = IcmpHeader {
        msg_type: IcmpType::DestinationUnreachable as u8,
        code,
        checksum: [0, 0],
        rest: [0; 4],
    };

    // 构建响应包
    let mut packet = alloc::vec::Vec::new();
    packet.extend_from_slice(&header.to_bytes());
    let header_len = original_header.len().min(28);
    packet.extend_from_slice(&original_header[..header_len]);

    header.checksum = IcmpHeader::compute_checksum(&packet);

    let mut result_packet = alloc::vec::Vec::new();
    result_packet.extend_from_slice(&header.to_bytes());
    result_packet.extend_from_slice(&original_header[..header_len]);

    (header, result_packet)
}

// ============================================================
// ICMP 自测
// ============================================================

pub fn selftest() -> bool {
    // 创建 ping 请求
    let header = IcmpHeader::echo_request(1, 0x1234);
    assert_eq!(header.msg_type(), Some(IcmpType::EchoRequest), "ICMP 类型不正确");
    assert_eq!(header.sequence(), 1, "序列号不正确");
    assert_eq!(header.id(), 0x1234, "标识符不正确");

    // 创建 ping 应答
    let reply = IcmpHeader::echo_reply(1, 0x1234);
    assert_eq!(reply.msg_type(), Some(IcmpType::EchoReply), "ICMP 应答类型不正确");

    // 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = IcmpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("ICMP 包头解析失败");
    });
    assert_eq!(parsed.sequence(), header.sequence(), "解析后序列号不匹配");

    true
}
