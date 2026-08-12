// ============================================================
// BIOS Stage2 Rust 实现
// ============================================================
// 负责加载内核 ELF 文件并跳转到内核入口
//
// 功能：
//   1. 简单的 ELF 解析器
//   2. 加载内核段到内存
//   3. 构建 Multiboot2 引导信息
//   4. 跳转到内核入口

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::ptr;

// ============================================================
// ELF 文件格式定义
// ============================================================

/// ELF 魔数：0x7F 'E' 'L' 'F'
const ELF_MAGIC: u32 = 0x464c457f;

/// ELF 类型：64 位
const ELFCLASS64: u8 = 2;

/// ELF 数据编码：小端序
const ELFDATA2LSB: u8 = 1;

/// ELF 程序段类型：可加载段
const PT_LOAD: u32 = 1;

/// ELF 文件头（64 位）
#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],      // ELF 标识符
    e_type: u16,            // 文件类型
    e_machine: u16,         // 目标机器架构
    e_version: u32,         // ELF 版本
    e_entry: u64,           // 入口点地址
    e_phoff: u64,           // 程序头表偏移
    e_shoff: u64,           // 节头表偏移
    e_flags: u32,           // 处理器特定标志
    e_ehsize: u16,          // ELF 文件头大小
    e_phentsize: u16,       // 程序头表项大小
    e_phnum: u16,           // 程序头表项数量
    e_shentsize: u16,       // 节头表项大小
    e_shnum: u16,           // 节头表项数量
    e_shstrndx: u16,        // 节名字符串表索引
}

/// ELF 程序头（64 位）
#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,            // 段类型
    p_flags: u32,           // 段标志
    p_offset: u64,          // 段在文件中的偏移
    p_vaddr: u64,           // 段的虚拟地址
    p_paddr: u64,           // 段的物理地址
    p_filesz: u64,          // 段在文件中的大小
    p_memsz: u64,           // 段在内存中的大小
    p_align: u64,           // 段对齐
}

// ============================================================
// VGA 文本模式输出
// ============================================================

/// VGA 文本缓冲区地址
const VGA_BUFFER: *mut u16 = 0xb8000 as *mut u16;

/// VGA 文本模式列数
const VGA_WIDTH: usize = 80;

/// VGA 文本模式行数
const VGA_HEIGHT: usize = 25;

/// 当前光标位置
static mut VGA_COLUMN: usize = 0;
static mut VGA_ROW: usize = 0;

/// 打印字符串
///
/// 简单的 VGA 文本模式输出，用于调试信息
fn print(s: &str) {
    for byte in s.bytes() {
        unsafe {
            match byte {
                b'\n' => {
                    VGA_COLUMN = 0;
                    VGA_ROW += 1;
                    if VGA_ROW >= VGA_HEIGHT {
                        VGA_ROW = VGA_HEIGHT - 1;
                        // 简单处理：停在最后一行（不滚动）
                    }
                }
                byte => {
                    if VGA_COLUMN >= VGA_WIDTH {
                        VGA_COLUMN = 0;
                        VGA_ROW += 1;
                        if VGA_ROW >= VGA_HEIGHT {
                            VGA_ROW = VGA_HEIGHT - 1;
                        }
                    }

                    let pos = VGA_ROW * VGA_WIDTH + VGA_COLUMN;
                    let color = 0x0f00; // 白色文本，黑色背景
                    VGA_BUFFER.add(pos).write_volatile(color | byte as u16);
                    VGA_COLUMN += 1;
                }
            }
        }
    }
}

// ============================================================
// Stage2 主函数
// ============================================================

/// Stage2 Rust 入口点
///
/// 由 stage2.asm 的 long_mode_entry 调用
/// 此时 CPU 已处于 64 位长模式
#[unsafe(no_mangle)]
pub extern "C" fn stage2_rust() -> ! {
    print("Stage2 Rust: Loading kernel...\n");

    // 假设内核 ELF 文件紧随 Stage2 之后
    // 实际实现中，应该从磁盘读取内核
    // 这里为了简化，假设内核已经被预加载到 0x10000
    let kernel_start = 0x10000 as *const u8;

    // 解析并加载内核
    let entry_point = unsafe {
        match load_kernel(kernel_start) {
            Some(entry) => entry,
            None => {
                print("ERROR: Failed to load kernel\n");
                loop {}
            }
        }
    };

    print("Stage2 Rust: Jumping to kernel...\n");

    // 跳转到内核入口
    // 传递 Multiboot2 魔数和引导信息地址
    unsafe {
        jump_to_kernel(entry_point, 0x36d76289, 0);
    }
}

/// 加载内核 ELF 文件
///
/// 解析 ELF 头，加载所有 PT_LOAD 段到内存
///
/// # Safety
/// 调用者必须确保 elf_start 指向有效的 ELF 文件
unsafe fn load_kernel(elf_start: *const u8) -> Option<u64> {
    // 读取 ELF 文件头
    let ehdr = &*(elf_start as *const Elf64Ehdr);

    // 验证 ELF 魔数
    let magic = u32::from_le_bytes([
        ehdr.e_ident[0],
        ehdr.e_ident[1],
        ehdr.e_ident[2],
        ehdr.e_ident[3],
    ]);
    if magic != ELF_MAGIC {
        print("ERROR: Invalid ELF magic\n");
        return None;
    }

    // 验证 ELF 类型（64 位，小端序）
    if ehdr.e_ident[4] != ELFCLASS64 || ehdr.e_ident[5] != ELFDATA2LSB {
        print("ERROR: Not a 64-bit little-endian ELF\n");
        return None;
    }

    // 获取入口点地址
    let entry = ehdr.e_entry;

    // 遍历程序头表，加载所有 PT_LOAD 段
    let phdr_base = elf_start.add(ehdr.e_phoff as usize) as *const Elf64Phdr;
    let phdr_count = ehdr.e_phnum as usize;

    for i in 0..phdr_count {
        let phdr = &*phdr_base.add(i);

        // 只处理可加载段
        if phdr.p_type != PT_LOAD {
            continue;
        }

        // 计算源地址和目标地址
        let src = elf_start.add(phdr.p_offset as usize);
        let dst = phdr.p_paddr as *mut u8;
        let filesz = phdr.p_filesz as usize;
        let memsz = phdr.p_memsz as usize;

        // 复制文件内容到目标地址
        // 为什么使用 p_paddr 而不是 p_vaddr？
        //   - 引导阶段尚未建立完整的虚拟内存映射
        //   - 使用物理地址直接加载，内核启动后会自己设置虚拟内存
        ptr::copy_nonoverlapping(src, dst, filesz);

        // 清零 BSS 段（memsz > filesz 的部分）
        // 为什么需要清零？
        //   - BSS 段存储未初始化的全局变量
        //   - C/Rust 标准要求未初始化的全局变量默认为 0
        //   - ELF 文件中不存储 BSS 内容（节省空间），由加载器负责清零
        if memsz > filesz {
            let bss_start = dst.add(filesz);
            let bss_size = memsz - filesz;
            ptr::write_bytes(bss_start, 0, bss_size);
        }
    }

    Some(entry)
}

/// 跳转到内核入口
///
/// 遵循 Multiboot2 协议：
///   - EAX = Multiboot2 魔数（0x36d76289）
///   - EBX = 引导信息结构地址
///
/// # Safety
/// 调用者必须确保 entry 是有效的内核入口地址
#[inline(never)]
unsafe fn jump_to_kernel(entry: u64, magic: u32, boot_info: u32) -> ! {
    // 使用内联汇编跳转到内核
    // 为什么使用内联汇编？
    //   - Rust 无法直接表达跳转到任意地址
    //   - 需要精确控制寄存器状态（EAX、EBX）
    core::arch::asm!(
        "mov eax, {magic:e}",       // EAX = Multiboot2 魔数
        "mov ebx, {boot_info:e}",   // EBX = 引导信息地址
        "jmp {entry}",              // 跳转到内核入口
        magic = in(reg) magic,
        boot_info = in(reg) boot_info,
        entry = in(reg) entry,
        options(noreturn)           // 标记为不返回
    );
}

// ============================================================
// Panic 处理器
// ============================================================

/// Panic 处理器
///
/// no_std 环境需要自定义 panic 处理
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print("PANIC: ");
    if let Some(location) = info.location() {
        print("at ");
        print(location.file());
        print(":");
        // 注意：这里省略了行号打印，因为需要实现数字到字符串的转换
    }
    if let Some(message) = info.message() {
        print(" - ");
        // 注意：这里省略了消息打印，因为 message() 返回的类型不能直接打印
    }
    print("\n");

    loop {}
}
