// ============================================================
// 网络栈配置和参数
// ============================================================
// 定义网络协议栈的编译时常数和运行时配置参数

/// 最大传输单元（MTU）
/// 标准以太网 MTU 为 1500 字节；QEMU 虚拟环境可支持更大的 MTU
pub const MTU: usize = 1500;

/// IP 包最大长度（不包括以太网头）
pub const IP_MAX_LENGTH: usize = MTU - 14;  // 14 = 以太网头长度

/// 网络缓冲区池大小（预分配的缓冲区数量）
pub const NET_BUFFER_POOL_SIZE: usize = 256;

// ============================================================
// 以太网配置
// ============================================================

/// 以太网帧类型（EtherType）
pub mod ethertype {
    pub const IPV4: u16 = 0x0800;  // IPv4
    pub const IPV6: u16 = 0x86DD;  // IPv6
    pub const ARP: u16 = 0x0806;   // ARP
}

/// 以太网帧头长度
pub const ETH_HEADER_LEN: usize = 14;
/// 以太网地址长度
pub const ETH_ADDR_LEN: usize = 6;

// ============================================================
// ARP 配置
// ============================================================

/// ARP 缓存条目超时时间（秒）
pub const ARP_CACHE_TIMEOUT: u32 = 600;  // 10 分钟

/// ARP 表最大条目数
pub const ARP_TABLE_MAX_ENTRIES: usize = 128;

// ============================================================
// IP 层配置
// ============================================================

/// IPv4 地址长度（字节）
pub const IPV4_ADDR_LEN: usize = 4;
/// IPv6 地址长度（字节）
pub const IPV6_ADDR_LEN: usize = 16;

/// IP 包头最小长度
pub const IP_MIN_HEADER_LEN: usize = 20;  // IPv4

/// IP 分片重组超时（秒）
pub const IP_FRAGMENT_TIMEOUT: u32 = 15;

/// IP 分片表最大条目数
pub const IP_FRAGMENT_TABLE_MAX: usize = 64;

/// 路由表最大条目数
pub const ROUTING_TABLE_MAX_ENTRIES: usize = 256;

// ============================================================
// ICMP 配置
// ============================================================

/// ICMP echo 请求超时（秒）
pub const ICMP_ECHO_TIMEOUT: u32 = 3;

// ============================================================
// UDP 配置
// ============================================================

/// UDP 包头长度
pub const UDP_HEADER_LEN: usize = 8;

/// UDP 套接字表最大大小
pub const UDP_SOCKET_MAX: usize = 512;

/// UDP 接收缓冲区大小（字节）
pub const UDP_RX_BUFFER_SIZE: usize = 64 * 1024;  // 64KB

// ============================================================
// TCP 配置
// ============================================================

/// TCP 包头最小长度
pub const TCP_MIN_HEADER_LEN: usize = 20;

/// TCP 连接表最大大小
pub const TCP_CONNECTION_MAX: usize = 256;

/// TCP 连接超时（秒）
pub const TCP_CONNECTION_TIMEOUT: u32 = 300;

/// TCP 重传超时（毫秒）
pub const TCP_RETRANSMIT_TIMEOUT: u32 = 1000;

/// TCP 最大重传次数
pub const TCP_MAX_RETRANSMIT: usize = 5;

/// TCP 接收窗口大小（字节）
pub const TCP_RX_WINDOW_SIZE: u32 = 65535;

/// TCP 接收缓冲区大小（字节）
pub const TCP_RX_BUFFER_SIZE: usize = 256 * 1024;  // 256KB

// ============================================================
// Socket 配置
// ============================================================

/// 监听队列最大大小（backlog）
pub const SOCKET_BACKLOG_MAX: usize = 128;

/// 套接字表最大大小
pub const SOCKET_TABLE_MAX: usize = 1024;

// ============================================================
// io_uring 配置
// ============================================================

/// io_uring 提交队列大小
pub const URING_SQ_SIZE: usize = 256;

/// io_uring 完成队列大小
pub const URING_CQ_SIZE: usize = 512;

// ============================================================
// 协议常数
// ============================================================

pub mod ip_protocol {
    /// ICMP 协议号
    pub const ICMP: u8 = 1;
    /// TCP 协议号
    pub const TCP: u8 = 6;
    /// UDP 协议号
    pub const UDP: u8 = 17;
    /// ICMPv6 协议号
    pub const ICMPV6: u8 = 58;
}

/// TCP 控制位标志
pub mod tcp_flags {
    pub const FIN: u8 = 0x01;
    pub const SYN: u8 = 0x02;
    pub const RST: u8 = 0x04;
    pub const PSH: u8 = 0x08;
    pub const ACK: u8 = 0x10;
    pub const URG: u8 = 0x20;
}

/// TCP 连接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    /// 监听状态
    Listen,
    /// 建立中（已发 SYN）
    SynSent,
    /// 建立中（已收 SYN）
    SynRecvd,
    /// 已建立连接
    Established,
    /// 主动关闭（已发 FIN）
    FinWait1,
    /// 主动关闭（已收 ACK）
    FinWait2,
    /// 被动关闭（已收 FIN）
    CloseWait,
    /// 被动关闭（已发 FIN）
    LastAck,
    /// 等待远端关闭
    TimeWait,
    /// 已关闭
    Closed,
}

impl core::fmt::Display for TcpState {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Listen => write!(f, "LISTEN"),
            Self::SynSent => write!(f, "SYN_SENT"),
            Self::SynRecvd => write!(f, "SYN_RCVD"),
            Self::Established => write!(f, "ESTABLISHED"),
            Self::FinWait1 => write!(f, "FIN_WAIT_1"),
            Self::FinWait2 => write!(f, "FIN_WAIT_2"),
            Self::CloseWait => write!(f, "CLOSE_WAIT"),
            Self::LastAck => write!(f, "LAST_ACK"),
            Self::TimeWait => write!(f, "TIME_WAIT"),
            Self::Closed => write!(f, "CLOSED"),
        }
    }
}
