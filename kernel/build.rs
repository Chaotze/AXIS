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

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 环境变量缺失"));

    for asm_file in asm_files {
        // 源文件变化时触发重新构建
        println!("cargo:rerun-if-changed={asm_file}");

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
}
