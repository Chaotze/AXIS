// ============================================================
// 网络基础类型定义
// ============================================================
// 定义所有网络协议栈使用的基础类型
//
// 为什么要单独提取基础类型：
// - MAC 地址、IP 地址等在多个协议中都需要使用
// - 集中管理避免重复定义和循环依赖
// - 便于统一的格式化和验证逻辑

use core::fmt;

// ============================================================
// MAC 地址
// ============================================================

/// MAC 地址（6 字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    /// 从字节数组创建 MAC 地址
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        MacAddress(bytes)
    }

    /// 获取字节数组
    pub const fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }

    /// 广播地址
    pub const fn broadcast() -> Self {
        MacAddress([0xFF; 6])
    }

    /// 零地址
    pub const fn zero() -> Self {
        MacAddress([0x00; 6])
    }

    /// 检查是否为广播地址
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 6]
    }

    /// 检查是否为多播地址（LSB of first octet = 1）
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0x01) != 0
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

// ============================================================
// IPv4 地址
// ============================================================

/// IPv4 地址（4 字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv4Address([u8; 4]);

impl Ipv4Address {
    /// 从字节数组创建 IPv4 地址
    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Ipv4Address(bytes)
    }

    /// 获取字节数组
    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// 从四个字节数字创建地址
    pub const fn from_parts(a: u8, b: u8, c: u8, d: u8) -> Self {
        Ipv4Address([a, b, c, d])
    }

    /// 零地址
    pub const fn zero() -> Self {
        Ipv4Address([0, 0, 0, 0])
    }

    /// 广播地址
    pub const fn broadcast() -> Self {
        Ipv4Address([255, 255, 255, 255])
    }

    /// 环回地址
    pub const fn loopback() -> Self {
        Ipv4Address([127, 0, 0, 1])
    }

    /// 检查是否为多播地址（224.0.0.0/4）
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0xF0) == 0xE0
    }

    /// 作为 u32 获取地址（大端字节序）
    pub fn as_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    /// 从 u32 创建地址（大端字节序）
    pub fn from_u32(addr: u32) -> Self {
        Ipv4Address(addr.to_be_bytes())
    }
}

impl fmt::Display for Ipv4Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

// ============================================================
// IPv6 地址
// ============================================================

/// IPv6 地址（128 位）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv6Address([u8; 16]);

impl Ipv6Address {
    /// 从字节数组创建 IPv6 地址
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Ipv6Address(bytes)
    }

    /// 获取字节数组
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// 环回地址
    pub const fn loopback() -> Self {
        Ipv6Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
    }

    /// 未指定地址
    pub const fn unspecified() -> Self {
        Ipv6Address([0; 16])
    }
}

impl fmt::Display for Ipv6Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // 简化显示（完整的 IPv6 格式化逻辑较复杂）
        write!(
            f,
            "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:...",
            self.0[0], self.0[1], self.0[2], self.0[3],
            self.0[4], self.0[5], self.0[6], self.0[7]
        )
    }
}
