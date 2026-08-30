// ============================================================
// unitest::lib —— 对应 kernel/src/lib 的纯算法子集
// ============================================================
// 仅收录不依赖 arch/硬件/VGA 的模块；print/vga/debug 等
// 胶水层不在宿主测试范围内（由内核启动自测验证）。
// 目录层级与内核一致（lib/collections, lib/collections/lockfree），
// 保证源文件内部 super:: 相对路径两环境语义相同。

#[path = "../../../kernel/src/lib/bit.rs"]
pub mod bit;

#[path = "../../../kernel/src/lib/crc.rs"]
pub mod crc;

#[path = "../../../kernel/src/lib/hash.rs"]
pub mod hash;

#[path = "../../../kernel/src/lib/string.rs"]
pub mod string;

#[path = "../../../kernel/src/lib/time.rs"]
pub mod time;

pub mod collections;

