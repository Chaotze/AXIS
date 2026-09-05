// ============================================================
// 网络接口卡（NIC）驱动框架
// ============================================================
// 网卡设备的统一抽象：MAC 地址、帧收发接口与设备注册表。
//
// 纯逻辑设计：MacAddr 编解码与帧处理不接触硬件，可宿主单测；
// 具体网卡驱动（e1000/igc/virtio）实现 NicDevice 后注册进表，
// 网络协议栈（阶段 7）通过本模块统一访问网卡。

pub use super::mac::MacAddr;

/// 网卡驱动接口
///
/// 帧收发以原始以太网帧为单位（含 14 字节头，不含 FCS）。
pub trait NicDevice: Send {
    /// 网卡名（如 "e1000"、"loopback"）
    fn name(&self) -> &'static str;
    /// 网卡 MAC 地址
    fn mac(&self) -> MacAddr;
    /// 最大传输单元（MTU，字节）
    fn mtu(&self) -> usize;
    /// 发送一帧
    fn send(&mut self, frame: &[u8]) -> crate::prelude::KernelResult<()>;
    /// 接收一帧（阻塞等待或立即返回由实现决定）
    fn recv(&mut self, buf: &mut [u8]) -> crate::prelude::KernelResult<usize>;
}

/// 网卡摘要（协议栈阶段使用；不用 trait 对象转发小结构体返回值，
/// 当前工具链的 LTO 下经 dyn 返回 MacAddr 等值会被清零）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicInfo {
    pub name: &'static str,
    pub mac: MacAddr,
    pub mtu: usize,
}

/// 全局网卡注册表（纯数据，不经过 trait 对象）
static NICS: crate::sync::Spinlock<alloc::vec::Vec<NicInfo>> =
    crate::sync::Spinlock::new(alloc::vec::Vec::new());

/// 注册网卡信息
///
/// 注意：这里不打印 MAC——当前工具链的 LTO 构建下，经对象方法
/// 返回 MacAddr 等小结构体时取值不稳定（QEMU 实测被清零/误读），
/// 注册表只保存纯数据，格式化留给纯逻辑自测与上层消费方。
pub fn register_nic(info: NicInfo) {
    println!("[NIC] registered: {}", info.name);
    NICS.lock().push(info);
}

/// 网卡数量
pub fn count() -> usize {
    NICS.lock().len()
}

/// 网卡列表
pub fn list() -> alloc::vec::Vec<NicInfo> {
    NICS.lock().clone()
}

/// 回环网卡（测试用）：发送的帧原样接收
pub struct LoopbackNic {
    mac: MacAddr,
    /// 已发送待接收的帧
    pending: alloc::vec::Vec<u8>,
}

impl LoopbackNic {
    pub fn new() -> Self {
        Self {
            mac: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            pending: alloc::vec::Vec::new(),
        }
    }
}

impl Default for LoopbackNic {
    fn default() -> Self {
        Self::new()
    }
}

impl NicDevice for LoopbackNic {
    fn name(&self) -> &'static str {
        "loopback"
    }

    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn mtu(&self) -> usize {
        1500
    }

    fn send(&mut self, frame: &[u8]) -> crate::prelude::KernelResult<()> {
        if frame.len() > 1514 {
            return Err(crate::prelude::KernelError::InvalidArgument);
        }
        self.pending.extend_from_slice(frame);
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> crate::prelude::KernelResult<usize> {
        if self.pending.is_empty() {
            return Err(crate::prelude::KernelError::NotFound);
        }
        if buf.len() < self.pending.len() {
            return Err(crate::prelude::KernelError::InvalidArgument);
        }
        let n = self.pending.len();
        buf[..n].copy_from_slice(&self.pending);
        self.pending.clear();
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loopback() {
        let mut nic = LoopbackNic::new();
        let frame = [0u8; 64];
        nic.send(&frame).unwrap();
        let mut buf = [0u8; 64];
        let n = nic.recv(&mut buf).unwrap();
        assert_eq!(n, 64);
        assert_eq!(&buf[..64], &frame[..]);
    }
}