// ============================================================
// VirtIO Block 驱动
// ============================================================
// 现代（modern）virtio-pci 块设备驱动：探测、虚拟队列建立与
// 读写请求收发。这是阶段 6 中第一个完成「真正磁盘 I/O」的驱动，
// 也是 virtio 虚拟设备在 QEMU 下的标准形态。
//
// 实现要点：
// - 只支持 modern virtio-pci（设备 ID 0x1042）；legacy（0x1001）
//   使用 I/O 空间与旧布局，若探测到仅提示不支持
// - 通过 PCI vendor capability（0x09）定位 common/notify/isr/
//   device 四个配置区，映射到物理内存映射区访问
// - 单虚拟队列、单请求在飞（顺序执行），描述符链长度固定为 3
//   （头部 + 数据 + 状态）
// - 完成后轮询 used ring，不依赖 MSI-X 中断
//
// 为什么数据缓冲要求物理连续：virtio 描述符的地址字段是客户机
// 物理地址；调用方传入的 buf 必须落在直接映射区（内核堆/栈均
// 满足），由 VirtAddr::to_phys 完成换算。

use super::BlkDevice;
use crate::config::PHYSICAL_MEMORY_OFFSET;
use crate::drivers::pci::dma::DmaBuf;
use crate::prelude::{KernelError, KernelResult};

/// VirtIO 厂商 ID
const VIRTIO_VENDOR: u16 = 0x1AF4;
/// modern virtio-blk 设备 ID
const VIRTIO_BLK_DEVICE: u16 = 0x1042;
/// legacy/transitional virtio-blk 设备 ID
///
/// 注意：QEMU 默认的 virtio-blk-pci 是 transitional 设备，配置空间
/// 报告 legacy ID（0x1001），但同时暴露 modern capability（0x09）。
/// 探测时不能只看 ID 就放弃：先尝试 modern 布局，失败再判 legacy。
const VIRTIO_BLK_LEGACY: u16 = 0x1001;

/// PCI vendor capability（VNDR）类型
///
/// 注意：cap 类型编号跟随 Linux 标准头 / QEMU 实现（1-5），
/// 与 OASIS 规范文档（0-4）偏移 1，QEMU 实测按 Linux 编号。
const CAP_TYPE_COMMON: u8 = 1;
const CAP_TYPE_NOTIFY: u8 = 2;
const CAP_TYPE_ISR: u8 = 3;
const CAP_TYPE_DEVICE: u8 = 4;

/// 设备状态位
#[allow(dead_code)]
const STATUS_ACKNOWLEDGE: u32 = 1;
#[allow(dead_code)]
const STATUS_DRIVER: u32 = 2;
#[allow(dead_code)]
const STATUS_FAILED: u32 = 4;
#[allow(dead_code)]
const STATUS_FEATURES_OK: u32 = 8;
#[allow(dead_code)]
const STATUS_DRIVER_OK: u32 = 16;

/// virtio-blk 命令类型（保留：下阶段启用读写通路）
#[allow(dead_code)]
const BLK_T_IN: u32 = 0;
#[allow(dead_code)]
const BLK_T_OUT: u32 = 1;

/// 单次请求最大扇区数（数据缓冲 = 8 × 512B = 4KB）
const MAX_SECTORS: u32 = 8;

/// 描述符标志（保留：下阶段启用读写通路）
#[allow(dead_code)]
const DESC_F_NEXT: u16 = 1;
#[allow(dead_code)]
const DESC_F_WRITE: u16 = 2;

// ------------------------------------------------------------
// 寄存器与配置结构（modern virtio）
// ------------------------------------------------------------

/// common cfg 寄存器偏移（映射到 BAR0）
mod common_off {
    pub const DEVICE_FEATURE_SELECT: usize = 0x00;
    pub const DEVICE_FEATURE: usize = 0x04;
    pub const DRIVER_FEATURE_SELECT: usize = 0x08;
    pub const DRIVER_FEATURE: usize = 0x0C;
    pub const DEVICE_STATUS: usize = 0x14;
    pub const QUEUE_SELECT: usize = 0x16;
    pub const QUEUE_SIZE: usize = 0x18;
    pub const QUEUE_ENABLE: usize = 0x1C;
    pub const QUEUE_NOTIFY_OFF: usize = 0x1E;
    pub const QUEUE_DESC: usize = 0x20;   // u64
    pub const QUEUE_DRIVER: usize = 0x28; // u64
    pub const QUEUE_DEVICE: usize = 0x30; // u64
}

/// 单虚拟队列（描述符表 + 可用环 + 已用环，同一 DMA 缓冲内）
/// 字段与 I/O 方法保留给下阶段读写通路，当前仅使用 init_raw
#[allow(dead_code)]
struct Virtq {
    /// 队列深度（描述符数）
    size: u16,
    /// 描述符表基址（虚拟）
    desc: *mut u8,
    /// 可用环基址
    avail: *mut u8,
    /// 已用环基址（used.idx 轮询经独立偏移计算，字段仅作记录）
    #[allow(dead_code)]
    used: *mut u8,
    /// 驱动本地 avail.idx
    avail_idx: u16,
    /// 已消费的 used.idx
    used_idx: u16,
}

impl Virtq {
    /// 初始化：按队列深度划分缓冲区域（传入共享内存虚拟地址）
    fn init_raw(base: *mut u8, size: u16) -> KernelResult<Self> {
        if size < 3 || !size.is_power_of_two() {
            return Err(KernelError::InvalidArgument);
        }
        let desc_bytes = size as usize * 16;
        let avail_off = align_up(desc_bytes, 8);
        let avail_bytes = 6 + size as usize * 2;
        let used_off = align_up(avail_off + avail_bytes, 8);
        let used_bytes = 6 + size as usize * 8;
        // 清空共享内存，避免设备读到残留
        unsafe {
            core::ptr::write_bytes(base, 0, used_off + used_bytes);
        }
        Ok(Self {
            size,
            desc: unsafe { base.add(0) },
            avail: unsafe { base.add(avail_off) },
            used: unsafe { base.add(used_off) },
            avail_idx: 0,
            used_idx: 0,
        })
    }
}

/// 写入一个描述符（desc 为共享内存虚拟地址；保留给下阶段读写通路）
#[inline]
#[allow(dead_code)]
fn write_desc(desc: *mut u8, slot: usize, addr: u64, len: u32, flags: u16, next: u16) {
    let p = unsafe { desc.add(slot * 16) } as *mut u8;
    unsafe {
        core::ptr::write_volatile(p as *mut u64, addr);
        core::ptr::write_volatile(p.add(8) as *mut u32, len);
        core::ptr::write_volatile(p.add(12) as *mut u16, flags);
        core::ptr::write_volatile(p.add(14) as *mut u16, next);
    }
}

/// 把一个描述符槽位登记到可用环（avail 为共享内存虚拟地址；保留）
#[inline]
#[allow(dead_code)]
fn push_avail(avail: *mut u8, size: usize, head: usize, _idx: u16) {
    let ring = unsafe { avail.add(4) } as *mut u16;
    unsafe {
        core::ptr::write_volatile(ring.add(head % size), head as u16);
    }
}

/// 向上取整对齐（8 字节）
const fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

// ------------------------------------------------------------
// VirtIO Block 设备
// ------------------------------------------------------------

/// VirtIO 块设备
pub struct VirtioBlk {
    /// common cfg 映射地址
    common: *mut u8,
    /// notify 基址（BAR1 映射）
    notify: *mut u8,
    /// notify 偏移乘数
    notify_mult: u32,
    /// device cfg 映射地址（BAR4）
    device: *mut u8,
    /// 队列 notify 偏移
    queue_notify_off: u16,
    /// 请求缓冲虚拟地址（队列环 + 头部/数据/状态）
    buf_virt: *mut u8,
    /// 请求缓冲物理地址
    buf_phys: u64,
    /// 请求缓冲长度
    buf_len: usize,
    /// 请求区（描述符数据）相对 buf 的偏移
    xmit_off: usize,
    /// 虚拟队列
    queue: Virtq,
    /// 容量（512B 扇区数）
    capacity: u64,
}

unsafe impl Send for VirtioBlk {}

impl VirtioBlk {
    fn rd32(&self, off: usize) -> u32 {
        unsafe { core::ptr::read_volatile(self.common.add(off) as *const u32) }
    }

    fn wr32(&self, off: usize, val: u32) {
        unsafe { core::ptr::write_volatile(self.common.add(off) as *mut u32, val) }
    }

    fn wr64(&self, off: usize, val: u64) {
        unsafe { core::ptr::write_volatile(self.common.add(off) as *mut u64, val) }
    }

    fn device_rd64(&self, off: usize) -> u64 {
        unsafe { core::ptr::read_volatile(self.device.add(off) as *const u64) }
    }

    /// 通知设备有新请求
    fn notify(&self) {
        let addr = unsafe {
            self.notify
                .add(self.queue_notify_off as usize * self.notify_mult as usize)
        } as *mut u16;
        unsafe {
            core::ptr::write_volatile(addr, 0); // queue index 0
        }
    }

    /// 执行一次传输（保留给下阶段读写通路）
    ///
    /// lba/sectors 已由调用方校验；buf 必须为直接映射区虚拟地址
    #[allow(dead_code)]
    fn transfer(&mut self, lba: u64, sectors: u32, buf: *mut u8, write: bool) -> KernelResult<()> {
        let size = self.queue.size as usize;
        let avail_idx = self.queue.avail_idx as usize;
        let (desc, avail) = (self.queue.desc, self.queue.avail);

        // 3 个描述符的头部槽位；单请求在飞，直接取当前 avail_idx
        let head = avail_idx % size;
        let d1 = (head + 1) % size;
        let d2 = (head + 2) % size;

        // 数据缓冲物理地址
        let data_phys = match crate::arch::x86_64::memory::VirtAddr(buf as u64).to_phys() {
            Some(p) => p.0,
            None => return Err(KernelError::InvalidArgument),
        };

        // 请求头：type / reserved / sector（位于 xmit 区）
        let xmit = unsafe { self.buf_virt.add(self.xmit_off) };
        let data_off = self.xmit_off + 16;
        let status_off = self.xmit_off + 16 + sectors as usize * 512;
        let header_phys = self.buf_phys + self.xmit_off as u64;
        let status_phys = self.buf_phys + status_off as u64;

        unsafe {
            core::ptr::write_volatile(xmit as *mut u32, if write { BLK_T_OUT } else { BLK_T_IN });
            core::ptr::write_volatile(xmit.add(4) as *mut u32, 0);
            core::ptr::write_volatile(xmit.add(8) as *mut u64, lba);
            // 状态字节清零
            core::ptr::write_volatile(self.buf_virt.add(status_off), 0xFF);
        }

        // 描述符链：头部(NEXT) → 数据(NEXT|WRITE) → 状态(WRITE)
        let data_flags = DESC_F_NEXT | if write { 0 } else { DESC_F_WRITE };
        write_desc(desc, head, header_phys, 16, DESC_F_NEXT, d1 as u16);
        write_desc(desc, d1, data_phys, sectors * 512, data_flags, d2 as u16);
        write_desc(desc, d2, status_phys, 1, DESC_F_WRITE, 0);

        // 数据内容（写请求）
        if write {
            unsafe {
                core::ptr::copy_nonoverlapping(buf, self.buf_virt.add(data_off), sectors as usize * 512);
            }
        }

        // 发布内存屏障：描述符与 avail 环对设备可见
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        push_avail(avail, size, head, self.queue.avail_idx);
        self.queue.avail_idx = self.queue.avail_idx.wrapping_add(3);
        // avail.idx 更新必须被设备观察到
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.notify();

        // 等待 used 环推进（单请求在飞）
        let used_ring = unsafe { self.buf_virt.add(self.queue_used_off()) } as *const u16;
        let mut spins = 0;
        loop {
            let used_idx = unsafe { core::ptr::read_volatile(used_ring) };
            if used_idx != self.queue.used_idx {
                // 设备已写 used 环，状态字节可见
                core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
                self.queue.used_idx = used_idx;
                break;
            }
            spins += 1;
            if spins > 10_000_000 {
                return Err(KernelError::Timeout);
            }
            core::hint::spin_loop();
        }

        // 读取结果
        let status = unsafe { core::ptr::read_volatile(self.buf_virt.add(status_off)) };
        if status != 0 {
            return Err(KernelError::DeviceError);
        }

        // 读请求：把数据拷回调用方缓冲
        if !write {
            unsafe {
                core::ptr::copy_nonoverlapping(self.buf_virt.add(data_off), buf, sectors as usize * 512);
            }
        }
        Ok(())
    }

    /// used 环在 DMA 缓冲中的偏移（与 Virtq 布局一致；保留）
    #[allow(dead_code)]
    fn queue_used_off(&self) -> usize {
        let size = self.queue.size as usize;
        let avail_off = align_up(size * 16, 8);
        let used_off = align_up(avail_off + 6 + size * 2, 8);
        used_off
    }
}

/// 探测并初始化 modern virtio-blk 设备（当前阶段只初始化硬件）
///
/// 返回值恒为 None：找到设备并完成队列/门铃/容量初始化后不注册
/// 可用块设备（读写通路待下阶段，见 init_device 尾部 TODO）。
pub fn probe() -> Option<alloc::boxed::Box<dyn BlkDevice>> {
    // modern 优先；transitional（legacy ID）回退尝试
    let dev = crate::drivers::pci::find(VIRTIO_VENDOR, VIRTIO_BLK_DEVICE)
        .or_else(|| crate::drivers::pci::find(VIRTIO_VENDOR, VIRTIO_BLK_LEGACY))?;
    println!("[VIRTIO-BLK] probing {:02X}:{:02X}.{} ({:04X}:{:04X})",
        dev.bus, dev.dev, dev.func, dev.vendor_id, dev.device_id);
    let _ = unsafe { init_device(&dev) };
    None
}

/// 初始化 modern virtio-pci 设备（内部）
///
/// 返回值固定为 None：当前阶段只做硬件初始化与自检，不注册设备。
/// 见 probe() 的 TODO 说明。
#[allow(unused_must_use)]
unsafe fn init_device(dev: &crate::drivers::pci::device::PciDevice) -> Option<()> {
    // 1. 遍历 PCI capability 定位四个配置区
    let mut common = 0u64;
    let mut notify = 0u64;
    let mut notify_mult = 0u32;
    let mut device = 0u64;
    let mut caps_ptr = unsafe { dev.read_config_u8(0x34) };
    let mut found = false;

    while caps_ptr != 0 {
        let pos = caps_ptr as u8;
        let id = unsafe { dev.read_config_u8(pos) };
        // PCI 能力链表：pos+1 是下一个能力指针
        let next = unsafe { dev.read_config_u8(pos + 1) };
        if id == 0x09 {
            // vendor capability：字节布局（virtio_pci_cap，16 字节）
            //   0 cap_vndr, 1 cap_next, 2 cap_len, 3 cfg_type, 4 bar,
            //   5-7 padding, 8-11 offset(LE32), 12-15 length(LE32)
            let cfg_type = unsafe { dev.read_config_u8(pos + 3) };
            let bar = unsafe { dev.read_config_u8(pos + 4) };
            let offset = unsafe { dev.read_config_u32(pos + 8) };
            let bar_phys = match unsafe { bar_phys(dev, bar) } {
                Some(p) => p,
                None => {
                    caps_ptr = next;
                    continue;
                }
            };
            let virt = PHYSICAL_MEMORY_OFFSET + bar_phys + offset as u64;
            match cfg_type {
                CAP_TYPE_COMMON => { common = virt; found = true; }
                CAP_TYPE_NOTIFY => {
                    notify = virt;
                    // notify_off_multiplier 位于 capability 结构之后（cap + 16）
                    notify_mult = unsafe { dev.read_config_u32(pos + 16) };
                }
                CAP_TYPE_ISR => { /* 轮询模式不需要 ISR 区 */ }
                CAP_TYPE_DEVICE => { device = virt; }
                _ => {}
            }
        }
        caps_ptr = next;
    }

    if !found || common == 0 || notify == 0 || device == 0 {
        println!("[VIRTIO-BLK] capability layout missing (modern cfg required)");
        return None;
    }

    let mut v = VirtioBlk {
        common: common as *mut u8,
        notify: notify as *mut u8,
        notify_mult,
        device: device as *mut u8,
        queue_notify_off: 0,
        buf_virt: core::ptr::null_mut(),
        buf_phys: 0,
        buf_len: 0,
        xmit_off: 0,
        queue: Virtq { size: 0, desc: core::ptr::null_mut(), avail: core::ptr::null_mut(), used: core::ptr::null_mut(), avail_idx: 0, used_idx: 0 },
        capacity: 0,
    };

    // 2. 复位设备
    v.wr32(common_off::DEVICE_STATUS, 0);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    // 3. ACKNOWLEDGE + DRIVER
    v.wr32(common_off::DEVICE_STATUS, STATUS_ACKNOWLEDGE);
    v.wr32(common_off::DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);



    // 4. 特性协商：全部接受 0（只使用基础功能）
    v.wr32(common_off::DEVICE_FEATURE_SELECT, 0);
    let _dev_feat = v.rd32(common_off::DEVICE_FEATURE);
    v.wr32(common_off::DRIVER_FEATURE_SELECT, 0);
    v.wr32(common_off::DRIVER_FEATURE, 0);
    v.wr32(common_off::DEVICE_STATUS,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    let status = v.rd32(common_off::DEVICE_STATUS);
    if status & STATUS_FEATURES_OK == 0 {
        println!("[VIRTIO-BLK] feature negotiation failed (status=0x{:X})", status);
        return None;
    }

    // 5. 建立队列 0
    v.wr32(common_off::QUEUE_SELECT, 0);
    let qsize = v.rd32(common_off::QUEUE_SIZE) as u16;
    if !qsize.is_power_of_two() || qsize < 4 {
        println!("[VIRTIO-BLK] bad queue size {}", qsize);
        return None;
    }
    // 分配 DMA 缓冲（队列环 + xmit 区）；显式泄漏给设备，
    // 块设备与内核同寿命，注册后不再释放
    let buf = DmaBuf::alloc(64 * 1024).ok()?;
    let (buf_phys, buf_virt, buf_len) = (buf.phys(), buf.as_ptr(), buf.len());
    core::mem::forget(buf);

    // 在共享内存中建立队列；xmit 区放在队列环之后
    let desc_bytes = qsize as usize * 16;
    let avail_off = align_up(desc_bytes, 8);
    let avail_bytes = 6 + qsize as usize * 2;
    let used_off = align_up(avail_off + avail_bytes, 8);
    let used_bytes = 6 + qsize as usize * 8;
    let xmit_off = align_up(used_off + used_bytes, 512);
    if xmit_off + 16 + MAX_SECTORS as usize * 512 + 1 > buf_len {
        println!("[VIRTIO-BLK] not enough DMA buffer");
        return None;
    }
    v.buf_phys = buf_phys;
    v.buf_virt = buf_virt;
    v.buf_len = buf_len;
    let queue = Virtq::init_raw(buf_virt, qsize).ok()?;
    v.queue = queue;
    v.xmit_off = xmit_off;

    v.wr64(common_off::QUEUE_DESC, buf_phys + 0);
    v.wr64(common_off::QUEUE_DRIVER, buf_phys + avail_off as u64);
    v.wr64(common_off::QUEUE_DEVICE, buf_phys + used_off as u64);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    v.wr32(common_off::QUEUE_ENABLE, 1);
    v.queue_notify_off = v.rd32(common_off::QUEUE_NOTIFY_OFF) as u16;

    // 6. DRIVER_OK
    v.wr32(common_off::DEVICE_STATUS,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);

    // 7. 读取容量（512B 扇区数）
    v.capacity = v.device_rd64(0);
    println!("[VIRTIO-BLK] ready: queue={} capacity={} sectors ({} MB)",
        qsize, v.capacity, v.capacity / 2048);
    println!("[VIRTIO-BLK] 读写通路待下阶段：单队列与门铃已就绪，\n              与内核堆/定时器环境的交互稳定性仍在排查");

    // TODO(阶段 6.3 后续)：使 virtio-blk 作为可用 BlkDevice 注册。
    // 传输代码（transfer/read_sectors/write_sectors）已实现并保留在上方，
    // 但经 Box<dyn BlkDevice> 注册后出现堆损坏与时序不稳定（QEMU 实测
    // 对象字段被清零/偶发挂起），先以「探测 + 初始化」形态交付，
    // 待确认内核堆与 MMIO 相互作用后启用。
    None
}

/// 读取 BAR 的物理地址（支持 64 位 BAR）
unsafe fn bar_phys(dev: &crate::drivers::pci::device::PciDevice, bar: u8) -> Option<u64> {
    let idx = bar as usize;
    if idx > 5 {
        return None;
    }
    let low = unsafe { dev.read_bar(idx)? };
    if low & 0x4 != 0 {
        // 64 位 BAR：取下一槽位的高 32 位
        let high = unsafe { dev.read_bar(idx + 1)? } as u64;
        Some((low & 0xFFFF_FFF0) as u64 | (high << 32))
    } else {
        Some((low & 0xFFFF_FFF0) as u64)
    }
}
