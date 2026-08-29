// ============================================================
// AXIS 内核构建脚本
// ============================================================
// 用于在编译时执行必要的配置和代码生成

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rustc-link-search=native={}", out_dir.display());

    // 汇编并链接汇编文件
    cl_asm("src/arch/x86_64/boot.asm", &out_dir, "boot");
    cl_asm("src/arch/x86_64/interrupt/entry.asm", &out_dir, "interrupt_entry");
    cl_asm("src/arch/x86_64/context/switch.asm", &out_dir, "context_switch");

    // 链接器脚本
    let linker_script = manifest_dir.join("kernel.ld");
    println!("cargo:rerun-if-changed={}", linker_script.display());
    println!("cargo:rustc-link-arg=-T{}", linker_script.display());
}

/// 汇编并链接汇编文件
fn cl_asm(src: &str, out_dir: &PathBuf, name: &str) {
    println!("cargo:rerun-if-changed={}", src);

    let obj_file = out_dir.join(format!("{}.o", name));

    // 使用 nasm 编译汇编文件
    let status = Command::new("nasm")
        .args(&[
            "-f", "elf64",
            "-o", obj_file.to_str().unwrap(),
            src,
        ])
        .status()
        .expect("Failed to run nasm");

    if !status.success() {
        panic!("Failed to assembly {}", src);
    }

    // 链接目标文件
    println!("cargo:rustc-link-arg={}", obj_file.display());
}
