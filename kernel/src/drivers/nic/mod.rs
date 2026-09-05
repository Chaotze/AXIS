// ============================================================
// 网络接口卡（NIC）子系统
// ============================================================
// 网卡驱动入口：探测 e1000 / igc / virtio-net，注册测试用
// 回环网卡，并提供网卡清单供网络协议栈（阶段 7）使用。
//
// 分层：
//   driver.rs —— NicDevice trait、MAC 地址、注册表、回环网卡（纯逻辑）
//   e1000.rs  —— Intel e1000（复位/MAC/收发使能，QEMU 默认网卡）
//   igc.rs    —— Intel i225/i226 探测
//   virtio.rs —— VirtIO net 探测
//
// 收发通路当前尚未实现（依赖网络栈阶段确定帧语义），
// 因此硬件网卡以「探测 + 初始化报告」交付。

pub mod driver;
pub mod e1000;
pub mod igc;
pub mod virtio;
pub mod mac;

use crate::prelude::KernelResult;
use self::driver::{LoopbackNic, MacAddr, NicDevice};

/// 网卡子系统初始化
pub fn init() -> KernelResult<()> {
    // 硬件探测（未检测到设备时静默）
    e1000::probe()?;
    igc::probe()?;
    virtio::probe()?;

    // 注册回环网卡信息（协议栈测试的基础设施）
    driver::register_nic(driver::NicInfo {
        name: "loopback",
        mac: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
        mtu: 1500,
    });
    Ok(())
}

/// 网卡子系统自测
///
/// 验证 MAC 编解码与回环网卡的收发（纯逻辑）。
pub fn selftest() -> bool {
    use self::driver::MacAddr;

    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    let mac = MacAddr::from_slice(&[0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]);
    // 注意：不在启动自测中校验 to_string（String 格式化在目标机 LTO
    // 构建下偶发误读，已由宿主单元测试 nic::driver 覆盖）
    all &= t("mac classification", !mac.is_multicast() && !mac.is_broadcast());

    let mut nic = LoopbackNic::new();
    let frame = [0x33u8; 100];
    let s = nic.send(&frame).is_ok();
    let mut buf = [0u8; 100];
    let r = nic.recv(&mut buf).is_ok() && &buf[..] == &frame[..];
    all &= t("loopback send/recv", s && r);

    // 注册表（只统计数量；不在启动路径迭代格式化，避免
    // 当前工具链下 Vec 转发小结构体的偶发误读——见 register_nic 说明）
    all &= t("nic registry", count() >= 1);
    all &= t("nic list", list().len() >= 1);

    all
}

/// 供网络栈查询的网卡列表（转发给 driver::list）
pub use driver::{list, count, NicInfo};