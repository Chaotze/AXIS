// ============================================================
// x86_64 CPU 特性检测和管理
// ============================================================
// 检测 CPU 支持的特性并启用必要的功能

use core::arch::asm;
use crate::config::cpu_features::*;

/// CPU 特性位掩码
static mut CPU_FEATURES: u32 = 0;

/// CPUID 指令结果
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct CpuidResult {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

/// 执行 CPUID 指令
///
/// CPUID 是 x86 CPU 提供的查询 CPU 信息的标准接口
/// 通过不同的 leaf（功能号）和 subleaf 可以查询不同的信息
///
/// 为什么用内联汇编：
/// - Rust 标准库的 CPUID 功能在 no_std 环境不可用
/// - 需要直接控制寄存器来获取完整的返回值
#[inline]
fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    let mut eax: u32;
    let mut ebx: u32;
    let mut ecx: u32;
    let mut edx: u32;

    unsafe {
        asm!(
            "mov {ebx_tmp:e}, ebx",  // 保存 rbx（LLVM 内部使用）
            "cpuid",
            "xchg {ebx_tmp:e}, ebx", // 恢复 rbx
            ebx_tmp = out(reg) ebx,
            inout("eax") leaf => eax,
            inout("ecx") subleaf => ecx,
            out("edx") edx,
            options(nostack, preserves_flags)
        );
    }

    CpuidResult { eax, ebx, ecx, edx }
}

/// 检测 CPU 特性
///
/// 通过 CPUID 指令检测 CPU 支持的各种特性：
/// - LEAF 1: 基础特性（SSE、SSE2、SSE3 等）
/// - LEAF 7: 扩展特性（AVX2、SMEP、SMAP 等）
fn detect_features() -> u32 {
    let mut features = 0;

    // CPUID.01H: 处理器信息和特性位
    let result = cpuid(1, 0);

    // ECX 寄存器包含的特性
    if result.ecx & (1 << 0) != 0 { features |= SSE3; }
    if result.ecx & (1 << 9) != 0 { features |= SSSE3; }
    if result.ecx & (1 << 19) != 0 { features |= SSE4_1; }
    if result.ecx & (1 << 20) != 0 { features |= SSE4_2; }
    if result.ecx & (1 << 26) != 0 { features |= XSAVE; }
    if result.ecx & (1 << 28) != 0 { features |= AVX; }

    // EDX 寄存器包含的特性
    if result.edx & (1 << 25) != 0 { features |= SSE; }
    if result.edx & (1 << 26) != 0 { features |= SSE2; }

    // CPUID.07H: 扩展特性
    let result = cpuid(7, 0);

    // EBX 寄存器包含的扩展特性
    if result.ebx & (1 << 5) != 0 { features |= AVX2; }
    if result.ebx & (1 << 7) != 0 { features |= SMEP; }
    if result.ebx & (1 << 20) != 0 { features |= SMAP; }

    features
}

/// 启用 SSE/AVX
///
/// SSE/AVX 是 x86 的 SIMD（单指令多数据）扩展，提供向量运算能力。
/// 内核需要启用这些特性，原因：
/// - 编译器可能生成使用 SSE 指令的代码（如浮点运算优化）
/// - 用户态程序需要使用这些特性
/// - 上下文切换时需要保存/恢复 SSE/AVX 寄存器
///
/// 启用步骤：
/// 1. 设置 CR0.MP = 1（启用数学协处理器）
/// 2. 清除 CR0.EM = 0（禁用仿真）
/// 3. 设置 CR4.OSFXSR = 1（启用 FXSAVE/FXRSTOR）
/// 4. 设置 CR4.OSXMMEXCPT = 1（启用 SSE 异常）
/// 5. 如果支持 AVX，设置 CR4.OSXSAVE = 1 并配置 XCR0
unsafe fn enable_sse_avx(features: u32) {
    unsafe {
        // 设置 CR0
        let mut cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0, options(nostack, preserves_flags));
        cr0 |= 1 << 1;  // CR0.MP = 1
        cr0 &= !(1 << 2); // CR0.EM = 0
        asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));

        // 设置 CR4
        let mut cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
        cr4 |= 1 << 9;  // CR4.OSFXSR = 1
        cr4 |= 1 << 10; // CR4.OSXMMEXCPT = 1

        // 如果支持 AVX，启用 XSAVE
        if features & AVX != 0 {
            cr4 |= 1 << 18; // CR4.OSXSAVE = 1
            asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));

            // 设置 XCR0 启用 AVX 状态
            // XCR0[0] = 1: x87 FPU
            // XCR0[1] = 1: SSE
            // XCR0[2] = 1: AVX
            let xcr0: u64 = 0b111;
            let eax = xcr0 as u32;
            let edx = (xcr0 >> 32) as u32;
            asm!(
                "xsetbv",
                in("ecx") 0u32,
                in("eax") eax,
                in("edx") edx,
                options(nostack, preserves_flags)
            );
        } else {
            asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
        }
    }
}

/// 启用 SMEP 和 SMAP
///
/// SMEP (Supervisor Mode Execution Prevention):
/// - 防止内核执行用户空间的代码
/// - 防御方式：ROP/JOP 攻击常试图跳转到用户空间执行 gadget
///
/// SMAP (Supervisor Mode Access Prevention):
/// - 防止内核访问用户空间的数据（除非显式允许）
/// - 防御方式：防止内核被欺骗访问恶意构造的用户数据
///
/// 启用方式：设置 CR4 的相应位
/// - CR4.SMEP = bit 20
/// - CR4.SMAP = bit 21
unsafe fn enable_smep_smap(features: u32) {
    unsafe {
        let mut cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));

        if features & SMEP != 0 {
            cr4 |= 1 << 20; // CR4.SMEP = 1
        }

        if features & SMAP != 0 {
            cr4 |= 1 << 21; // CR4.SMAP = 1
        }

        asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }
}

/// 初始化 CPU
///
/// 这是 CPU 子系统的主入口，负责：
/// 1. 检测 CPU 特性
/// 2. 启用必要的安全和性能特性
/// 3. 记录 CPU 能力供后续使用
pub unsafe fn init() {
    unsafe {
        // 检测 CPU 特性
        let features = detect_features();
        CPU_FEATURES = features;

        // 启用 SSE/AVX
        enable_sse_avx(features);

        // 启用安全特性
        enable_smep_smap(features);

        // 打印检测结果
        println!("[CPU] Features detected:");
        if features & SSE != 0 { println!("  - SSE"); }
        if features & SSE2 != 0 { println!("  - SSE2"); }
        if features & SSE3 != 0 { println!("  - SSE3"); }
        if features & SSSE3 != 0 { println!("  - SSSE3"); }
        if features & SSE4_1 != 0 { println!("  - SSE4.1"); }
        if features & SSE4_2 != 0 { println!("  - SSE4.2"); }
        if features & AVX != 0 { println!("  - AVX"); }
        if features & AVX2 != 0 { println!("  - AVX2"); }
        if features & SMEP != 0 { println!("  - SMEP"); }
        if features & SMAP != 0 { println!("  - SMAP"); }
        if features & XSAVE != 0 { println!("  - XSAVE"); }
    }
}

/// 检查 CPU 是否支持某特性
#[allow(dead_code)]
pub fn has_feature(feature: u32) -> bool {
    unsafe { CPU_FEATURES & feature != 0 }
}
