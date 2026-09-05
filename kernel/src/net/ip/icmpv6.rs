// ============================================================
// ICMPv6 协议（Internet Control Message Protocol for IPv6）
// ============================================================
// 实现 RFC 4443 ICMPv6 协议
//
// ICMPv6 消息类型：
//   Type 1: Destination Unreachable
//   Type 2: Packet Too Big
//   Type 3: Time Exceeded
//   Type 4: Parameter Problem
//   Type 128: Echo Request
//   Type 129: Echo Reply
//   Type 133: Router Solicitation (NDP)
//   Type 134: Router Advertisement (NDP)
//   Type 135: Neighbor Solicitation (NDP)
//   Type 136: Neighbor Advertisement (NDP)

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use super::super::types::Ipv6Address;

// ============================================================
// ICMPv6 消息类型
// ============================================================

/// ICMPv6 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icmpv6Type {
    /// 目标不可达
    DestinationUnreachable = 1,
    /// 数据包过大
    PacketTooBig = 2,
    /// 超时
    TimeExceeded = 3,
    /// 参数问题
    ParameterProblem = 4,
    /// 回显请求（ping）
    EchoRequest = 128,
    /// 回显应答
    EchoReply = 129,
    /// 路由器请求（NDP）
    RouterSolicitation = 133,
    /// 路由器通告（NDP）
    RouterAdvertisement = 134,
    /// 邻居请求（NDP）
    NeighborSolicitation = 135,
    /// 邻居通告（NDP）
    NeighborAdvertisement = 136,
}

impl Icmpv6Type {
    /// 从数值创建类型
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Icmpv6Type::DestinationUnreachable),
            2 => Some(Icmpv6Type::PacketTooBig),
            3 => Some(Icmpv6Type::TimeExceeded),
            4 => Some(Icmpv6Type::ParameterProblem),
            128 => Some(Icmpv6Type::EchoRequest),
            129 => Some(Icmpv6Type::EchoReply),
            133 => Some(Icmpv6Type::RouterSolicitation),
            134 => Some(Icmpv6Type::RouterAdvertisement),
            135 => Some(Icmpv6Type::NeighborSolicitation),
            136 => Some(Icmpv6Type::NeighborAdvertisement),
            _ => None,
        }
    }
}

// ============================================================
// ICMPv6 包头
// ============================================================

/// ICMPv6 包头（8 字节最小）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Icmpv6Header {
    /// 消息类型
    pub msg_type: u8,
    /// 代码
    pub code: u8,
    /// 校验和（大端）
    pub checksum: [u8; 2],
    /// Rest of Header（用途取决于消息类型）
    pub rest: [u8; 4],
}

impl Icmpv6Header {
    /// 创建回显请求（ping 请求）
    pub fn echo_request(sequence: u16, id: u16) -> Self {
        Icmpv6Header {
            msg_type: Icmpv6Type::EchoRequest as u8,
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
        Icmpv6Header {
            msg_type: Icmpv6Type::EchoReply as u8,
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
    pub fn msg_type(&self) -> Option<Icmpv6Type> {
        Icmpv6Type::from_u8(self.msg_type)
    }

    /// 获取序列号（用于 echo 请求/应答）
    pub fn sequence(&self) -> u16 {
        u16::from_be_bytes([self.rest[2], self.rest[3]])
    }

    /// 获取标识符（用于 echo 请求/应答）
    pub fn id(&self) -> u16 {
        u16::from_be_bytes([self.rest[0], self.rest[1]])
    }

    /// 从字节数组解析 ICMPv6 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 8 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const Icmpv6Header)
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
// ICMPv6 处理
// ============================================================

/// 处理接收到的 ICMPv6 包
pub fn handle_icmpv6(
    _src_addr: Ipv6Address,
    _dst_addr: Ipv6Address,
    data: &[u8],
) -> KernelResult<()> {
    let header = Icmpv6Header::from_bytes(data)?;

    match header.msg_type() {
        Some(Icmpv6Type::EchoRequest) => {
            // 处理 ping 请求：应该回复 echo reply
            // TODO: 发送 ICMPv6 回显应答
        }
        Some(Icmpv6Type::EchoReply) => {
            // 处理 ping 应答
            // TODO: 通知应用层
        }
        Some(Icmpv6Type::NeighborSolicitation) => {
            // 处理邻居请求（NDP）
            // TODO: 发送邻居通告
        }
        Some(Icmpv6Type::RouterSolicitation) => {
            // 处理路由器请求
            // TODO: 发送路由器通告
        }
        _ => {
            // 其他 ICMPv6 消息类型暂不处理
        }
    }

    Ok(())
}

// ============================================================
// IPv6 邻居发现（NDP - Neighbor Discovery Protocol）
// ============================================================

/// 邻居请求包（Neighbor Solicitation）
/// 用于地址解析和邻居不可达检测（NUD）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct NeighborSolicitation {
    /// 目标地址（要查找的 IPv6 地址）
    pub target_addr: [u8; 16],
}

impl NeighborSolicitation {
    /// 创建邻居请求包
    pub fn new(target_addr: Ipv6Address) -> Self {
        NeighborSolicitation {
            target_addr: *target_addr.as_bytes(),
        }
    }

    /// 将邻居请求包转换为字节数组
    pub fn to_bytes(&self) -> [u8; 16] {
        self.target_addr
    }
}

/// 邻居通告包（Neighbor Advertisement）
/// 用于响应邻居请求
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct NeighborAdvertisement {
    /// 标志（R、S、O）和目标地址（下面的 4 字节是标志）
    pub flags_and_target: [u8; 20],
}

impl NeighborAdvertisement {
    /// 创建邻居通告包
    /// 参数：
    /// - target_addr: 要通告的 IPv6 地址
    /// - router: 是否是路由器
    /// - solicited: 是否是对请求的应答
    /// - override_: 是否覆盖现有缓存条目
    pub fn new(target_addr: Ipv6Address, router: bool, solicited: bool, override_: bool) -> Self {
        let mut flags_and_target = [0u8; 20];

        // 设置标志（第一个字节的高 3 位）
        // R（Router）: bit 7
        // S（Solicited）: bit 6
        // O（Override）: bit 5
        if router {
            flags_and_target[0] |= 0x80;  // R flag
        }
        if solicited {
            flags_and_target[0] |= 0x40;  // S flag
        }
        if override_ {
            flags_and_target[0] |= 0x20;  // O flag
        }

        // 复制目标地址（从字节 4 开始）
        flags_and_target[4..20].copy_from_slice(target_addr.as_bytes());

        NeighborAdvertisement { flags_and_target }
    }

    /// 获取目标地址
    pub fn target_addr(&self) -> Ipv6Address {
        let mut addr = [0u8; 16];
        addr.copy_from_slice(&self.flags_and_target[4..20]);
        Ipv6Address::from_bytes(addr)
    }

    /// 检查是否是路由器
    pub fn is_router(&self) -> bool {
        (self.flags_and_target[0] & 0x80) != 0
    }

    /// 检查是否是对请求的应答
    pub fn is_solicited(&self) -> bool {
        (self.flags_and_target[0] & 0x40) != 0
    }
}

/// 处理邻居请求
/// 为什么需要：对方想知道我们的 MAC 地址
pub fn handle_neighbor_solicitation(
    _src_addr: Ipv6Address,
    _target_addr: Ipv6Address,
) -> NeighborAdvertisement {
    // TODO: 检查目标地址是否是我们的地址
    // TODO: 查找我们的 MAC 地址
    // TODO: 创建并返回邻居通告

    // 当前简化实现：创建一个通告包
    NeighborAdvertisement::new(_target_addr, false, true, true)
}

/// 处理邻居通告
/// 为什么需要：更新邻居缓存中的 MAC 地址映射
pub fn handle_neighbor_advertisement(
    src_addr: Ipv6Address,
    _target_addr: Ipv6Address,
    _mac_addr: [u8; 6],
) -> KernelResult<()> {
    // TODO: 验证目标地址
    // TODO: 查询或创建邻居缓存条目
    // TODO: 更新 MAC 地址映射
    // TODO: 检查是否有待发送的包等待此地址解析

    let _ = src_addr;
    Ok(())
}

pub fn selftest() -> bool {
    // 1. ICMPv6 类型测试
    assert_eq!(Icmpv6Type::EchoRequest as u8, 128, "Echo Request 类型值错误");
    assert_eq!(Icmpv6Type::EchoReply as u8, 129, "Echo Reply 类型值错误");

    // 2. 回显请求创建
    let header = Icmpv6Header::echo_request(1, 0x5678);
    assert_eq!(header.msg_type(), Some(Icmpv6Type::EchoRequest), "ICMPv6 类型不正确");
    assert_eq!(header.sequence(), 1, "序列号不正确");
    assert_eq!(header.id(), 0x5678, "标识符不正确");

    // 3. 回显应答创建
    let reply = Icmpv6Header::echo_reply(1, 0x5678);
    assert_eq!(reply.msg_type(), Some(Icmpv6Type::EchoReply), "ICMPv6 应答类型不正确");

    // 4. 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = Icmpv6Header::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("ICMPv6 包头解析失败");
    });
    assert_eq!(parsed.sequence(), header.sequence(), "解析后序列号不匹配");
    assert_eq!(parsed.id(), header.id(), "解析后标识符不匹配");

    // 5. 所有消息类型测试
    assert!(Icmpv6Type::from_u8(1).is_some(), "DestinationUnreachable 应支持");
    assert!(Icmpv6Type::from_u8(133).is_some(), "RouterSolicitation 应支持");
    assert!(Icmpv6Type::from_u8(135).is_some(), "NeighborSolicitation 应支持");
    assert!(Icmpv6Type::from_u8(255).is_none(), "未知类型应返回 None");

    true
}
