// ============================================================
// MAC 地址
// ============================================================
// 6 字节网卡物理地址的编解码与分类判断。
//
// 纯逻辑设计：不依赖 arch/锁/堆之外的内核设施（仅 alloc），
// 可宿主单元测试。注意：当前工具链的 LTO 构建下，经 trait 对象
// /Vec 转发本结构体返回值时偶发误读（内核自测已规避），
// 宿主测试仍可完整覆盖其语义。

/// 6 字节 MAC 地址
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const fn new(b: [u8; 6]) -> Self {
        Self(b)
    }

    /// 从字节切片构造（长度不足 6 时高位补 0）
    pub fn from_slice(s: &[u8]) -> Self {
        let mut b = [0u8; 6];
        let n = s.len().min(6);
        b[..n].copy_from_slice(&s[..n]);
        Self(b)
    }

    /// 是否为组播地址（bit0 置位）
    pub const fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }

    /// 是否为广播地址（全 FF）
    pub const fn is_broadcast(&self) -> bool {
        self.0[0] == 0xFF && self.0[1] == 0xFF && self.0[2] == 0xFF
            && self.0[3] == 0xFF && self.0[4] == 0xFF && self.0[5] == 0xFF
    }

    /// 格式化（冒号分隔小写，如 52:54:00:12:34:56）
    pub fn to_string(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                s.push(':');
            }
            s.push_str(&alloc::format!("{:02x}", b));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_addr() {
        let mac = MacAddr::from_slice(&[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(mac.to_string(), "52:54:00:12:34:56");
        assert!(!mac.is_multicast());
        assert!(!mac.is_broadcast());

        let bc = MacAddr::new([0xFF; 6]);
        assert!(bc.is_broadcast());

        let mc = MacAddr::new([0x01, 0x00, 0x5E, 0, 0, 1]);
        assert!(mc.is_multicast());
    }
}
