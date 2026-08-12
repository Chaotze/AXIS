// ============================================================
// UEFI Bootloader 主入口
// ============================================================
// UEFI 引导加载程序，利用 UEFI 固件服务加载内核
//
// 功能：
//   1. 初始化 UEFI 服务
//   2. 获取内存映射
//   3. 加载内核 ELF 文件
//   4. 初始化图形输出（GOP）
//   5. ExitBootServices 并跳转到内核

#![no_std]
#![no_main]

mod graphics;
mod memory;

use uefi::prelude::*;
use uefi::table::boot::{AllocateType, MemoryType};
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::Result;
use core::slice;
use core::ptr;

// ============================================================
// ELF 文件格式定义
// ============================================================

const ELF_MAGIC: u32 = 0x464c457f;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const PT_LOAD: u32 = 1;

#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

// ============================================================
// UEFI 主入口点
// ============================================================

/// UEFI 应用程序入口点
///
/// UEFI 固件调用此函数启动引导加载程序
///
/// # 参数
/// - `image_handle`: 当前镜像的句柄
/// - `system_table`: UEFI 系统表，提供所有 UEFI 服务的访问入口
#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // 初始化 UEFI 服务（日志、panic handler 等）
    uefi::helpers::init(&mut system_table).unwrap();

    let boot_services = system_table.boot_services();

    writeln!(system_table.stdout(), "AXIS UEFI Bootloader").unwrap();
    writeln!(system_table.stdout(), "Loading kernel...").unwrap();

    // 1. 加载内核 ELF 文件
    let kernel_entry = match load_kernel(image_handle, boot_services) {
        Ok(entry) => {
            writeln!(system_table.stdout(), "Kernel loaded at 0x{:x}", entry).unwrap();
            entry
        }
        Err(e) => {
            writeln!(system_table.stdout(), "Failed to load kernel: {:?}", e).unwrap();
            return Status::LOAD_ERROR;
        }
    };

    // 2. 初始化图形输出（GOP）
    let framebuffer = match graphics::init_graphics(boot_services) {
        Ok(fb) => {
            writeln!(
                system_table.stdout(),
                "Graphics initialized: {}x{} @ 0x{:x}",
                fb.width, fb.height, fb.addr
            )
            .unwrap();
            Some(fb)
        }
        Err(e) => {
            writeln!(system_table.stdout(), "Graphics init failed: {:?}", e).unwrap();
            None
        }
    };

    // 3. 获取内存映射
    writeln!(system_table.stdout(), "Getting memory map...").unwrap();
    let memory_map = memory::get_memory_map(boot_services).unwrap();

    // 4. 获取 ACPI RSDP 地址
    let rsdp_addr = system_table
        .config_table()
        .iter()
        .find(|entry| {
            entry.guid == uefi::table::cfg::ACPI2_GUID
                || entry.guid == uefi::table::cfg::ACPI_GUID
        })
        .map(|entry| entry.address as u64);

    // 5. ExitBootServices
    writeln!(system_table.stdout(), "Exiting boot services...").unwrap();

    // ExitBootServices 会终止 UEFI 引导服务
    // 之后无法再使用 UEFI 的大部分服务（除了运行时服务）
    let (_runtime_system_table, _memory_map) = system_table
        .exit_boot_services(MemoryType::LOADER_DATA);

    // 6. 跳转到内核
    // 传递引导信息（这里简化处理，实际应该构建完整的 Multiboot2 结构）
    unsafe {
        jump_to_kernel(kernel_entry, 0x36d76289, 0);
    }
}

// ============================================================
// 内核加载
// ============================================================

/// 从 EFI 系统分区加载内核 ELF 文件
///
/// 读取路径：\EFI\AXIS\kernel.elf
fn load_kernel(image_handle: Handle, boot_services: &BootServices) -> Result<u64> {
    // 获取简单文件系统协议（访问 ESP 分区）
    let fs_handle = boot_services
        .get_handle_for_protocol::<SimpleFileSystem>()?;

    let mut fs = boot_services
        .open_protocol_exclusive::<SimpleFileSystem>(fs_handle)?;

    // 打开根目录
    let mut root = fs.open_volume()?;

    // 打开内核文件
    let kernel_path = cstr16!(r"\EFI\AXIS\kernel.elf");
    let kernel_file_handle = root.open(kernel_path, FileMode::Read, FileAttribute::READ_ONLY)?;

    let mut kernel_file = match kernel_file_handle.into_type()? {
        uefi::proto::media::file::FileType::Regular(file) => file,
        _ => return Err(Status::INVALID_PARAMETER.into()),
    };

    // 获取文件大小
    let mut info_buffer = [0u8; 512];
    let file_info = kernel_file.get_info::<uefi::proto::media::file::FileInfo>(&mut info_buffer)?;
    let kernel_size = file_info.file_size() as usize;

    // 分配内存缓冲区
    let kernel_buffer = boot_services.allocate_pool(MemoryType::LOADER_DATA, kernel_size)?;

    // 读取文件到内存
    let kernel_data = unsafe { slice::from_raw_parts_mut(kernel_buffer, kernel_size) };
    kernel_file.read(kernel_data)?;

    // 解析并加载 ELF
    let entry = unsafe { load_elf(kernel_data.as_ptr(), boot_services)? };

    Ok(entry)
}

/// 解析 ELF 文件并加载到内存
///
/// # Safety
/// 调用者必须确保 elf_data 指向有效的 ELF 文件数据
unsafe fn load_elf(elf_data: *const u8, boot_services: &BootServices) -> Result<u64> {
    // 读取 ELF 文件头
    let ehdr = &*(elf_data as *const Elf64Ehdr);

    // 验证 ELF 魔数
    let magic = u32::from_le_bytes([
        ehdr.e_ident[0],
        ehdr.e_ident[1],
        ehdr.e_ident[2],
        ehdr.e_ident[3],
    ]);
    if magic != ELF_MAGIC {
        return Err(Status::LOAD_ERROR.into());
    }

    // 验证 64 位小端序
    if ehdr.e_ident[4] != ELFCLASS64 || ehdr.e_ident[5] != ELFDATA2LSB {
        return Err(Status::LOAD_ERROR.into());
    }

    let entry = ehdr.e_entry;

    // 加载所有 PT_LOAD 段
    let phdr_base = elf_data.add(ehdr.e_phoff as usize) as *const Elf64Phdr;
    let phdr_count = ehdr.e_phnum as usize;

    for i in 0..phdr_count {
        let phdr = &*phdr_base.add(i);

        if phdr.p_type != PT_LOAD {
            continue;
        }

        // 分配内存（使用物理地址）
        // 注意：UEFI 环境下，虚拟地址通常等于物理地址
        let page_count = ((phdr.p_memsz + 0xfff) / 0x1000) as usize;
        let phys_addr = phdr.p_paddr;

        boot_services.allocate_pages(
            AllocateType::Address(phys_addr),
            MemoryType::LOADER_DATA,
            page_count,
        )?;

        // 复制段内容
        let src = elf_data.add(phdr.p_offset as usize);
        let dst = phdr.p_paddr as *mut u8;
        ptr::copy_nonoverlapping(src, dst, phdr.p_filesz as usize);

        // 清零 BSS
        if phdr.p_memsz > phdr.p_filesz {
            let bss_start = dst.add(phdr.p_filesz as usize);
            let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
            ptr::write_bytes(bss_start, 0, bss_size);
        }
    }

    Ok(entry)
}

/// 跳转到内核入口
///
/// # Safety
/// 调用者必须确保内核已正确加载，entry 是有效的入口地址
#[inline(never)]
unsafe fn jump_to_kernel(entry: u64, magic: u32, boot_info: u64) -> ! {
    core::arch::asm!(
        "mov eax, {magic:e}",
        "mov rbx, {boot_info}",
        "jmp {entry}",
        magic = in(reg) magic,
        boot_info = in(reg) boot_info,
        entry = in(reg) entry,
        options(noreturn)
    );
}
