// ============================================================
// 调试工具
// ============================================================
// 提供内核调试功能

/// 打印十六进制内存转储
///
/// 用于调试时查看内存内容
pub fn hexdump(data: &[u8], base_addr: usize) {
    const BYTES_PER_LINE: usize = 16;

    for (i, chunk) in data.chunks(BYTES_PER_LINE).enumerate() {
        let addr = base_addr + i * BYTES_PER_LINE;

        // 打印地址
        print!("{:08X}  ", addr);

        // 打印十六进制
        for (j, &byte) in chunk.iter().enumerate() {
            print!("{:02X} ", byte);
            if j == 7 {
                print!(" ");
            }
        }

        // 填充空白（如果不足一行）
        for j in chunk.len()..BYTES_PER_LINE {
            print!("   ");
            if j == 7 {
                print!(" ");
            }
        }

        // 打印 ASCII
        print!(" |");
        for &byte in chunk {
            let ch = if (0x20..=0x7e).contains(&byte) {
                byte as char
            } else {
                '.'
            };
            print!("{}", ch);
        }
        println!("|");
    }
}

/// 断言宏（panic 版本）
///
/// 在条件不满足时触发 panic
#[macro_export]
macro_rules! assert {
    ($cond:expr) => {
        if !$cond {
            panic!("Assertion failed: {}", stringify!($cond));
        }
    };
    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            panic!("Assertion failed: {}: {}", stringify!($cond), format_args!($($arg)*));
        }
    };
}

/// 相等断言
#[macro_export]
macro_rules! assert_eq {
    ($left:expr, $right:expr) => {
        match (&$left, &$right) {
            (left_val, right_val) => {
                if !(*left_val == *right_val) {
                    panic!(
                        "Assertion failed: {} == {}\n  left: {:?}\n right: {:?}",
                        stringify!($left),
                        stringify!($right),
                        left_val,
                        right_val
                    );
                }
            }
        }
    };
}

/// 不相等断言
#[macro_export]
macro_rules! assert_ne {
    ($left:expr, $right:expr) => {
        match (&$left, &$right) {
            (left_val, right_val) => {
                if *left_val == *right_val {
                    panic!(
                        "Assertion failed: {} != {}\n  left: {:?}\n right: {:?}",
                        stringify!($left),
                        stringify!($right),
                        left_val,
                        right_val
                    );
                }
            }
        }
    };
}
