// ============================================================
// AXIS 内核构建脚本
// ============================================================
// 负责汇编 NASM 源文件并接入内核链接，同时指定链接脚本
//
// 为什么需要 build.rs：
// - NASM 汇编文件（中断入口、上下文切换等）不经过 rustc 编译
// - 需要把汇编产物（.o）作为输入传给内核链接器
// - 需要以绝对路径指定链接脚本，避免相对路径依赖 rustc 的工作目录
//
// 依赖：NASM >= 3.02（见 dev/dev.md 构建工具要求）
// ============================================================

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

/// 需要汇编并链接进内核的 NASM 源文件（相对于 crate 根目录）
///
/// 说明：
/// - interrupt/entry.asm：中断/异常入口存根，IDT 依赖其中定义的
///   exception_*_handler / irq_*_handler 符号
/// - context/switch.asm：上下文切换（context_switch）
/// - boot.asm / localboot.asm：内核引导入口。当前入口仍为 _boot_rust
///   （由引导器在长模式下直接跳入），待引导路径重设计后再接入，故暂不汇编
const ASM_SOURCES: &[&str] = &[
    "src/arch/x86_64/interrupt/entry.asm",
    "src/arch/x86_64/context/switch.asm",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // 汇编 NASM 源文件，并把目标文件加入内核链接
    for source in ASM_SOURCES {
        let src_path = manifest_dir.join(source);
        let obj_name = format!(
            "{}.o",
            PathBuf::from(source).file_stem().unwrap().to_str().unwrap()
        );
        let obj_path = out_dir.join(obj_name);

        println!("cargo:rerun-if-changed={}", src_path.display());

        let status = Command::new("nasm")
            .args(["-f", "elf64"])
            .arg(&src_path)
            .arg("-o")
            .arg(&obj_path)
            .status()
            .expect("failed to run nasm: 请确认已安装 NASM（>= 3.02）");
        assert!(status.success(), "nasm 汇编失败: {}", source);

        println!("cargo:rustc-link-arg={}", obj_path.display());
    }

    // 链接脚本：使用绝对路径，避免相对路径依赖 rustc 的工作目录
    let ld_path = manifest_dir.join("kernel.ld");
    println!("cargo:rerun-if-changed={}", ld_path.display());
    println!("cargo:rustc-link-arg=-T{}", ld_path.display());

    // 链接脚本内容哈希写入环境变量：
    // cargo 只记录 build.rs 输出（链接参数）的指纹，参数不变则不会重新链接；
    // 该环境变量随脚本内容变化，强制触发重编/重链，避免 kernel.ld 改动不生效
    let ld_content = std::fs::read_to_string(&ld_path).unwrap();
    let mut hasher = DefaultHasher::new();
    ld_content.hash(&mut hasher);
    println!("cargo:rustc-env=KERNEL_LD_HASH={}", hasher.finish());
}
