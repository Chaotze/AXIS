// ============================================================
// PS/2 键盘驱动（扫描码集 1 解码器）
// ============================================================
// 把 8042 控制器输出的扫描码流解码成 HID 语义的 KeyEvent。
//
// 支持特性：
// - 扫描码集 1（PC/AT），make/break 码
// - E0 前缀扩展键（方向键、Insert/Delete、右 Ctrl/Alt、Super、小键盘）
// - 修饰键状态跟踪（Shift/Ctrl/Alt/Super/CapsLock）
//
// 纯逻辑设计：解码器只接收 u8 扫描码、产出 KeyEvent，不接触端口；
// 控制器访问与中断接线在 mod.rs。宿主环境可用任意扫描码序列测试。

use super::hid::{Key, KeyEvent, Modifiers};

/// 解码器状态：是否处于 E0 扩展前缀
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Normal,
    /// 收到 0xE0，下一个码是扩展键
    E0,
    /// 收到 0xE0 0x2A（PrintScreen 序列的一部分，等待 0xE0 0x37）
    E0PrintA,
    /// 收到 0xE0 0x37（PrintScreen 完成）或 0xE0 0x2A 之后直接按键
    E0PrintB,
}

/// PS/2 键盘解码器（扫描码集 1）
#[derive(Debug, Clone, Default)]
pub struct Ps2Keyboard {
    state: State,
    /// 修饰键当前状态
    modifiers: Modifiers,
    /// CapsLock 锁定状态
    caps_lock: bool,
}

impl Ps2Keyboard {
    pub const fn new() -> Self {
        Self {
            state: State::Normal,
            modifiers: Modifiers { shift: false, ctrl: false, alt: false, super_: false },
            caps_lock: false,
        }
    }

    /// 当前修饰键状态
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// CapsLock 状态
    pub const fn caps_lock(&self) -> bool {
        self.caps_lock
    }

    /// 喂入一个扫描码，返回可能产生的一个事件
    ///
    /// 多数扫描码不产生事件（如 E0 前缀本身）；一个物理按键
    /// 通常产生 make 与 break 两个事件。
    pub fn feed(&mut self, code: u8) -> Option<KeyEvent> {
        // 先处理扩展前缀状态机
        match self.state {
            State::Normal => {
                if code == 0xE0 {
                    self.state = State::E0;
                    return None;
                }
                self.decode_normal(code)
            }
            State::E0 => {
                // 0xE0 0x2A / 0xE0 0x37：PrintScreen 序列开始
                if code == 0x2A || code == 0x37 {
                    self.state = State::E0PrintA;
                    return None;
                }
                self.state = State::Normal;
                self.decode_extended(code)
            }
            State::E0PrintA => {
                self.state = State::Normal;
                if code == 0xE0 {
                    self.state = State::E0PrintB;
                    return None;
                }
                // 0xE0 0x2A 之后直接是普通键：当作 PrintScreen 按下
                let ev = self.make_event(Key::PrintScreen, true);
                // 丢弃多余状态：把当前码当作普通扫描码喂入
                self.decode_normal(code).or(ev)
            }
            State::E0PrintB => {
                self.state = State::Normal;
                if code == 0x37 {
                    return self.make_event(Key::PrintScreen, true);
                }
                // 异常序列：按 break 处理
                self.make_event(Key::PrintScreen, false)
            }
        }
    }

    /// 普通扫描码解码
    fn decode_normal(&mut self, code: u8) -> Option<KeyEvent> {
        let pressed = code < 0x80;
        let sc = if pressed { code } else { code - 0x80 };

        // 修饰键：先更新状态，再产出事件
        let key = match sc {
            0x1D => {
                self.modifiers.ctrl = pressed;
                Key::Ctrl
            }
            0x2A => {
                self.modifiers.shift = pressed;
                Key::Shift
            }
            0x36 => {
                self.modifiers.shift = pressed;
                Key::Shift
            }
            0x38 => {
                self.modifiers.alt = pressed;
                Key::Alt
            }
            0x3A => {
                if pressed {
                    self.caps_lock = !self.caps_lock;
                }
                Key::CapsLock
            }
            0x45 => Key::NumLock,
            0x46 => Key::ScrollLock,
            _ => self.base_key(sc)?,
        };
        self.make_event(key, pressed)
    }

    /// E0 扩展扫描码解码
    fn decode_extended(&mut self, code: u8) -> Option<KeyEvent> {
        let pressed = code < 0x80;
        let sc = if pressed { code } else { code - 0x80 };

        let key = match sc {
            0x1C => Key::Enter,        // 小键盘 Enter
            0x1D => {
                self.modifiers.ctrl = pressed;
                Key::Ctrl                // 右 Ctrl
            }
            0x35 => Key::Unknown(sc), // 小键盘 /
            0x38 => {
                self.modifiers.alt = pressed;
                Key::Alt                 // 右 Alt（AltGr）
            }
            0x47 => Key::Home,
            0x48 => Key::Up,
            0x49 => Key::PageUp,
            0x4B => Key::Left,
            0x4D => Key::Right,
            0x4F => Key::End,
            0x50 => Key::Down,
            0x51 => Key::PageDown,
            0x52 => Key::Insert,
            0x53 => Key::Delete,
            0x5B | 0x5C => {
                self.modifiers.super_ = pressed;
                Key::Super
            }
            _ => Key::Unknown(sc),
        };
        self.make_event(key, pressed)
    }

    /// 基础扫描码 → 按键（不含修饰键与锁）
    fn base_key(&self, sc: u8) -> Option<Key> {
        let key = match sc {
            0x01 => Key::Esc,
            0x02 => Key::Char(b'1'),
            0x03 => Key::Char(b'2'),
            0x04 => Key::Char(b'3'),
            0x05 => Key::Char(b'4'),
            0x06 => Key::Char(b'5'),
            0x07 => Key::Char(b'6'),
            0x08 => Key::Char(b'7'),
            0x09 => Key::Char(b'8'),
            0x0A => Key::Char(b'9'),
            0x0B => Key::Char(b'0'),
            0x0C => Key::Char(b'-'),
            0x0D => Key::Char(b'='),
            0x0E => Key::Backspace,
            0x0F => Key::Tab,
            0x10 => Key::Char(b'q'),
            0x11 => Key::Char(b'w'),
            0x12 => Key::Char(b'e'),
            0x13 => Key::Char(b'r'),
            0x14 => Key::Char(b't'),
            0x15 => Key::Char(b'y'),
            0x16 => Key::Char(b'u'),
            0x17 => Key::Char(b'i'),
            0x18 => Key::Char(b'o'),
            0x19 => Key::Char(b'p'),
            0x1A => Key::Char(b'['),
            0x1B => Key::Char(b']'),
            0x1C => Key::Enter,
            0x1E => Key::Char(b'a'),
            0x1F => Key::Char(b's'),
            0x20 => Key::Char(b'd'),
            0x21 => Key::Char(b'f'),
            0x22 => Key::Char(b'g'),
            0x23 => Key::Char(b'h'),
            0x24 => Key::Char(b'j'),
            0x25 => Key::Char(b'k'),
            0x26 => Key::Char(b'l'),
            0x27 => Key::Char(b';'),
            0x28 => Key::Char(b'\''),
            0x29 => Key::Char(b'`'),
            0x2B => Key::Char(b'\\'),
            0x2C => Key::Char(b'z'),
            0x2D => Key::Char(b'x'),
            0x2E => Key::Char(b'c'),
            0x2F => Key::Char(b'v'),
            0x30 => Key::Char(b'b'),
            0x31 => Key::Char(b'n'),
            0x32 => Key::Char(b'm'),
            0x33 => Key::Char(b','),
            0x34 => Key::Char(b'.'),
            0x35 => Key::Char(b'/'),
            0x39 => Key::Space,
            0x3B => Key::F1,
            0x3C => Key::F2,
            0x3D => Key::F3,
            0x3E => Key::F4,
            0x3F => Key::F5,
            0x40 => Key::F6,
            0x41 => Key::F7,
            0x42 => Key::F8,
            0x43 => Key::F9,
            0x44 => Key::F10,
            0x57 => Key::F11,
            0x58 => Key::F12,
            _ => return None,
        };
        Some(key)
    }

    /// 构造事件（修饰键已被本函数调用前更新）
    fn make_event(&self, key: Key, pressed: bool) -> Option<KeyEvent> {
        Some(KeyEvent { key, pressed, modifiers: self.modifiers })
    }
}

/// 把基础字符键翻译成可见字符（受 Shift / CapsLock 影响）
///
/// 纯函数，供需要文本输入的消费方使用；快捷键场景直接用 KeyEvent。
pub fn char_with_modifiers(key: Key, shift: bool, caps: bool) -> Option<char> {
    let Key::Char(c) = key else {
        // 非字符键没有文本层
        return match key {
            Key::Space => Some(' '),
            Key::Enter => Some('\n'),
            Key::Tab => Some('\t'),
            Key::Backspace => Some('\u{8}'),
            _ => None,
        };
    };
    let upper = shift ^ (caps && c.is_ascii_lowercase());
    if c.is_ascii_alphabetic() {
        return Some(if upper { (c as char).to_ascii_uppercase() } else { c as char });
    }
    // 数字与符号层
    if upper {
        Some(match c {
            b'1' => '!', b'2' => '@', b'3' => '#', b'4' => '$', b'5' => '%',
            b'6' => '^', b'7' => '&', b'8' => '*', b'9' => '(', b'0' => ')',
            b'-' => '_', b'=' => '+', b'[' => '{', b']' => '}', b'\\' => '|',
            b';' => ':', b'\'' => '"', b',' => '<', b'.' => '>', b'/' => '?',
            b'`' => '~', _ => c as char,
        })
    } else {
        Some(c as char)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_letter() {
        let mut kbd = Ps2Keyboard::new();
        // 按 a（0x1E），松开 a（0x9E）
        let ev = kbd.feed(0x1E).unwrap();
        assert_eq!(ev, KeyEvent { key: Key::Char(b'a'), pressed: true, modifiers: Modifiers::default() });
        let ev = kbd.feed(0x9E).unwrap();
        assert!(!ev.pressed);
    }

    #[test]
    fn test_shift_letter() {
        let mut kbd = Ps2Keyboard::new();
        kbd.feed(0x2A); // LShift 按下
        let ev = kbd.feed(0x1E).unwrap(); // a
        assert!(ev.modifiers.shift);
        let ch = char_with_modifiers(ev.key, ev.modifiers.shift, kbd.caps_lock());
        assert_eq!(ch, Some('A'));
        kbd.feed(0xAA); // LShift 松开
        let ev = kbd.feed(0x1E).unwrap();
        assert!(!ev.modifiers.shift);
    }

    #[test]
    fn test_extended_arrows() {
        let mut kbd = Ps2Keyboard::new();
        assert!(kbd.feed(0xE0).is_none());
        let ev = kbd.feed(0x48).unwrap(); // Up
        assert_eq!(ev.key, Key::Up);
        assert!(ev.pressed);
        assert!(kbd.feed(0xE0).is_none());
        let ev = kbd.feed(0xC8).unwrap(); // Up 松开（同样 E0 前缀）
        assert_eq!(ev.key, Key::Up);
        assert!(!ev.pressed);
    }

    #[test]
    fn test_caps_lock() {
        let mut kbd = Ps2Keyboard::new();
        kbd.feed(0x3A); // CapsLock 按下
        assert!(kbd.caps_lock());
        kbd.feed(0xBA); // CapsLock 松开
        assert!(kbd.caps_lock()); // 锁存状态保持
        let ev = kbd.feed(0x1E).unwrap();
        let ch = char_with_modifiers(ev.key, false, kbd.caps_lock());
        assert_eq!(ch, Some('A'));
    }

    #[test]
    fn test_unknown_key() {
        let mut kbd = Ps2Keyboard::new();
        // 0x7F 是未定义扫描码
        assert!(kbd.feed(0x7F).is_none());
    }

    #[test]
    fn test_modifier_tracking() {
        let mut kbd = Ps2Keyboard::new();
        kbd.feed(0x1D); // LCtrl
        assert!(kbd.modifiers().ctrl);
        kbd.feed(0x9D); // LCtrl 松开
        assert!(!kbd.modifiers().ctrl);
        kbd.feed(0xE0);
        kbd.feed(0x1D); // RCtrl
        assert!(kbd.modifiers().ctrl);
    }
}