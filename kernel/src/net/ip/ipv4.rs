// ============================================================
// IPv4 协议实现
// ============================================================
// 实现 RFC 791 IPv4 协议
//
// IPv4 包头结构（20 字节最小）：
//   [Ver(4) | IHL(4) | DSCP(6) | ECN(2) | Length(16) | ID(16) |
//    Flags(3) | FragOffset(13) | TTL(8) | Protocol(8) | Checksum(16) |
//    SrcIP(32) | DstIP(32) | Options(0-40)]
//
// 为什么分离 ipv4 模块：
// - IPv4 和 IPv6 是独立的协议族，虽有相似但不兼容
// - 将它们分离便于各自迭代开发
// - 对于仅支持 IPv4 的轻量级系统可选择性编译

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use super::super::types::Ipv4Address;

// ============================================================
// IPv4 包头结构
// ============================================================

/// IPv4 包头（最小 20 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    /// 版本(4) + 首部长度(4)
    pub version_ihl: u8,
    /// 服务类型
    pub dscp_ecn: u8,
    /// 总长度（大端）
    pub total_length: [u8; 2],
    /// 标识（大端）
    pub id: [u8; 2],
    /// 标志(3) + 分片偏移(13)（大端）
    pub flags_fragment_offset: [u8; 2],
    /// 生存期（跳数限制）
    pub ttl: u8,
    /// 协议
    pub protocol: u8,
    /// 首部校验和（大端）
    pub header_checksum: [u8; 2],
    /// 源 IP 地址
    pub src_ip: [u8; 4],
    /// 目标 IP 地址
    pub dst_ip: [u8; 4],
}

impl Ipv4Header {
    /// 创建新的 IPv4 包头
    pub fn new(
        src_ip: Ipv4Address,
        dst_ip: Ipv4Address,
        protocol: u8,
        payload_len: u16,
    ) -> Self {
        let total_length = (20u16 + payload_len).to_be_bytes();

        let mut header = Ipv4Header {
            version_ihl: 0x45,  // 版本 4，首部长度 5（20 字节）
            dscp_ecn: 0,
            total_length,
            id: 0u16.to_be_bytes(),
            flags_fragment_offset: 0u16.to_be_bytes(),
            ttl: 64,
            protocol,
            header_checksum: [0, 0],
            src_ip: *src_ip.as_bytes(),
            dst_ip: *dst_ip.as_bytes(),
        };

        // 计算校验和
        header.header_checksum = Self::checksum(&header.to_bytes()[0..20]);

        header
    }

    /// 获取版本号
    pub fn version(&self) -> u8 {
        (self.version_ihl >> 4) & 0x0F
    }

    /// 获取首部长度（32 位字为单位）
    pub fn ihl(&self) -> u8 {
        self.version_ihl & 0x0F
    }

    /// 获取首部长度（字节）
    pub fn header_length(&self) -> usize {
        (self.ihl() as usize) * 4
    }

    /// 获取总长度
    pub fn total_length(&self) -> u16 {
        u16::from_be_bytes(self.total_length)
    }

    /// 获取负载长度
    pub fn payload_length(&self) -> u16 {
        self.total_length() - (self.header_length() as u16)
    }

    /// 获取标识
    pub fn id(&self) -> u16 {
        u16::from_be_bytes(self.id)
    }

    /// 获取标志和分片偏移
    pub fn flags_fragment_offset(&self) -> u16 {
        u16::from_be_bytes(self.flags_fragment_offset)
    }

    /// 获取标志（3 位）
    pub fn flags(&self) -> u8 {
        ((self.flags_fragment_offset[0] >> 5) & 0x07) as u8
    }

    /// 获取分片偏移（13 位）
    pub fn fragment_offset(&self) -> u16 {
        u16::from_be_bytes(self.flags_fragment_offset) & 0x1FFF
    }

    /// 检查 DF（不分片）标志
    pub fn is_dont_fragment(&self) -> bool {
        (self.flags() & 0x04) != 0
    }

    /// 检查 MF（更多分片）标志
    pub fn is_more_fragments(&self) -> bool {
        (self.flags() & 0x02) != 0
    }

    /// 获取 TTL
    pub fn ttl(&self) -> u8 {
        self.ttl
    }

    /// 获取协议号
    pub fn protocol(&self) -> u8 {
        self.protocol
    }

    /// 获取源 IP 地址
    pub fn src_ip(&self) -> Ipv4Address {
        Ipv4Address::from_bytes(self.src_ip)
    }

    /// 获取目标 IP 地址
    pub fn dst_ip(&self) -> Ipv4Address {
        Ipv4Address::from_bytes(self.dst_ip)
    }

    /// 验证校验和
    pub fn verify_checksum(&self) -> bool {
        let mut header_copy = *self;
        header_copy.header_checksum = [0, 0];
        let calculated = Self::checksum(&header_copy.to_bytes()[0..20]);
        self.header_checksum == calculated
    }

    /// 计算 IPv4 首部校验和（16 位补数和）
    pub fn checksum(data: &[u8]) -> [u8; 2] {
        let mut sum: u32 = 0;

        // 每次处理 2 字节
        let mut i = 0;
        while i < data.len() {
            if i + 1 < data.len() {
                let word = u16::from_be_bytes([data[i], data[i + 1]]);
                sum += word as u32;
                i += 2;
            } else {
                // 最后只有 1 字节（不应该发生，但处理一下）
                sum += (data[i] as u32) << 8;
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

    /// 从字节数组解析 IPv4 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 20 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const Ipv4Header)
        };

        // 验证版本
        if header.version() != 4 {
            return Err(KernelError::InvalidArgument);
        }

        Ok(header)
    }

    /// 转换为字节数组（仅首部）
    pub fn to_bytes(&self) -> [u8; 20] {
        let mut bytes = [0u8; 20];
        bytes[0] = self.version_ihl;
        bytes[1] = self.dscp_ecn;
        bytes[2..4].copy_from_slice(&self.total_length);
        bytes[4..6].copy_from_slice(&self.id);
        bytes[6..8].copy_from_slice(&self.flags_fragment_offset);
        bytes[8] = self.ttl;
        bytes[9] = self.protocol;
        bytes[10..12].copy_from_slice(&self.header_checksum);
        bytes[12..16].copy_from_slice(&self.src_ip);
        bytes[16..20].copy_from_slice(&self.dst_ip);
        bytes
    }
}

// ============================================================
// IPv4 包处理
// ============================================================

/// 发送 IPv4 包
pub fn send_packet(_dest_ip: &[u8], _protocol: u8, data: &[u8]) -> KernelResult<usize> {
    if _dest_ip.len() != 4 {
        return Err(KernelError::InvalidArgument);
    }

    let _dest = Ipv4Address::from_bytes([_dest_ip[0], _dest_ip[1], _dest_ip[2], _dest_ip[3]]);

    // TODO: 查询本机 IPv4 地址
    // TODO: 查询默认网关
    // TODO: 通过链路层发送

    // 占位符：返回成功
    Ok(data.len())
}

/// 接收 IPv4 包
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    let header = Ipv4Header::from_bytes(data)?;

    // 验证校验和
    if !header.verify_checksum() {
        return Err(KernelError::InvalidArgument);
    }

    // 检查 TTL
    if header.ttl() == 0 {
        // TODO: 发送 ICMP 超时
        return Err(KernelError::Other("TTL expired"));
    }

    // TODO: 查询路由表
    // TODO: 处理分片
    // TODO: 分发给上层协议

    Ok(())
}

// ============================================================
// IPv4 自测
// ============================================================

pub fn selftest() -> bool {
    // IPv4 包头创建
    let src = Ipv4Address::from_parts(192, 168, 1, 10);
    let dst = Ipv4Address::from_parts(8, 8, 8, 8);

    let header = Ipv4Header::new(src, dst, 6, 100);  // 协议 6 = TCP

    assert_eq!(header.version(), 4, "IP 版本不正确");
    assert_eq!(header.src_ip(), src, "源 IP 不匹配");
    assert_eq!(header.dst_ip(), dst, "目标 IP 不匹配");
    assert_eq!(header.protocol(), 6, "协议号不匹配");
    assert!(header.verify_checksum(), "校验和验证失败");

    // 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = Ipv4Header::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("IPv4 包头解析失败");
    });

    assert_eq!(parsed.src_ip(), src, "解析后源 IP 不匹配");
    assert_eq!(parsed.dst_ip(), dst, "解析后目标 IP 不匹配");

    true
}
