// ============================================================
// AXIS 内核构建脚本
// ============================================================
// 负责将内核中的 NASM 汇编文件编译为目标文件（.o），
// 并通过 cargo 的链接参数机制注入最终的链接过程。
//
// 为什么不用 global_asm!：
//   - 汇编文件使用 NASM 专有语法（%macro、%define、resb 等），
//     LLVM 内置汇编器无法完整支持
//   - 单独使用 NASM 汇编更符合项目「汇编与 Rust 分离」的组织方式
//
// 汇编文件清单（路径相对于 kernel/ 包目录）：
//   - src/arch/x86_64/boot.asm            内核引导入口 _boot
//   - src/arch/x86_64/interrupt/entry.asm 中断/异常入口存根
//   - src/arch/x86_64/context/switch.asm  上下文切换汇编
//
// 为什么按目标门控（仅在裸机目标注入汇编对象与链接脚本）：
//   - 单元测试以宿主目标构建（cargo test --target
//     x86_64-pc-windows-msvc）：宿主没有 _boot_rust 等符号，
//     强行链接 elf64 对象会导致"未定义符号/格式不兼容"；
//     门控后同一份源码可以同时支撑裸机构建与宿主测试

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // 需要汇编的文件（相对包根目录的路径）
    let asm_files = [
        "src/arch/x86_64/boot.asm",
        "src/arch/x86_64/interrupt/entry.asm",
        "src/arch/x86_64/context/switch.asm",
    ];

    // 目标操作系统：裸机目标为 "none"（见 x86_64-unknown-axis.json）
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_bare_metal = target_os == "none";

    // 汇编文件变化总是触发重新构建（宿主构建也能及早
    // 发现语法问题——但注意：宿主构建不实际运行 NASM）
    for asm_file in asm_files {
        println!("cargo:rerun-if-changed={asm_file}");
    }

    if !is_bare_metal {
        // 宿主构建（单元测试）：不注入任何链接参数
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 环境变量缺失"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    for asm_file in asm_files {
        // 目标文件：OUT_DIR/<文件名>.o
        let stem = Path::new(asm_file)
            .file_stem()
            .expect("汇编文件名无效")
            .to_string_lossy();
        let obj_path = out_dir.join(format!("{stem}.o"));

        // 调用 NASM 汇编（-f elf64 生成 x86_64 ELF 目标文件）
        let status = Command::new("nasm")
            .args(["-f", "elf64"])
            .arg(asm_file)
            .arg("-o")
            .arg(&obj_path)
            .status()
            .unwrap_or_else(|e| panic!("无法启动 NASM：{e}。请安装 NASM 并加入 PATH"));

        if !status.success() {
            panic!("NASM 汇编失败：{asm_file}");
        }

        // 将目标文件注入链接命令（GNU 风格 lld 接受 .o 文件）
        println!("cargo:rustc-link-arg={}", obj_path.display());
    }

    // 链接器脚本（绝对路径注入，避免工作目录歧义）
    let linker_script = manifest_dir.join("kernel.ld");
    println!("cargo:rerun-if-changed={}", linker_script.display());
    println!("cargo:rustc-link-arg=-T{}", linker_script.display());
}
