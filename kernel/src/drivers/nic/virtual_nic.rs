// ============================================================
// 虚拟网络接口卡（Virtual NIC）驱动
// ============================================================
// 用于测试和开发的虚拟网络设备
// 实现了 NetworkDevice trait，可以模拟网络通信
//
// 设计：
// - 使用内存缓冲区模拟网络连接
// - 支持"发送"和"接收"操作
// - 后续可通过添加包处理逻辑来模拟真实网络

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::Spinlock;

// ============================================================
// 虚拟网卡状态
// ============================================================

/// 虚拟网卡内部状态
struct VirtualNicState {
    /// 发送队列（模拟发送的帧）
    tx_queue: VecDeque<Vec<u8>>,
    /// 接收队列（待处理的帧）
    rx_queue: VecDeque<Vec<u8>>,
}

impl VirtualNicState {
    fn new() -> Self {
        VirtualNicState {
            tx_queue: VecDeque::new(),
            rx_queue: VecDeque::new(),
        }
    }
}

// ============================================================
// 虚拟网卡
// ============================================================

/// 虚拟网络接口卡
pub struct VirtualNic {
    /// 虚拟网卡的 MAC 地址（虚构地址）
    mac: [u8; 6],
    /// 设备启用状态
    enabled: AtomicBool,
    /// 内部状态（发送/接收队列）
    state: Spinlock<VirtualNicState>,
}

impl VirtualNic {
    /// 创建虚拟网卡实例
    pub fn new() -> Self {
        VirtualNic {
            mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],  // 虚拟 MAC 地址
            enabled: AtomicBool::new(false),
            state: Spinlock::new(VirtualNicState::new()),
        }
    }
}

// ============================================================
// NetworkDevice Trait 实现
// ============================================================

impl super::NetworkDevice for VirtualNic {
    fn send(&self, frame: &[u8]) -> KernelResult<usize> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(KernelError::NotFound);
        }

        // 禁用中断，保护访问
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();
        // 复制帧到发送队列
        // 在真实驱动中，这里会将帧推送给 NIC 硬件
        let frame_vec = alloc::vec::Vec::from(frame);
        state.tx_queue.push_back(frame_vec.clone());

        // 释放锁
        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        // 打印发送信息（调试用）
        println!(
            "[VNIC] TX {} bytes: {:02x?}...",
            frame.len(),
            &frame[0..core::cmp::min(14, frame.len())]
        );

        Ok(frame.len())
    }

    fn recv(&self) -> KernelResult<Vec<u8>> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(KernelError::NotFound);
        }

        // 禁用中断，保护访问
        let flags = crate::arch::x86_64::cpu::irq_save();

        let mut state = self.state.lock();
        let frame = state.rx_queue.pop_front();

        drop(state);
        unsafe { crate::arch::x86_64::cpu::irq_restore(flags); }

        match frame {
            Some(f) => {
                println!("[VNIC] RX {} bytes", f.len());
                Ok(f)
            }
            None => {
                // 无数据可接收，返回空向量
                Ok(alloc::vec::Vec::new())
            }
        }
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn name(&self) -> &str {
        "vnic0"
    }

    fn enable(&self) -> KernelResult<()> {
        self.enabled.store(true, Ordering::SeqCst);
        println!("[VNIC] Device enabled");
        Ok(())
    }

    fn disable(&self) -> KernelResult<()> {
        self.enabled.store(false, Ordering::SeqCst);
        println!("[VNIC] Device disabled");
        Ok(())
    }
}

// ============================================================
// 虚拟网卡测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_nic_basic() {
        let vnic = VirtualNic::new();

        // 启用网卡
        assert!(vnic.enable().is_ok());

        // 检查 MAC 地址
        let mac = vnic.mac_address();
        assert_eq!(mac[0], 0x02);

        // 检查设备名称
        assert_eq!(vnic.name(), "vnic0");

        // 禁用网卡
        assert!(vnic.disable().is_ok());
    }

    #[test]
    fn test_virtual_nic_send_recv() {
        let vnic = VirtualNic::new();
        vnic.enable().unwrap();

        let test_frame = alloc::vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];

        // 发送测试帧
        let sent = vnic.send(&test_frame).unwrap();
        assert_eq!(sent, test_frame.len());

        // 接收应该为空（虚拟网卡没有回环）
        let recv = vnic.recv().unwrap();
        assert!(recv.is_empty());
    }
}
