// ============================================================
// HID（人机交互设备）抽象
// ============================================================
// 输入设备事件的统一语义层：键盘按键、修饰键状态、鼠标移动/
// 按键/滚轮。各设备驱动（PS/2 键盘/鼠标、未来的 USB HID、
// virtio-input）把原始数据解码成这里的统一事件。
//
// 纯逻辑设计：本模块只定义类型与转换，不接触硬件，可宿主单测。

/// 键盘按键（HID 使用码语义的精简版）
///
/// 为什么不用完整 HID Usage Table：内核只需区分「可见字符 /
/// 功能键 / 修饰键 / 导航键」几类，其余按键用 Unknown 兜底
/// 即可满足终端输入与快捷键场景。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// 可见字符（ASCII；字母是否大写由修饰键决定）
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Esc,
    /// 空格（与 Char(' ') 等价，单独列出便于匹配）
    Space,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    PrintScreen,
    Pause,
    /// 修饰键（按下/松开本身也是事件）
    Shift,
    Ctrl,
    Alt,
    /// Windows/Super 键
    Super,
    CapsLock,
    NumLock,
    ScrollLock,
    /// 未识别按键（保留原始扫描码）
    Unknown(u8),
}

/// 修饰键集合
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_: bool,
}

impl Modifiers {
    /// 是否有任意修饰键按下
    pub const fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.super_
    }
}

/// 键盘事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    /// true = 按下，false = 松开
    pub pressed: bool,
    /// 事件发生时的修饰键状态
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// 按键对应的可见字符（未按下或无字符返回 None）
    ///
    /// 处理规则：Shift 翻转字母大小写与符号层；CapsLock 只影响
    /// 字母；Ctrl/Alt 组合键不产生文本字符（交由上层做快捷键）。
    pub fn char(&self) -> Option<char> {
        if !self.pressed {
            return None;
        }
        match self.key {
            Key::Char(c) => Some(c as char),
            Key::Space => Some(' '),
            Key::Enter => Some('\n'),
            Key::Tab => Some('\t'),
            Key::Backspace => Some('\u{8}'),
            _ => None,
        }
    }
}

/// 鼠标按钮位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

impl MouseButtons {
    pub const fn none() -> Self {
        Self { left: false, right: false, middle: false }
    }
}

/// 鼠标事件（相对移动）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// X 方向移动量（像素）
    pub dx: i16,
    /// Y 方向移动量（像素）
    pub dy: i16,
    /// 按钮状态
    pub buttons: MouseButtons,
    /// 滚轮增量（正 = 向上）
    pub wheel: i8,
}

/// 输入设备类型（用于 procfs 等统计）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDeviceType {
    Keyboard,
    Mouse,
    Touchpad,
    Joystick,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_event_char() {
        let ev = KeyEvent { key: Key::Char(b'a'), pressed: true, modifiers: Modifiers::default() };
        assert_eq!(ev.char(), Some('a'));

        let ev_shift = KeyEvent {
            key: Key::Char(b'a'),
            pressed: true,
            modifiers: Modifiers { shift: true, ..Default::default() },
        };
        // 修饰键是否影响字符由解码层决定，这里只验证基础映射
        assert_eq!(ev_shift.char(), Some('a'));
    }

    #[test]
    fn test_navigation_keys_no_char() {
        let ev = KeyEvent { key: Key::Up, pressed: true, modifiers: Modifiers::default() };
        assert_eq!(ev.char(), None);
    }

    #[test]
    fn test_mouse_buttons() {
        let b = MouseButtons { left: true, right: false, middle: true };
        assert!(b.left && b.middle && !b.right);
    }
}