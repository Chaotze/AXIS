// ============================================================
// 块设备子系统
// ============================================================
// 块存储设备统一抽象（BlkDevice）、设备注册表与初始化：
// - 纯逻辑：请求队列（blk_queue）、I/O 调度器（io_scheduler）
// - 合成设备：RamDisk（内存盘，可用于文件系统接入前的验证）
// - 硬件驱动：virtio-blk、NVMe、AHCI（各自模块探测并注册）
//
// 为什么先做 RamDisk：块设备驱动可以脱离真实硬件验证请求/
// 调度/读写回路的正确性；后续文件系统（exFAT）接入块设备时
// 也有一个确定性的测试目标。

pub mod ahci;
pub mod blk_queue;
pub mod io_scheduler;
pub mod nvme;
pub mod virtio;

use alloc::vec::Vec;

use crate::prelude::{KernelError, KernelResult};
use crate::sync::Spinlock;

use self::blk_queue::{BlkQueue, ReqOp, ReqStatus, DEFAULT_QUEUE_DEPTH};

/// 块设备接口
///
/// 所有块设备（RamDisk/virtio-blk/NVMe/AHCI）实现该 trait；
/// 读写以扇区（sector）为单位，扇区大小由 sector_size 提供。
pub trait BlkDevice: Send {
    /// 设备名（如 "ramdisk"、"virtio-blk"、"nvme0"）
    fn name(&self) -> &'static str;
    /// 容量（扇区数）
    fn capacity_sectors(&self) -> u64;
    /// 扇区大小（字节）
    fn sector_size(&self) -> u32;
    /// 从 lba 开始读 sectors 个扇区到 buf
    ///
    /// buf 长度必须 >= sectors * sector_size
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> KernelResult<()>;
    /// 从 lba 开始写 sectors 个扇区
    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> KernelResult<()>;
}

/// 全局块设备注册表
static DEVICES: Spinlock<Vec<alloc::boxed::Box<dyn BlkDevice>>> = Spinlock::new(Vec::new());

/// 注册块设备
pub fn register_device(dev: alloc::boxed::Box<dyn BlkDevice>) {
    let name = dev.name();
    DEVICES.lock().push(dev);
    println!("[BLK] registered device: {}", name);
}

/// 设备数量
pub fn device_count() -> usize {
    DEVICES.lock().len()
}

/// 获取设备名列表（供 procfs / 诊断）
pub fn device_names() -> Vec<&'static str> {
    DEVICES.lock().iter().map(|d| d.name()).collect()
}

/// 返回第 idx 个设备的摘要（名称 + 容量）；用于 procfs 展示
#[derive(Debug, Clone, Copy, Default)]
pub struct BlockDeviceInfo {
    pub name: &'static str,
    pub sectors: u64,
    pub sector_size: u32,
}

pub fn devices_info() -> Vec<BlockDeviceInfo> {
    DEVICES.lock().iter().map(|d| BlockDeviceInfo {
        name: d.name(),
        sectors: d.capacity_sectors(),
        sector_size: d.sector_size(),
    }).collect()
}

// ------------------------------------------------------------
// RamDisk（内存盘）
// ------------------------------------------------------------

/// 内存盘：用内核堆模拟块设备（扇区大小 512B）
pub struct RamDisk {
    /// 数据存储
    data: Vec<u8>,
    /// 扇区大小
    sector_size: u32,
}

impl RamDisk {
    /// 创建指定扇区数的内存盘
    pub fn new(sectors: u64, sector_size: u32) -> KernelResult<Self> {
        let bytes = sectors.checked_mul(sector_size as u64)
            .ok_or(KernelError::InvalidArgument)?;
        if bytes > 1 << 30 {
            return Err(KernelError::InvalidArgument); // 上限 1GB，防御误用
        }
        let mut data = Vec::new();
        data.resize(bytes as usize, 0);
        Ok(Self { data, sector_size })
    }
}

impl BlkDevice for RamDisk {
    fn name(&self) -> &'static str {
        "ramdisk"
    }

    fn capacity_sectors(&self) -> u64 {
        (self.data.len() / self.sector_size as usize) as u64
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> KernelResult<()> {
        let sz = self.sector_size as usize;
        let start = (lba as usize).checked_mul(sz).ok_or(KernelError::InvalidArgument)?;
        let end = start.checked_add(buf.len()).ok_or(KernelError::InvalidArgument)?;
        if end > self.data.len() {
            return Err(KernelError::InvalidArgument);
        }
        // 读：数据从盘的 data 区拷入调用方缓冲（方向与写相反）
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> KernelResult<()> {
        let sz = self.sector_size as usize;
        let start = (lba as usize).checked_mul(sz).ok_or(KernelError::InvalidArgument)?;
        let end = start.checked_add(buf.len()).ok_or(KernelError::InvalidArgument)?;
        if end > self.data.len() {
            return Err(KernelError::InvalidArgument);
        }
        self.data[start..end].copy_from_slice(buf);
        Ok(())
    }
}

// ------------------------------------------------------------
// 初始化
// ------------------------------------------------------------

/// 块设备子系统初始化
///
/// 1. 创建 RamDisk 并注册（始终可用）
/// 2. 通过 PCI 探测各硬件块设备（virtio-blk / NVMe / AHCI），
///    找到的注册进设备表
pub fn init() {
    // RamDisk：16MB（32768 扇区 × 512B），供后续文件系统接入
    if let Ok(disk) = RamDisk::new(32768, 512) {
        register_device(alloc::boxed::Box::new(disk));
    } else {
        println!("[BLK] RamDisk creation failed");
    }

    // 硬件设备探测（未找到设备时对应驱动返回 None）
    if let Some(dev) = virtio::probe() {
        register_device(dev);
    }
    if let Some(dev) = nvme::probe() {
        register_device(dev);
    }
    if let Some(dev) = ahci::probe() {
        register_device(dev);
    }
}

/// 块设备子系统自测
///
/// 1. RamDisk 读写回路（经 trait 对象，验证真实接口路径）
/// 2. 请求队列容量/序号
/// 3. I/O 调度器行为（FIFO 顺序、合并、SSTF 寻道序）
pub fn selftest() -> bool {
    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    // 1. RamDisk 读写
    {
        let mut disk = RamDisk::new(64, 512).expect("ramdisk");
        // 立即读写同一扇区做最小验证
        {
            let mut tiny = [0u8; 512];
            tiny[0] = 0x5A;
            tiny[511] = 0xA5;
            let w = disk.write_sectors(0, &tiny).is_ok();
            let mut back = [0u8; 512];
            let r = disk.read_sectors(0, &mut back).is_ok();
            all &= t("ramdisk sector0 roundtrip", w && r && back[0] == 0x5A && back[511] == 0xA5);
        }
        let mut wbuf = [0u8; 512];
        for (i, b) in wbuf.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        // 写两个扇区再读回
        let mut wbuf2 = [0u8; 1024];
        for (i, b) in wbuf2.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(3);
        }
        all &= t("ramdisk write", disk.write_sectors(2, &wbuf).is_ok());
        all &= t("ramdisk write multi", disk.write_sectors(8, &wbuf2).is_ok());
        let mut rbuf = [0u8; 512];
        let read_ok = disk.read_sectors(2, &mut rbuf).is_ok() && rbuf == wbuf;
        all &= t("ramdisk read", read_ok);
        let mut rbuf2 = [0u8; 1024];
        let read2_ok = disk.read_sectors(8, &mut rbuf2).is_ok() && rbuf2 == wbuf2;
        all &= t("ramdisk read multi", read2_ok);
        // 越界读应报错
        all &= t("ramdisk bounds", disk.read_sectors(100, &mut rbuf).is_err());
    }

    // 2. 请求队列
    {
        let mut q = BlkQueue::new(4);
        let mut r = blk_queue::BlkRequest::new(ReqOp::Read, 0, 1, core::ptr::null_mut(), 0);
        all &= t("blkqueue submit", q.submit(&mut r).is_ok());
        all &= t("blkqueue outstanding", q.outstanding() == 1);
        r.complete_ok();
        q.complete(&mut r);
        all &= t("blkqueue complete", q.is_idle() && r.status == ReqStatus::Ok);
    }

    // 3. 调度器
    {
        use self::io_scheduler::IoScheduler;
        let make = |op: ReqOp, lba: u64| blk_queue::BlkRequest::new(op, lba, 1, core::ptr::null_mut(), 0);

        let mut fifo = io_scheduler::FifoScheduler::new(128);
        fifo.push(make(ReqOp::Read, 10));
        fifo.push(make(ReqOp::Read, 11));
        all &= t("scheduler fifo merge", fifo.len() == 1);
        let head = fifo.pop().unwrap();
        all &= t("scheduler fifo order", head.lba == 10 && head.sectors == 2);
        all &= t("scheduler fifo empty", fifo.is_empty());

        let mut sstf = io_scheduler::SstfScheduler::new();
        sstf.push(make(ReqOp::Read, 100));
        sstf.push(make(ReqOp::Read, 5));
        all &= t("scheduler sstf nearest", sstf.pop().unwrap().lba == 5);
    }

    // 硬件读写验证：virtio-blk/NVMe/AHCI 的读写通路尚未在本阶段
    // 注册（见各驱动模块说明），此处只统计设备注册表。

    // 设备注册表
    all &= t("device registry", device_count() >= 1);
    if !device_names().is_empty() {
        // 逐项打印（join 在本工具链下有显示问题，改用循环）
        for n in device_names() {
            println!("    [INFO] registered: {}", n);
        }
    }

    all
}

/// 调试辅助：把调度器/队列深度暴露给上层（procfs 等）
pub const fn default_queue_depth() -> usize {
    DEFAULT_QUEUE_DEPTH
}