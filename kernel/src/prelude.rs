// ============================================================
// AXIS 内核 Prelude
// ============================================================
// 重新导出常用的类型、trait 和宏，简化导入

// 核心类型
pub use core::fmt::{self, Write};
pub use core::mem;
pub use core::ptr;

// 同步原语
pub use crate::sync::{Spinlock, SpinlockGuard};

// 结果类型
pub use crate::libcore::result::{KernelResult, KernelError};

// 宏
pub use crate::{print, println};
