fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    // 仅在目标平台为 x86_64-none（裸机/无操作系统）时，才将链接器脚本传给 rustc。
    // 注意：此处的判断依据是“目标构建”的架构和操作系统，而非运行构建脚本的主机平台。
    // 链接器脚本（linker.ld）定义了内存布局、段地址等，是裸机程序正常运行所必需的。
    // 如果是在非裸机目标（如 Linux、Windows）上构建（例如运行单元测试），则不添加该脚本，
    // 因为系统自带的链接器配置已足够，且添加额外的 -T 参数反而可能导致链接失败。
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "x86_64"
        && std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "none" {
        println!("cargo:rustc-link-arg=-T");
        println!("cargo:rustc-link-arg={}/linker.ld", crate_dir);
    }

    // 当 linker.ld 文件发生变更时，重新运行构建脚本，确保链接器脚本的改动被生效。
    println!("cargo:rerun-if-changed=linker.ld");
}
