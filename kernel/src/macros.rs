// ============================================================
// AXIS 内核宏定义
// ============================================================
// 提供打印等常用宏

/// print! 宏
///
/// 类似标准库的 print!，但输出到内核控制台（VGA 文本模式）
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::lib::print::_print(format_args!($($arg)*))
    };
}

/// println! 宏
///
/// 类似标准库的 println!，输出后自动换行
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
