// ============================================================
// 以太网协议（Ethernet）
// ============================================================
// 实现 IEEE 802.3 以太网帧的封装和解析
//
// 以太网帧结构：
//   [DA(6) | SA(6) | Type(2) | Payload(46-1500) | FCS(4)]
//   DA: 目标 MAC 地址
//   SA: 源 MAC 地址
//   Type: EtherType（协议类型，如 0x0800 for IPv4）
//   FCS: 帧校验序列（通常由硬件处理，不在内核检查）
//
// 为什么分离 ethernet 模块：
// - MAC 地址和帧格式在所有上层协议中都需使用
// - 以太网头处理和 MAC 地址解析相对独立
// - 便于单元测试验证帧的正确性

use alloc::vec::Vec;
use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use super::super::types::MacAddress;

/// 以太网帧头（14 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    /// 目标 MAC 地址
    pub dest_mac: [u8; 6],
    /// 源 MAC 地址
    pub src_mac: [u8; 6],
    /// EtherType（大端字节序）
    pub ether_type: [u8; 2],
}

impl EthernetHeader {
    /// 创建新的以太网帧头
    pub fn new(dest_mac: MacAddress, src_mac: MacAddress, ether_type: u16) -> Self {
        EthernetHeader {
            dest_mac: *dest_mac.as_bytes(),
            src_mac: *src_mac.as_bytes(),
            ether_type: ether_type.to_be_bytes(),
        }
    }

    /// 获取目标 MAC 地址
    pub fn dest_mac(&self) -> MacAddress {
        MacAddress::from_bytes(self.dest_mac)
    }

    /// 获取源 MAC 地址
    pub fn src_mac(&self) -> MacAddress {
        MacAddress::from_bytes(self.src_mac)
    }

    /// 获取 EtherType（大端转换）
    pub fn ether_type(&self) -> u16 {
        u16::from_be_bytes(self.ether_type)
    }

    /// 从字节数组解析帧头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < core::mem::size_of::<EthernetHeader>() {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const EthernetHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组
    pub fn to_bytes(&self) -> [u8; 14] {
        let mut bytes = [0u8; 14];
        bytes[0..6].copy_from_slice(&self.dest_mac);
        bytes[6..12].copy_from_slice(&self.src_mac);
        bytes[12..14].copy_from_slice(&self.ether_type);
        bytes
    }
}

// ============================================================
// 以太网帧结构
// ============================================================

/// 完整的以太网帧
pub struct EthernetFrame {
    /// 帧头
    pub header: EthernetHeader,
    /// 负载数据
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    /// 创建新的以太网帧
    pub fn new(header: EthernetHeader, payload: Vec<u8>) -> Self {
        EthernetFrame { header, payload }
    }

    /// 从字节流解析帧
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 14 {
            return Err(KernelError::InvalidArgument);
        }

        let header = EthernetHeader::from_bytes(data)?;
        let payload = data[14..].to_vec();

        Ok(EthernetFrame { header, payload })
    }

    /// 将帧序列化为字节流
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.header.to_bytes().to_vec();
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// 获取帧的总长度
    pub fn len(&self) -> usize {
        14 + self.payload.len()
    }

    /// 检查帧是否为空
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

// ============================================================
// 以太网工具函数
// ============================================================

/// 在现有缓冲区中写入以太网帧头
pub fn write_header_at(
    buffer: &mut [u8],
    offset: usize,
    dest_mac: MacAddress,
    src_mac: MacAddress,
    ether_type: u16,
) -> KernelResult<()> {
    if buffer.len() < offset + 14 {
        return Err(KernelError::InvalidArgument);
    }

    let header = EthernetHeader::new(dest_mac, src_mac, ether_type);
    let header_bytes = header.to_bytes();
    buffer[offset..offset + 14].copy_from_slice(&header_bytes);

    Ok(())
}

/// 从缓冲区中读取以太网帧头
pub fn read_header_at(buffer: &[u8], offset: usize) -> KernelResult<EthernetHeader> {
    if buffer.len() < offset + 14 {
        return Err(KernelError::InvalidArgument);
    }

    EthernetHeader::from_bytes(&buffer[offset..])
}

// ============================================================
// 以太网自测
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address() {
        let mac = MacAddress::from_bytes([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        assert_eq!(mac.as_bytes(), &[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        assert!(!mac.is_broadcast());
        assert!(!mac.is_multicast());

        let bcast = MacAddress::broadcast();
        assert!(bcast.is_broadcast());

        let mcast = MacAddress::from_bytes([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
        assert!(mcast.is_multicast());
    }

    #[test]
    fn test_ethernet_header() {
        let dest = MacAddress::from_bytes([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let src = MacAddress::from_bytes([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let header = EthernetHeader::new(dest, src, 0x0800);

        assert_eq!(header.dest_mac(), dest);
        assert_eq!(header.src_mac(), src);
        assert_eq!(header.ether_type(), 0x0800);

        let bytes = header.to_bytes();
        let parsed = EthernetHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.ether_type(), header.ether_type());
    }
}

/// 以太网协议单测入口（内核内调用）
pub fn selftest() -> bool {
    // MAC 地址测试
    let mac1 = MacAddress::from_bytes([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
    let mac2 = MacAddress::broadcast();
    assert!(mac2.is_broadcast(), "广播地址检测失败");
    assert!(!mac1.is_broadcast(), "非广播地址误报");

    // 以太网帧头测试
    let header = EthernetHeader::new(mac1, mac2, 0x0800);
    let bytes = header.to_bytes();
    let parsed = EthernetHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("帧头解析失败");
    });
    assert_eq!(parsed.ether_type(), 0x0800, "EtherType 不匹配");

    // 完整帧测试
    let payload = alloc::vec::Vec::from(&b"Hello, Ethernet!"[..]);
    let frame = EthernetFrame::new(header, payload.clone());
    let serialized = frame.to_bytes();
    let deserialized = EthernetFrame::from_bytes(&serialized).unwrap_or_else(|_| {
        panic!("帧解析失败");
    });
    assert_eq!(deserialized.payload, payload, "负载不匹配");

    true
}
