// 网络接口卡（NIC）子系统
// ============================================================
// 网卡驱动入口：探测 e1000 / igc / virtio-net，注册测试用
// 回环网卡，并提供网卡清单供网络协议栈使用。
//
// 分层：
//   driver.rs —— NicDevice trait、MAC 地址、注册表、回环网卡（纯逻辑）
//   e1000.rs  —— Intel e1000 硬件驱动
//   igc.rs    —— Intel i225/i226 探测
//   virtio.rs —— VirtIO net 探测
//   mac.rs    —— MAC 地址编解码

pub mod driver;
pub mod e1000;
pub mod igc;
pub mod virtio;
pub mod mac;

use crate::prelude::KernelResult;
use alloc::vec::Vec;
use self::driver::{LoopbackNic, MacAddr, NicDevice};

// ============================================================
// 全局网卡实例管理（供网络栈全局调用）
// ============================================================

/// 全局回环网卡实例（用于网络栈的收发）
static LOOPBACK_NIC: crate::sync::Spinlock<Option<LoopbackNic>> =
    crate::sync::Spinlock::new(None);

/// 发送帧（通过全局网卡）
pub fn send_frame(frame: &[u8]) -> KernelResult<usize> {
    let mut nic_guard = LOOPBACK_NIC.lock();
    if let Some(ref mut nic) = *nic_guard {
        nic.send(frame)?;
        Ok(frame.len())
    } else {
        Err(crate::prelude::KernelError::NotFound)
    }
}

/// 接收帧（通过全局网卡）
pub fn recv_frame() -> KernelResult<Vec<u8>> {
    let mut nic_guard = LOOPBACK_NIC.lock();
    if let Some(ref mut nic) = *nic_guard {
        let mut buf = Vec::with_capacity(1500);
        // 初始化缓冲区
        buf.resize(1500, 0);
        match nic.recv(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) => Err(e),
        }
    } else {
        Err(crate::prelude::KernelError::NotFound)
    }
}

/// 获取本机 MAC 地址
pub fn get_mac_address() -> [u8; 6] {
    let nic_guard = LOOPBACK_NIC.lock();
    if let Some(ref nic) = *nic_guard {
        nic.mac().0
    } else {
        [0; 6]
    }
}

// ============================================================
// 网卡子系统初始化与自测
// ============================================================

/// 网卡子系统初始化
pub fn init() -> KernelResult<()> {
    // 硬件探测（未检测到设备时静默）
    e1000::probe()?;
    igc::probe()?;
    virtio::probe()?;

    // 初始化全局回环网卡实例
    let mut loopback_guard = LOOPBACK_NIC.lock();
    *loopback_guard = Some(LoopbackNic::new());

    // 注册回环网卡信息（协议栈的基础设施）
    driver::register_nic(driver::NicInfo {
        name: "loopback",
        mac: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
        mtu: 1500,
    });
    Ok(())
}

/// 网卡子系统自测
pub fn selftest() -> bool {
    use self::driver::MacAddr;

    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    let mac = MacAddr::from_slice(&[0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]);
    all &= t("mac classification", !mac.is_multicast() && !mac.is_broadcast());

    let mut nic = LoopbackNic::new();
    let frame = [0x33u8; 100];
    let s = nic.send(&frame).is_ok();
    let mut buf = [0u8; 100];
    let r = nic.recv(&mut buf).is_ok() && &buf[..] == &frame[..];
    all &= t("loopback send/recv", s && r);

    all &= t("nic registry", driver::count() >= 1);
    all &= t("nic list", driver::list().len() >= 1);

    all
}

/// 供网络栈查询的网卡列表
pub use driver::{list, count, NicInfo};
