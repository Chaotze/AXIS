// ============================================================
// VirtIO 网卡驱动（virtio-net）
// ============================================================
// VirtIO 虚拟网卡探测。
//
// 实现状态：
// ✅ 完成：PCI 探测（modern 0x1041 / transitional 0x1000）
// ⏳ 待完成（网络栈阶段）：
//   - 复用 virtio-pci 配置区定位与虚拟队列（同 virtio-blk 骨架）
//   - 收发队列与 NicDevice 实现
//
// 探测逻辑与 virtio-blk 的 capability 枚举相同；为避免重复实现，
// 队列初始化在跟网络栈一起落地（阶段 7 需要确切的帧格式约定）。

use crate::prelude::KernelResult;

const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET_DEVICE: u16 = 0x1041; // modern
const VIRTIO_NET_LEGACY: u16 = 0x1000; // legacy/transitional

/// 探测 virtio-net 设备（当前仅报告）
pub fn probe() -> KernelResult<()> {
    let dev = crate::drivers::pci::find(VIRTIO_VENDOR, VIRTIO_NET_DEVICE)
        .or_else(|| crate::drivers::pci::find(VIRTIO_VENDOR, VIRTIO_NET_LEGACY));
    let Some(dev) = dev else {
        return Ok(());
    };
    println!("[VIRTIO-NET] {:02X}:{:02X}.{} detected ({:04X})",
        dev.bus, dev.dev, dev.func, dev.device_id);
    println!("[VIRTIO-NET] virtqueue 与收发通路待网络栈阶段（阶段 7）");
    Ok(())
}