// ============================================================
// PS/2 鼠标驱动（数据包解码）
// ============================================================
// 把 PS/2 鼠标（标准 3 键或带滚轮的 Intellimouse）输出的数据包
// 解码成 HID 语义的 MouseEvent。
//
// 数据包格式：
// - 标准模式：3 字节 [flags, dx, dy]
//   flags: bit0=左键, bit1=右键, bit2=中键, bit4=x 符号, bit5=y 符号,
//          bit6=x 溢出, bit7=y 溢出
// - 滚轮模式（Intellimouse，4 字节）：第 4 字节低 4 位为滚轮增量
//   （9 位有符号，第 3 位扩展符号）
//
// 纯逻辑设计：解码器只接收 u8 数据流，不接触端口，可宿主单测。

use super::hid::{MouseButtons, MouseEvent};

/// 解码器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    /// 等待包首字节
    #[default]
    Start,
    /// 已收 1 字节，等待 dx
    Dx,
    /// 已收 2 字节，等待 dy
    Dy,
    /// 已收 3 字节；4 字节模式等待滚轮字节，否则吐包
    Done,
}

/// PS/2 鼠标解码器
#[derive(Debug, Clone, Default)]
pub struct Ps2Mouse {
    state: State,
    /// 是否 4 字节模式（滚轮）
    wheel: bool,
    flags: u8,
    dx: u8,
    dy: u8,
}

impl Ps2Mouse {
    pub const fn new() -> Self {
        Self {
            state: State::Start,
            wheel: false,
            flags: 0,
            dx: 0,
            dy: 0,
        }
    }

    /// 设置 4 字节（滚轮）模式；在发送 mode 命令后调用
    pub fn set_wheel(&mut self, on: bool) {
        self.wheel = on;
    }

    /// 当前是否滚轮模式
    pub const fn has_wheel(&self) -> bool {
        self.wheel
    }

    /// 喂入一个数据字节，攒够一包时返回 MouseEvent
    pub fn feed(&mut self, byte: u8) -> Option<MouseEvent> {
        loop {
            match self.state {
                State::Start => {
                    // 包头第三位必须为 1（来自数据端口同步位）；
                    // 若不同步则丢弃该字节，等待真正的包头
                    if byte & 0x08 != 0 {
                        self.flags = byte;
                        self.state = State::Dx;
                    }
                    // 未同步的字节不产生事件，也不会推进到 Dx
                    return None;
                }
                State::Dx => {
                    self.dx = byte;
                    self.state = State::Dy;
                    return None;
                }
                State::Dy => {
                    self.dy = byte;
                    self.state = if self.wheel { State::Done } else { State::Start };
                    // 非滚轮模式：此时包已完整
                    if !self.wheel {
                        return Some(self.emit(0));
                    }
                    return None;
                }
                State::Done => {
                    // 第 4 字节：低 4 位滚轮增量（第 3 位是符号扩展位）
                    let wheel = ((byte & 0x0F) << 4) as i8 >> 4;
                    self.state = State::Start;
                    return Some(self.emit(wheel));
                }
            }
        }
    }

    /// 组装事件（dx/dy 为 8 位，按符号位扩展为 9 位并有符号）
    fn emit(&self, wheel: i8) -> MouseEvent {
        MouseEvent {
            dx: ((self.dx as i8) as i16),
            dy: ((self.dy as i8) as i16),
            buttons: MouseButtons {
                left: self.flags & 0x01 != 0,
                right: self.flags & 0x02 != 0,
                middle: self.flags & 0x04 != 0,
            },
            wheel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 组装一个 3 字节包并喂给解码器
    fn feed_packet(mouse: &mut Ps2Mouse, flags: u8, dx: u8, dy: u8) -> Option<MouseEvent> {
        mouse.feed(flags | 0x08); // 置同步位
        mouse.feed(dx);
        mouse.feed(dy)
    }

    #[test]
    fn test_basic_move() {
        let mut mouse = Ps2Mouse::new();
        let ev = feed_packet(&mut mouse, 0x00, 3, 0xFC).unwrap(); // dx=3, dy=-4
        assert_eq!(ev.dx, 3);
        assert_eq!(ev.dy, -4);
        assert!(!ev.buttons.left && !ev.buttons.right && !ev.buttons.middle);
    }

    #[test]
    fn test_button_press() {
        let mut mouse = Ps2Mouse::new();
        let ev = feed_packet(&mut mouse, 0x01, 0, 0).unwrap();
        assert!(ev.buttons.left);
        let ev = feed_packet(&mut mouse, 0x06, 0, 0).unwrap();
        assert!(ev.buttons.right && ev.buttons.middle);
    }

    #[test]
    fn test_resync_bad_byte() {
        let mut mouse = Ps2Mouse::new();
        // 喂一个非包头字节（bit3=0），应被丢弃
        // 注：真正的 ACK 0xFA 恰好置了 bit3，会被误判为包头——
        // 这是 PS/2 同步启发式的已知怪癖，驱动通过复位重同步
        assert!(mouse.feed(0x04).is_none());
        // 继续收正常包
        let ev = feed_packet(&mut mouse, 0x02, 10, 5).unwrap();
        assert_eq!(ev.dx, 10);
        assert_eq!(ev.dy, 5);
        assert!(ev.buttons.right);
    }

    #[test]
    fn test_wheel() {
        let mut mouse = Ps2Mouse::new();
        mouse.set_wheel(true);
        mouse.feed(0x08 | 0x00); // flags
        mouse.feed(1);           // dx
        mouse.feed(2);           // dy（第 3 字节后进入 Done，尚无事件）
        let ev = mouse.feed(0x01).unwrap();  // 第 4 字节：滚轮 +1
        assert_eq!(ev.wheel, 1);
        assert_eq!(ev.dx, 1);

        // 滚轮 -1（0x0F 的第 3 位符号扩展）
        mouse.feed(0x08);
        mouse.feed(0);
        mouse.feed(0);
        let ev = mouse.feed(0x0F).unwrap();
        assert_eq!(ev.wheel, -1);
    }

    #[test]
    fn test_negative_delta() {
        let mut mouse = Ps2Mouse::new();
        let ev = feed_packet(&mut mouse, 0x10, 0xFF, 0x01).unwrap(); // dx 符号位
        assert_eq!(ev.dx, -1);
        assert_eq!(ev.dy, 1);
    }
}