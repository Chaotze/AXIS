// ============================================================
// AXIS 内核配置
// ============================================================
// 定义内核运行时常量和配置参数

/// 内核版本信息
pub const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const KERNEL_NAME: &str = "AXIS";
pub const KERNEL_SLOGAN: &str = "AXIS eXecute Instructions Steadily";
pub const KERNEL_AUTHOR: &str = "AXIS Project from Tongji Univisity";

/// 内核启动 Banner
pub const KERNEL_BANNER: &str = r#"
    ___    _  __ ____ _____
   /   |  | |/ //  _// ___/
  / /| |  |   / / /  \__ \
 / ___ | /   |_/ /  ___/ /
/_/  |_|/_/|_/___/ /____/
"#;

/// 内核虚拟地址空间布局（x86_64）
///
/// 高半核（Higher Half Kernel）布局：
/// - 物理内存映射区：0xFFFF_8000_0000_0000 - 0xFFFF_8800_0000_0000 (映射前 512GB 物理内存)
/// - 内核代码/数据：0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_C000_0000 (1GB)
/// - 内核堆：      0xFFFF_FFFF_C000_0000 - 0xFFFF_FFFF_E000_0000 (512MB)
/// - 临时映射区：  0xFFFF_FFFF_E000_0000 - 0xFFFF_FFFF_F000_0000 (256MB)
pub const KERNEL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_HEAP_START: u64 = 0xFFFF_FFFF_C000_0000;
pub const KERNEL_HEAP_SIZE: u64 = 512 * 1024 * 1024; // 512MB

/// 页面大小
pub const PAGE_SIZE: usize = 4096;

/// 中断相关配置
pub const IDT_ENTRIES: usize = 256;

/// 定时器频率（Hz）
pub const TIMER_FREQUENCY: u32 = 1000; // 1ms per tick

/// CPU 特性标志
pub mod cpu_features {
    pub const SSE: u32 = 1 << 0;
    pub const SSE2: u32 = 1 << 1;
    pub const SSE3: u32 = 1 << 2;
    pub const SSSE3: u32 = 1 << 3;
    pub const SSE4_1: u32 = 1 << 4;
    pub const SSE4_2: u32 = 1 << 5;
    pub const AVX: u32 = 1 << 6;
    pub const AVX2: u32 = 1 << 7;
    pub const SMEP: u32 = 1 << 8;  // Supervisor Mode Execution Prevention
    pub const SMAP: u32 = 1 << 9;  // Supervisor Mode Access Prevention
    pub const XSAVE: u32 = 1 << 10;
}
