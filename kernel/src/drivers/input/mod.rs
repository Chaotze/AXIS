// ============================================================
// 输入设备子系统
// ============================================================
// 8042（PS/2 控制器）驱动：控制器初始化、键盘/鼠标设备配置、
// IRQ1/IRQ12 中断接线，以及解码后事件的全局队列。
//
// 分层：
//   hid.rs      —— 统一事件类型（Key/KeyEvent/MouseEvent）
//   keyboard.rs —— PS/2 扫描码集 1 解码器（纯逻辑）
//   mouse.rs    —— PS/2 鼠标数据包解码器（纯逻辑）
//   mod.rs      —— 控制器访问、全局状态、中断处理、初始化
//
// 中断纪律：IRQ 处理路径只用 try_lock 访问设备队列——若主线程
// 正持锁（如打印/初始化），直接丢弃本次字节而非自旋，避免
// 「中断上下文等待主线程锁」的死锁。串口驱动的 irqsave 约定
// 同样适用于本模块的持锁操作方（调用方负责）。

pub mod hid;
pub mod keyboard;
pub mod mouse;

use alloc::collections::VecDeque;

use crate::arch::x86_64::io::{inb, outb};
use crate::sync::Spinlock;

use self::hid::{KeyEvent, MouseEvent};

// ------------------------------------------------------------
// 8042 控制器寄存器与命令
// ------------------------------------------------------------

/// 数据端口（读写数据 / 回显设备输出）
const PORT_DATA: u16 = 0x60;
/// 状态端口（只读）
const PORT_STATUS: u16 = 0x64;
/// 命令端口（写命令）
const PORT_COMMAND: u16 = 0x64;

/// 状态寄存器位
const STATUS_OUTPUT_FULL: u8 = 0x01; // 输出缓冲有数据（可读）
const STATUS_INPUT_FULL: u8 = 0x02;  // 输入缓冲满（不可写）

/// 控制器命令
const CMD_SELF_TEST: u8 = 0xAA;
const CMD_DISABLE_KEYBOARD: u8 = 0xAD;
const CMD_ENABLE_KEYBOARD: u8 = 0xAE;
const CMD_DISABLE_MOUSE: u8 = 0xA7;
const CMD_ENABLE_MOUSE: u8 = 0xA8;
/// 转发下一个数据字节到鼠标设备（aux 通道）
const CMD_MOUSE_CMD: u8 = 0xD4;

/// 设备命令回执
const DEV_ACK: u8 = 0xFA;
/// 设备命令：设置默认参数
const DEV_SET_DEFAULTS: u8 = 0xF6;
/// 设备命令：启用扫描（键盘）/ 启用数据（鼠标）
const DEV_ENABLE: u8 = 0xF4;
/// 设备命令：设置采样率（鼠标滚轮模式切换用）
const DEV_SET_SAMPLE: u8 = 0xF3;

/// 事件队列上限（防御驱动异常刷屏）
const EVENT_QUEUE_LIMIT: usize = 64;

// ------------------------------------------------------------
// 控制器低层 I/O
// ------------------------------------------------------------

/// 等待输入缓冲空（可以写数据/命令）
fn wait_write_ready() {
    unsafe {
        while inb(PORT_STATUS) & STATUS_INPUT_FULL != 0 {
            core::hint::spin_loop();
        }
    }
}

/// 等待输出缓冲满（可以读数据）
#[allow(dead_code)]
fn wait_read_ready() {
    unsafe {
        while inb(PORT_STATUS) & STATUS_OUTPUT_FULL == 0 {
            core::hint::spin_loop();
        }
    }
}

/// 写控制器命令
unsafe fn write_command(cmd: u8) {
    unsafe {
        wait_write_ready();
        outb(PORT_COMMAND, cmd);
    }
}

/// 写数据字节（给键盘或经 0xD4 给鼠标）
unsafe fn write_data(byte: u8) {
    unsafe {
        wait_write_ready();
        outb(PORT_DATA, byte);
    }
}

/// 读数据字节（如无数据返回 None）
unsafe fn read_data() -> Option<u8> {
    unsafe {
        if inb(PORT_STATUS) & STATUS_OUTPUT_FULL != 0 {
            Some(inb(PORT_DATA))
        } else {
            None
        }
    }
}

/// 向键盘设备发送命令并等待 ACK
fn keyboard_command(byte: u8) -> Option<u8> {
    unsafe {
        write_data(byte);
        // 等待回执；ACK 通常立即返回
        for _ in 0..1000 {
            if let Some(b) = read_data() {
                return Some(b);
            }
            core::hint::spin_loop();
        }
    }
    None
}

/// 向鼠标设备发送命令并等待 ACK（经 0xD4 转发）
fn mouse_command(byte: u8) -> Option<u8> {
    unsafe {
        write_command(CMD_MOUSE_CMD);
        write_data(byte);
        for _ in 0..1000 {
            if let Some(b) = read_data() {
                return Some(b);
            }
            core::hint::spin_loop();
        }
    }
    None
}

// ------------------------------------------------------------
// 全局设备状态
// ------------------------------------------------------------

/// 键盘全局状态：解码器 + 事件队列
struct KeyboardState {
    decoder: keyboard::Ps2Keyboard,
    events: VecDeque<KeyEvent>,
}

/// 鼠标全局状态：解码器 + 事件队列
struct MouseState {
    decoder: mouse::Ps2Mouse,
    events: VecDeque<MouseEvent>,
}

static KEYBOARD: Spinlock<KeyboardState> = Spinlock::new(KeyboardState {
    decoder: keyboard::Ps2Keyboard::new(),
    events: VecDeque::new(),
});

static MOUSE: Spinlock<MouseState> = Spinlock::new(MouseState {
    decoder: mouse::Ps2Mouse::new(),
    events: VecDeque::new(),
});

// ------------------------------------------------------------
// 初始化
// ------------------------------------------------------------

/// 输入子系统初始化
///
/// 流程：
/// 1. 8042 自检并复位键盘/鼠标通道
/// 2. 启用设备扫描/上报
/// 3. 请求鼠标滚轮模式（失败则保持 3 字节模式）
/// 4. 在 I/O APIC 中放开 IRQ1（键盘）与 IRQ12（鼠标）
///
/// 为什么在初始化末尾才开启 IRQ：在此之前控制器传来的任何
/// 中断都会被当作未知 IRQ 打印，且设备尚未就绪。
pub fn init() {
    // 1. 禁用设备，复位控制器
    unsafe {
        write_command(CMD_DISABLE_KEYBOARD);
        write_command(CMD_DISABLE_MOUSE);
    }
    // 清空输出缓冲（复位期间的残留数据）
    while unsafe { read_data() }.is_some() {}

    // 2. 控制器自检：0x55 = 成功
    unsafe {
        write_command(CMD_SELF_TEST);
    }
    let self_test = unsafe { read_data() };
    // QEMU 实测自检返回 0x55；不强制要求（某些虚拟化环境不一致）

    // 3. 重新启用设备
    unsafe {
        write_command(CMD_ENABLE_KEYBOARD);
        write_command(CMD_ENABLE_MOUSE);
    }

    // 4. 键盘：恢复默认参数 + 启用扫描
    let kbd_ok = keyboard_command(DEV_SET_DEFAULTS).map(|a| a == DEV_ACK).unwrap_or(false);
    let kbd_ok = keyboard_command(DEV_ENABLE).map(|a| a == DEV_ACK).unwrap_or(false) && kbd_ok;

    // 5. 鼠标：恢复默认 + 启用；随后尝试切换 4 字节滚轮模式
    let mouse_ok = mouse_command(DEV_SET_DEFAULTS).map(|a| a == DEV_ACK).unwrap_or(false);
    let mouse_ok = mouse_command(DEV_ENABLE).map(|a| a == DEV_ACK).unwrap_or(false) && mouse_ok;

    // Intellimouse 滚轮模式：采样率 200→100→80 组合使鼠标进入 4 字节模式
    let mut wheel_ok = false;
    if mouse_ok {
        let seq = [200u8, 100u8, 80u8];
        let mut all_acked = true;
        for rate in seq {
            if mouse_command(DEV_SET_SAMPLE).map(|a| a == DEV_ACK).unwrap_or(false)
                && mouse_command(rate).map(|a| a == DEV_ACK).unwrap_or(false)
            {
                // 逐次成功
            } else {
                all_acked = false;
                break;
            }
        }
        wheel_ok = all_acked;
    }
    if wheel_ok {
        MOUSE.lock().decoder.set_wheel(true);
    }

    // 6. 放开 IRQ1 / IRQ12（键盘/鼠标）
    unsafe {
        crate::arch::x86_64::interrupt::ioapic::set_entry(1, 33, 0);
        crate::arch::x86_64::interrupt::ioapic::set_entry(12, 44, 0);
    }

    println!("[INPUT] PS/2 controller: self_test={:#x} keyboard={} mouse={} wheel={}",
        self_test.unwrap_or(0), kbd_ok, mouse_ok, wheel_ok);
}

// ------------------------------------------------------------
// 中断处理（由 arch handler 调用）
// ------------------------------------------------------------

/// IRQ1（键盘）中断处理：读取扫描码并解码入队
///
/// 为什么 try_lock：中断可能在任何时刻打断主线程的持锁临界区；
/// 若强行加锁会死锁，丢一个字节比死锁更可接受。
pub fn keyboard_irq() {
    let Some(byte) = (unsafe { read_data() }) else { return };
    let Some(mut kb) = KEYBOARD.try_lock() else { return };
    if let Some(ev) = kb.decoder.feed(byte) {
        if kb.events.len() < EVENT_QUEUE_LIMIT {
            kb.events.push_back(ev);
        }
    }
}

/// IRQ12（鼠标）中断处理：读取数据包字节并解码入队
pub fn mouse_irq() {
    let Some(byte) = (unsafe { read_data() }) else { return };
    let Some(mut ms) = MOUSE.try_lock() else { return };
    if let Some(ev) = ms.decoder.feed(byte) {
        if ms.events.len() < EVENT_QUEUE_LIMIT {
            ms.events.push_back(ev);
        }
    }
}

// ------------------------------------------------------------
// 事件消费接口
// ------------------------------------------------------------

/// 取一个键盘事件（无事件返回 None）
pub fn keyboard_event() -> Option<KeyEvent> {
    KEYBOARD.lock().events.pop_front()
}

/// 取一个鼠标事件（无事件返回 None）
pub fn mouse_event() -> Option<MouseEvent> {
    MOUSE.lock().events.pop_front()
}

/// 键盘事件队列是否为空
pub fn keyboard_empty() -> bool {
    KEYBOARD.lock().events.is_empty()
}

/// 鼠标事件队列是否为空
pub fn mouse_empty() -> bool {
    MOUSE.lock().events.is_empty()
}

// ------------------------------------------------------------
// 启动自测
// ------------------------------------------------------------

/// 输入子系统自测
///
/// 核心验证解码器（纯逻辑）：扫描码序列与鼠标数据包合成事件。
/// 硬件侧（8042 是否存在）已在 init 打印，IRQ 路径由 QEMU 实测。
pub fn selftest() -> bool {
    use self::hid::Key;

    let mut all = true;
    let t = |name: &str, ok: bool| {
        println!("    [{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        ok
    };

    // 键盘：'a' 与 E0 方向键
    let mut kbd = keyboard::Ps2Keyboard::new();
    let ev = kbd.feed(0x1E).unwrap();
    all &= t("keyboard scancode", ev.pressed && ev.key == Key::Char(b'a'));
    kbd.feed(0xE0);
    let ev = kbd.feed(0x48).unwrap();
    all &= t("keyboard E0 prefix", ev.key == Key::Up);

    // 修饰键与字符层
    kbd.feed(0x2A); // Shift
    let ev = kbd.feed(0x10).unwrap(); // 'q'
    let ch = keyboard::char_with_modifiers(ev.key, ev.modifiers.shift, kbd.caps_lock());
    all &= t("keyboard shift layer", ch == Some('Q'));

    // 鼠标：3 字节包与滚轮 4 字节包
    let mut mouse = mouse::Ps2Mouse::new();
    mouse.feed(0x08 | 0x00); // 包首字节（同步位置位）
    mouse.feed(0x04);        // dx
    let ev = mouse.feed(0xFC).unwrap(); // dy（第 3 字节即返回事件）
    all &= t("mouse 3-byte packet", ev.dx == 4 && ev.dy == -4);

    mouse.set_wheel(true);
    mouse.feed(0x08); // 包首字节（同步位置位）
    mouse.feed(0);    // dx
    mouse.feed(0);    // dy
    let ev = mouse.feed(0x01).unwrap(); // 第 4 字节：滚轮 +1
    all &= t("mouse wheel packet", ev.wheel == 1);

    all
}