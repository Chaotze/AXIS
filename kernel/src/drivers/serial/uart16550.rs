// ============================================================
// UART 16550 串口驱动
// ============================================================
// 实现与 NS16550 兼容的串口控制器驱动：初始化（波特率、数据格式、
// FIFO）、轮询收发。这是内核日志镜像与调试控制台的基础。
//
// 为什么用端口 I/O（IN/OUT）而不是 MMIO：
// - 传统 PC 上 16550 既出现在端口空间（ISA COM1/COM2，0x3F8/0x2F8）
//   也可能以 MMIO 形式出现（PCIe 串口卡）
// - 端口访问由 arch::x86_64::io 提供，本模块只关心 UART 寄存器语义
//
// 并发约定：单字节 putc/getc 各是一条 OUT/IN 指令，天然原子；
// 多字节写由调用方（如 print.rs 已持 WRITER 锁）保证不交错。

use crate::arch::x86_64::io::{inb, outb, io_wait};

// ------------------------------------------------------------
// 寄存器偏移（相对端口基址）
// ------------------------------------------------------------

/// 接收缓冲寄存器（RBR）/ 发送保持寄存器（THR）/ 分频锁存低字节（DLL）
const REG_DATA: u16 = 0;
/// 中断使能寄存器（IER）/ 分频锁存高字节（DLM）
const REG_IER: u16 = 1;
/// 中断标识寄存器（IIR）/ FIFO 控制寄存器（FCR）
const REG_FCR: u16 = 2;
/// 线路控制寄存器（LCR）
const REG_LCR: u16 = 3;
/// 调制解调器控制寄存器（MCR）
const REG_MCR: u16 = 4;
/// 线路状态寄存器（LSR）
const REG_LSR: u16 = 5;

// LSR 标志位
const LSR_DATA_READY: u8 = 0x01;   // 接收数据就绪（RBR 可读）
const LSR_THR_EMPTY: u8 = 0x20;    // 发送保持寄存器空（THR 可写）

// LCR 标志位
const LCR_DLAB: u8 = 0x80;         // 分频锁存访问位
const LCR_8N1: u8 = 0x03;          // 8 数据位、无校验、1 停止位

// FCR 标志位
const FCR_ENABLE: u8 = 0x01;       // 启用 FIFO
const FCR_CLEAR_RX: u8 = 0x02;     // 清空接收 FIFO
const FCR_CLEAR_TX: u8 = 0x04;     // 清空发送 FIFO

// MCR 标志位
const MCR_DTR: u8 = 0x01;          // 数据终端就绪
const MCR_RTS: u8 = 0x02;          // 请求发送
const MCR_OUT2: u8 = 0x08;         // OUT2（使能 UART 中断输出）

/// 默认波特率除数（1 → 115200 baud，基于 1.8432MHz 基准时钟）
const DEFAULT_DIVISOR: u16 = 1;

/// COM1 端口基址（调试控制台）
pub const COM1_PORT: u16 = 0x3F8;
/// COM2 端口基址
pub const COM2_PORT: u16 = 0x2F8;

/// 16550 兼容串口控制器
///
/// 方法全部取 &self：寄存器访问是易失性端口 I/O，无需可变引用
#[derive(Debug, Clone, Copy)]
pub struct Uart16550 {
    /// 端口基址（如 0x3F8）
    port: u16,
}

impl Uart16550 {
    /// 创建串口控制器（不初始化）
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    /// 初始化控制器：115200-8N1 + 启 FIFO + 置 DTR/RTS
    ///
    /// 步骤（顺序敏感）：
    /// 1. 置 DLAB=1，写波特率除数（DLL/DLM）
    /// 2. 置 LCR=8N1（此操作同时清除 DLAB）
    /// 3. 配置 FCR：启用并清空收发 FIFO
    /// 4. 置 MCR：DTR/RTS/OUT2（OUT2 是 UART 中断输出的总开关）
    pub fn init(&self) {
        unsafe {
            // 进入分频锁存模式（DLAB=1）
            outb(self.port + REG_LCR, LCR_DLAB);
            io_wait();
            // 写除数：低字节 + 高字节
            outb(self.port + REG_DATA, (DEFAULT_DIVISOR & 0xFF) as u8);
            outb(self.port + REG_IER, (DEFAULT_DIVISOR >> 8) as u8);
            io_wait();
            // 数据格式 8N1（同时退出分频锁存模式）
            outb(self.port + REG_LCR, LCR_8N1);
            io_wait();
            // FIFO：启用 + 清空
            outb(self.port + REG_FCR, FCR_ENABLE | FCR_CLEAR_RX | FCR_CLEAR_TX);
            io_wait();
            // 调制解调器控制：DTR + RTS + OUT2
            outb(self.port + REG_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);
            io_wait();
        }
    }

    /// 发送一个字节（轮询 THR 空后再写）
    ///
    /// 为什么轮询 LSR：硬件寄存器没有"可写"信号，软件只能靠
    /// LSR.THRE 判断发送保持寄存器是否空；FIFO 模式下 THRE 表示
    /// FIFO 未满，可以直接写入。
    pub fn putc(&self, byte: u8) {
        unsafe {
            // 等待发送缓存可写（带自重试，避免无设备时永久自旋的
            // 场景由调用方保证设备存在）
            while inb(self.port + REG_LSR) & LSR_THR_EMPTY == 0 {}
            outb(self.port + REG_DATA, byte);
        }
    }

    /// 尝试读取一个字节；无数据时返回 None（非阻塞轮询）
    pub fn getc(&self) -> Option<u8> {
        unsafe {
            if inb(self.port + REG_LSR) & LSR_DATA_READY != 0 {
                Some(inb(self.port + REG_DATA))
            } else {
                None
            }
        }
    }

    /// 连续发送一段字节
    pub fn write_slice(&self, data: &[u8]) {
        for &b in data {
            self.putc(b);
        }
    }

    /// 发送字符串（UTF-8 逐字节输出）
    pub fn write_str(&self, s: &str) {
        self.write_slice(s.as_bytes());
    }

    /// 发送一行（追加 \r\n，适合终端）
    pub fn write_line(&self, s: &str) {
        self.write_str(s);
        self.putc(b'\r');
        self.putc(b'\n');
    }

    /// 端口基址（只读，供诊断输出）
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// 全局 COM1 控制台（调试串口）
///
/// 为什么初始化前也能写：QEMU/实机的 16550 复位后默认 8N1 且
/// THRE 置位，print.rs 在内核最早阶段（drivers::init 之前）镜像
/// 日志时直接 putc 即可工作；完整参数（波特率等）由 init 设置。
pub static COM1: Uart16550 = Uart16550::new(COM1_PORT);

/// 全局 COM2（备用串口，暂不启用中断）
pub static COM2: Uart16550 = Uart16550::new(COM2_PORT);

/// 初始化调试串口（COM1 + COM2）
pub fn init() {
    COM1.init();
    COM2.init();
}

/// 调试控制台单字节输出（print.rs 镜像日志用，无锁、初始化前可用）
///
/// 为什么单独提供：print.rs 只需要"往串口丢一个字节"的语义，
/// 不希望它依赖完整的 Uart16550 类型；单字节 out 本身原子。
#[inline]
pub fn console_write_byte(byte: u8) {
    COM1.putc(byte);
}