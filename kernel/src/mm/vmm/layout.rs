// ============================================================
// 虚拟地址空间布局（Layout）
// ============================================================
// 统一定义整个 64 位虚拟地址空间的划分。内核侧已有的布局常量
// （KERNEL_BASE、PHYSICAL_MEMORY_OFFSET 等）以 config.rs 为唯一
// 事实来源（避免两处重复定义产生漂移），本模块在此之上补充
// 用户空间（User Space）布局。
//
// x86_64 地址空间（48 位规范地址）：
//   低半区 [0x0000_0000_0000_0000, 0x0000_7FFF_FFFF_FFFF) —— 用户空间
//   高半区 [0xFFFF_8000_0000_0000, 0xFFFF_FFFF_FFFF_FFFF) —— 内核空间
// 分割线即第 47 位。

// 复用 config.rs（单一事实来源）
pub use crate::config::{
    KERNEL_BASE, KERNEL_HEAP_SIZE, KERNEL_HEAP_START, PHYSICAL_MEMORY_OFFSET,
};

/// 用户空间基址：ELF 主流约定从 4MB 起加载（避开 0 页陷阱）
pub const USER_BASE: u64 = 0x400000;

/// 用户空间栈顶（低半区最高可寻址地址向下预留）
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_FFFF;
/// 用户栈默认大小（8MB）
pub const USER_STACK_SIZE: u64 = 8 * 1024 * 1024;
/// 用户栈底部地址
pub const USER_STACK_BOTTOM: u64 = USER_STACK_TOP - USER_STACK_SIZE;

/// 用户空间大小（低半区）
pub const USER_SPACE_SIZE: u64 = USER_STACK_TOP + 1;

/// 用户堆（brk）初始位置：程序加载段之后由其自身决定，这里给出
/// 一个不会撞车的最小起始值（可被 exec 时按 ELF 布局覆盖）
pub const USER_HEAP_BASE: u64 = USER_BASE + 0x100_0000;

/// 内核栈起始（每个 CPU/任务独占，里程碑阶段先约定地址）
///
/// 为什么放在布局模块：内核栈属于地址空间布局的一部分，与
/// KERNEL_BASE 等高半区常量同源，集中定义避免魔数分散。
pub const KERNEL_STACK_BASE: u64 = KERNEL_HEAP_START + KERNEL_HEAP_SIZE + 0x1_0000;
/// 每个内核栈的大小（16KB，可容纳较大的异常处理栈帧）
pub const KERNEL_STACK_SIZE: u64 = 16 * 1024;

/// 判定一段虚拟地址是否属于用户空间
#[inline]
pub const fn is_user_addr(addr: u64) -> bool {
    addr <= USER_STACK_TOP
}

/// 判定一段虚拟地址是否属于内核空间（高半区）
#[inline]
pub const fn is_kernel_addr(addr: u64) -> bool {
    addr >= KERNEL_BASE || addr >= USER_STACK_TOP + 1
}
