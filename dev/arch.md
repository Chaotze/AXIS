# AXIS 架构设计文档

[项目结构](#项目结构) | [部分模块详解](#部分模块详解)

<br>



## 项目结构

AXIS/
├── bootloader/                                 # 引导加载程序 (固件和内核之间的桥梁)
│   ├── bios/                                   # BIOS 传统引导模式
│   │   ├── Cargo.toml
│   │   ├── Cargo.lock
│   │   ├── stage1.asm                          # 第一阶段引导 (MBR, 16 位实模式, 512 字节)
│   │   ├── stage2.asm                          # 第二阶段引导 (32 位保护模式启用)
│   │   ├── stage2.ld                           # Stage2 链接脚本
│   │   └── src/
│   │       └── lib.rs                          # Stage2 Rust 实现 (ELF 解析、内核加载)
│   │
│   ├── uefi/                                   # UEFI 现代引导模式 (UEFI 固件下运行)
│   │   ├── Cargo.toml
│   │   ├── Cargo.lock
│   │   ├── build.rs                            # 构建脚本
│   │   └── src/
│   │       ├── main.rs                         # UEFI 主入口
│   │       ├── graphics.rs                     # UEFI 图形初始化
│   │       └── memory.rs                       # UEFI 内存映射获取
│   │
│   └── common/                                 # 引导程序通用代码
│       ├── multiboot2.rs                       # Multiboot2 协议定义 (GRUB 兼容)
│       └── boot_info.rs                        # 引导信息结构体
│
├── kernel/                                     # 内核主程序
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── build.rs                                # 构建脚本 (交叉编译配置)
│   ├── kernel.ld                               # 内核链接脚本 (内存布局定义)
│   ├── x86_64-unknown-axis.json                # 自定义 target 定义 (x86-64 编译目标)
│   │
│   └── src/
│       ├── main.rs                             # 内核主函数入口
│       ├── lib.rs                              # 内核库根模块
│       ├── panic.rs                            # Panic 处理器 (系统崩溃处理)
│       ├── prelude.rs                          # 常用类型和宏预导入
│       │
│       ├── arch/                               # 架构特定代码 (CPU、中断、上下文切换等)
│       │   ├── mod.rs
│       │   └── x86_64/                         # x86_64 架构实现
│       │       ├── mod.rs
│       │       ├── boot.asm                    # 内核启动代码 (64 位验证、栈初始化)
│       │       ├── cpu.rs                      # CPU 特性检测和启用
│       │       ├── gdt.rs                      # GDT (全局描述符表) 设置
│       │       ├── idt.rs                      # IDT (中断描述符表) 设置
│       │       ├── memory.rs                   # 内存管理常量和工具函数
│       │       ├── paging.rs                   # 分页机制初始化
│       │       ├── vdso.rs                     # VDSO (虚拟动态共享对象) 快速路径实现
│       │       │
│       │       ├── interrupt/                  # 中断和异常处理
│       │       │   ├── mod.rs
│       │       │   ├── entry.asm               # 中断入口存根 (汇编)
│       │       │   ├── handler.rs              # 中断处理逻辑 (Rust)
│       │       │   ├── apic.rs                 # APIC 本地中断控制器
│       │       │   ├── ioapic.rs               # I/O APIC 中断控制器
│       │       │   ├── msi.rs                  # MSI/MSI-X 消息信号中断
│       │       │   └── timer.rs                # 定时器中断处理
│       │       │
│       │       └── context/                    # 进程上下文和状态保存
│       │           ├── mod.rs
│       │           ├── frame.rs                # 异常栈帧结构体
│       │           └── switch.asm              # 上下文切换汇编代码
│       │
│       ├── sync/                               # 同步原语和并发控制
│       │   ├── mod.rs
│       │   ├── spinlock.rs                     # 自旋锁 (最底层忙等待锁)
│       │   ├── mutex.rs                        # 互斥锁 (基于 futex 的内核锁)
│       │   ├── rwlock.rs                       # 读写锁 (允许多读单写)
│       │   ├── semaphore.rs                    # 信号量 (通用计数同步)
│       │   ├── condvar.rs                      # 条件变量 (配合互斥锁的等待机制)
│       │   ├── barrier.rs                      # 屏障 (线程同步点)
│       │   ├── event.rs                        # 事件对象 (事件触发同步)
│       │   ├── wait_queue.rs                   # 等待队列 (进程/线程阻塞队列)
│       │   └── atomic.rs                       # 原子操作工具函数
│       │
│       ├── mm/                                 # 内存管理 (物理、虚拟、堆)
│       │   ├── mod.rs
│       │   ├── pmm.rs                          # 物理内存管理 (伙伴系统分配器)
│       │   │   ├── buddy.rs                    # 伙伴系统算法
│       │   │   ├── zone.rs                     # 内存区域管理 (DMA、Normal、High)
│       │   │   ├── frame.rs                    # 页帧数据结构
│       │   │   ├── numa.rs                     # NUMA (非一致内存架构) 支持
│       │   │   └── watermark.rs                # 页面水位标记 (内存压力指示)
│       │   │
│       │   ├── vmm.rs                          # 虚拟内存管理 (页表、映射、COW)
│       │   │   ├── page_table.rs               # 四级页表操作
│       │   │   ├── mapping.rs                  # 虚拟地址到物理地址的映射
│       │   │   ├── layout.rs                   # 虚拟地址空间布局
│       │   │   ├── vma.rs                      # VMA (虚拟内存区域) 描述符
│       │   │   ├── cow.rs                      # COW (写时复制) 机制
│       │   │   └── swap.rs                     # 交换机制 (内存溢出到磁盘)
│       │   │
│       │   ├── heap.rs                         # 动态内存堆分配
│       │   │   ├── slub.rs                     # SLUB 分配器
│       │   │   ├── kmalloc.rs                  # 内核内存分配接口
│       │   │   └── slab_cache.rs               # Slab 缓存管理
│       │   │
│       │   └── addr.rs                         # 地址工具函数 (虚拟/物理转换)
│       │
│       ├── task/                               # 进程和线程管理 (调度、生命周期)
│       │   ├── mod.rs
│       │   ├── process.rs                      # 进程操作 (fork、exec、exit、wait)
│       │   ├── pcb.rs                          # PCB (进程控制块) 数据结构
│       │   ├── thread.rs                       # 线程数据结构和操作
│       │   ├── namespace.rs                    # 命名空间 (PID、Network、Mount、UTS、IPC、User)
│       │   │
│       │   ├── scheduler/                      # CPU 调度器实现
│       │   │   ├── mod.rs
│       │   │   ├── cfs.rs                      # CFS (完全公平调度器) 主体
│       │   │   ├── load_balance.rs             # 负载均衡 (多核任务分配)
│       │   │   ├── cpu_affinity.rs             # CPU 亲和性 (绑定任务到 CPU)
│       │   │   └── preemption.rs               # 抢占调度 (高优先级任务插队)
│       │   │
│       │   ├── signal.rs                       # 信号处理机制
│       │   ├── cgroup.rs                       # cgroup v2 资源控制
│       │   └── resource.rs                     # 资源限制 (rlimit)
│       │
│       ├── fs/
│       │   ├── mod.rs                          # VFS 抽象层入口
│       │   ├── vfs.rs                          # 虚拟文件系统核心接口 (Inode、File trait)
│       │   ├── file.rs                         # 文件对象和文件操作实现
│       │   ├── dentry.rs                       # 目录项 (dentry) 数据结构
│       │   │
│       │   ├── filesystems/                    # 各种具体的文件系统实现
│       │   │   ├── mod.rs
│       │   │   ├── tmpfs.rs                    # 临时内存文件系统 (tmpfs)
│       │   │   ├── devfs.rs                    # 设备文件系统 (devfs)
│       │   │   ├── procfs.rs                   # proc 文件系统 (/proc)
│       │   │   ├── sysfs.rs                    # sysfs 文件系统 (/sys)
│       │   │   └── exfat.rs                    # exFAT 文件系统驱动
│       │   │
│       │   ├── inode.rs                        # Inode 元数据和操作实现
│       │   ├── path.rs                         # 路径解析和遍历实现
│       │   ├── mount.rs                        # 挂载点管理和处理
│       │   ├── dcache.rs                       # 目录项缓存 (dentry cache)
│       │   └── pagecache.rs                    # 页缓存实现 (page cache)
│       │
│       ├── net/                                # 网络协议栈
│       │   ├── mod.rs
│       │   │
│       │   ├── link/                           # 链路层协议实现
│       │   │   ├── mod.rs
│       │   │   ├── ethernet.rs                 # 以太网协议处理
│       │   │   └── arp.rs                      # ARP (地址解析协议)
│       │   │
│       │   ├── ip/                             # IP 层协议实现
│       │   │   ├── mod.rs
│       │   │   ├── ipv4.rs                     # IPv4 协议处理和路由
│       │   │   ├── ipv6.rs                     # IPv6 协议处理和自动配置
│       │   │   ├── routing.rs                  # 路由表管理和查询
│       │   │   ├── icmp.rs                     # ICMP 协议 (ping)
│       │   │   ├── icmpv6.rs                   # ICMPv6 协议 (邻居发现)
│       │   │   └── fragment.rs                 # IP 分片和重组
│       │   │
│       │   ├── transport/                      # 传输层协议实现
│       │   │   ├── mod.rs
│       │   │   ├── tcp.rs                      # TCP 协议栈实现
│       │   │   ├── udp.rs                      # UDP 协议实现
│       │   │   └── socket.rs                   # Socket 抽象接口
│       │   │
│       │   ├── io_uring.rs                     # io_uring 高性能异步接口
│       │   └── config.rs                       # 网络栈配置和参数
│       │
│       ├── drivers/                           # 硬件设备驱动层
│       │   ├── mod.rs
│       │   │
│       │   ├── serial/                         # 串口驱动
│       │   │   ├── mod.rs
│       │   │   └── uart16550.rs                # UART 16550 兼容串口控制器
│       │   │
│       │   ├── display/                        # 显示驱动
│       │   │   ├── mod.rs
│       │   │   ├── fb.rs                       # 帧缓冲 (framebuffer)
│       │   │   ├── gop.rs                      # UEFI GOP 图形驱动
│       │   │   └── vesafb.rs                   # VESA 帧缓冲驱动
│       │   │
│       │   ├── input/                          # 输入设备驱动
│       │   │   ├── mod.rs
│       │   │   ├── hid.rs                      # HID (人机交互设备) 通用驱动
│       │   │   ├── keyboard.rs                 # 键盘驱动
│       │   │   └── mouse.rs                    # 鼠标驱动
│       │   │
│       │   ├── block/                          # 块存储设备驱动
│       │   │   ├── mod.rs
│       │   │   ├── nvme.rs                     # NVMe (高速 SSD) 驱动
│       │   │   ├── ahci.rs                     # AHCI (SATA 控制器) 驱动
│       │   │   ├── virtio.rs                   # VirtIO 虚拟块设备驱动
│       │   │   ├── io_scheduler.rs             # I/O 调度器 (CFQ、Noop 等)
│       │   │   └── blk_queue.rs                # 块设备请求队列管理
│       │   │
│       │   ├── nic/                            # 网络接口卡 (NIC) 驱动 (仅硬件驱动层)
│       │   │   ├── mod.rs
│       │   │   ├── e1000.rs                    # Intel e1000 网卡驱动
│       │   │   ├── igc.rs                      # Intel i225/i226 网卡驱动
│       │   │   ├── virtio.rs                   # VirtIO 虚拟网卡驱动
│       │   │   └── driver.rs                   # 通用网卡驱动框架
│       │   │
│       │   ├── pci/                            # PCI 总线枚举和配置
│       │   │   ├── mod.rs
│       │   │   ├── config.rs                   # PCI 配置空间读写
│       │   │   ├── device.rs                   # PCI 设备对象和操作
│       │   │   ├── ecam.rs                     # ECAM (增强型配置地址映射)
│       │   │   ├── dma.rs                      # DMA (直接内存访问) 管理
│       │   │   └── iommu.rs                    # IOMMU (I/O 内存管理单元)
│       │   │
│       │   └── acpi/                           # ACPI (高级配置和电源接口)
│       │       ├── mod.rs
│       │       ├── parse.rs                    # ACPI 表解析器
│       │       └── tables.rs                   # ACPI 数据表定义
│       │
│       ├── syscall/                            # 系统调用接口 (用户态进入内核)
│       │   ├── mod.rs
│       │   ├── dispatch.rs                     # 系统调用分发器 (路由到具体处理函数)
│       │   ├── fs.rs                           # 文件系统相关系统调用 (open、read、write、stat)
│       │   ├── process.rs                      # 进程相关系统调用 (fork、execve、exit)
│       │   ├── memory.rs                       # 内存相关系统调用 (mmap、brk、mprotect)
│       │   ├── signal.rs                       # 信号相关系统调用 (kill、signal、sigaction)
│       │   ├── ipc.rs                          # IPC 系统调用 (msgget、shmget、semget)
│       │   ├── net.rs                          # 网络相关系统调用 (socket、connect、send)
│       │   ├── time.rs                         # 时间相关系统调用 (gettimeofday、nanosleep)
│       │   ├── io_uring.rs                     # io_uring 异步 I/O 系统调用
│       │   ├── cgroup.rs                       # cgroup 相关系统调用
│       │   ├── namespace.rs                    # 命名空间相关系统调用 (unshare、setns)
│       │   ├── perf.rs                         # 性能计数相关系统调用
│       │   └── misc.rs                         # 其他杂项系统调用
│       │
│       ├── compat/                             # Linux 兼容层 (运行 Linux 二进制程序)
│       │   ├── mod.rs
│       │   ├── loader.rs                       # ELF 加载器 (支持动态链接)
│       │   ├── interpreter.rs                  # 动态链接器/解释器 (ld-linux.so)
│       │   ├── syscall_translator.rs           # 系统调用号转换 (Linux ABI -> AXIS ABI)
│       │   ├── auxv.rs                         # 辅助向量构建 (AT_* 常量)
│       │   ├── signal_adapter.rs               # 信号处理适配 (Linux 信号语义)
│       │   ├── procfs_emulation.rs             # /proc 伪文件系统模拟 (不一定必要)
│       │   └── personality.rs                  # 进程个性设置 (PER_LINUX)
│       │
│       ├── lib/                                # 通用库函数和工具
│       │   ├── mod.rs
│       │   ├── print.rs                        # 打印和日志函数
│       │   │
│       │   ├── collections/                    # 数据结构库
│       │   │   ├── mod.rs
│       │   │   ├── ring_buffer.rs              # 环形缓冲区 (定长循环缓冲)
│       │   │   ├── btree.rs                    # B 树 (有序数据结构)
│       │   │   ├── radix_tree.rs               # Radix 树 (高效的前缀树)
│       │   │   ├── bitmap.rs                   # 位图 (高效的布尔数组)
│       │   │   ├── lru.rs                      # LRU 缓存 (最近最少使用替换)
│       │   │   └── lockfree/                   # 无锁并发数据结构 (高性能)
│       │   │       ├── mod.rs
│       │   │       ├── stack.rs                # 无锁栈 (LIFO 队列)
│       │   │       ├── queue.rs                # 无锁队列 (FIFO 队列)
│       │   │       └── hashmap.rs              # 无锁哈希表 (CAS 原子操作实现)
│       │   │
│       │   ├── string.rs                       # 字符串处理工具
│       │   ├── hash.rs                         # 哈希函数库
│       │   ├── crc.rs                          # CRC 校验码计算
│       │   ├── bit.rs                          # 位操作工具函数
│       │   ├── result.rs                       # 错误处理 Result/Option
│       │   ├── time.rs                         # 时间计算和格式化
│       │   └── debug.rs                        # 调试输出工具
│       │
│       └── config.rs                           # 内核编译时配置常量
│
├── Makefile                                    # 构建系统主入口 (编译、运行、清理)
├── build.sh                                    # Shell 构建脚本
├── run_qemu.sh                                 # QEMU 虚拟机启动脚本 (BIOS 模式)
├── run_grub.sh                                 # GRUB 引导启动脚本
├── run_uefi.sh                                 # UEFI 模式启动脚本
├── .cargo/config.toml                          # Cargo 配置 (编译器选项、依赖定义)
├── .gitignore                                  # Git 忽略文件列表
├── Cargo.toml                                  # 项目清单文件 (依赖、元数据)
├── Cargo.lock                                  # 依赖版本锁定文件
├── dev/                                        # 开发者文件夹
│   ├── dev.md                                  # 开发者文档
│   └── arch.md                                 # 架构设计文档
├── README.md                                   # 项目概览 (英文)
└── README.zh-CN.md                             # 项目概览 (中文)

<br>



## 部分模块详解

### 1. Bootloader

#### bootloader/bios/stage1.asm

用途: BIOS 传统引导的第一阶段

实现思路:
- 16位实模式汇编代码
- 位于 MBR（主引导记录，512B）
- 任务：
  1. 初始化寄存器和堆栈
  2. 从磁盘加载 stage2 到内存
  3. 进入保护模式（32位）
  4. 跳转到 stage2

代码框架:
```asm
; MBR 引导扇区
org 0x7c00
bits 16

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7c00

    ; 加载 stage2
    mov ah, 0x02        ; 读磁盘
    mov al, 4           ; 读 4 个扇区（stage2 大小）
    mov ch, 0           ; 柱面
    mov cl, 2           ; 扇区
    mov dh, 0           ; 磁头
    mov dl, 0x80        ; 驱动器（0x80=第一块硬盘）
    mov bx, 0x7e00      ; 目标地址
    int 0x13

    ; 进入保护模式
    lgdt [gdt_descriptor]
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    jmp 0x08:0x7e00     ; 跳转到 stage2
```

---
#### bootloader/bios/stage2.asm + bootloader/bios/src/lib.rs

用途: 32位保护模式加载器

实现思路:
- 汇编负责：CPU 进入长模式（64位）、页表设置
- Rust 负责：ELF 解析、内核加载、引导信息构建
- 切换到长模式前夕设置临时页表
- 加载内核 ELF 镜像到内存
- 传递多引导2格式的引导信息给内核

关键代码段:
// stage2/src/lib.rs
pub unsafe extern "C" fn stage2_main(boot_info_ptr: usize) -> ! {
    let boot_info = BootInfo::from_ptr(boot_info_ptr);

    // 1. 验证 CPU 支持 long mode
    check_cpu_features();

    // 2. 建立临时页表
    let page_table = setup_page_tables();

    // 3. 加载内核 ELF
    let elf = parse_elf(boot_info.kernel_addr);
    load_elf_segments(&elf);

    // 4. 切换到长模式（汇编）
    enable_long_mode(page_table);

    // 5. 跳转到内核入口
    jump_to_kernel(elf.entry_point, boot_info_ptr);
}

---
#### bootloader/uefi/src/main.rs

用途: UEFI 固件下的引导应用程序

实现思路:
- 使用 uefi crate 提供的 UEFI 服务
- 初始化 UEFI 运行环境（内存、控制台）
- 从磁盘或网络加载内核
- 使用 ExitBootServices 进入内核
- 构造符合预期的引导信息结构

实现方法:
#[entry]
fn efi_main(handle: Handle, system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&handle, system_table).unwrap();

    // 1. 打印启动消息
    println!("AXIS UEFI Bootloader v1.0");

    // 2. 获取内存映射
    let boot_services = system_table.boot_services();
    let memory_map = get_memory_map(boot_services);

    // 3. 加载内核
    let kernel_data = load_kernel_file(boot_services);
    let kernel_entry = parse_and_load_kernel(kernel_data);

    // 4. 准备引导参数
    let boot_info = BootInfo {
        memory_map,
        rsdp_addr: find_rsdp(),
        kernel_entry,
        // ...
    };

    // 5. 退出 UEFI Boot Services
    let (_runtime_services, boot_mmap) = system_table.exit_boot_services(handle);

    // 6. 跳转到内核
    jump_to_kernel(kernel_entry, &boot_info);
}

---
#### bootloader/common/multiboot2.rs

用途: Multiboot2 格式定义和常量

实现思路:
- 定义 Multiboot2 头结构（GRUB 兼容）
- 定义引导信息结构体
- 提供解析函数

代码:
pub const MULTIBOOT2_MAGIC: u32 = 0xe85250d6;
pub const MULTIBOOT2_ARCH_I386: u32 = 0;
pub const MULTIBOOT2_MAGIC_EAX: u32 = 0x36d76289;

#[repr(C, packed)]
pub struct Multiboot2Header {
    pub magic: u32,
    pub architecture: u32,
    pub header_length: u32,
    pub checksum: u32,
    // 标签...
}

#[repr(C)]
pub struct BootInfoResponse {
    pub total_size: u32,
    pub reserved: u32,
    pub tags: [Tag; 0],
}

pub enum Tag {
    End,
    BootDevice,
    BootLoaderName,
    Module,
    BasicMeminfo,
    Bootdev,
    Mmap,
    // ...
}

---

### 2. Architecture (arch/x86_64/)

#### arch/x86_64/boot.asm

用途: 内核启动的最初代码，从 bootloader 跳入

实现思路:
- 验证 long mode（64位）已启用
- 验证分页已启用
- 初始化堆栈
- 跳转到 Rust main 函数

代码:
```asm
; boot.asm
extern kernel_main

KERNEL_BASE equ 0xFFFF800000000000

section .text
bits 64

global _start
_start:
    ; 验证我们在 64 位模式
    mov rax, cr4
    test rax, 1 << 5    ; PAE (Physical Address Extension)
    jz .error

    mov rax, cr0
    test rax, 1 << 31   ; Paging
    jz .error

    ; 设置栈
    mov rsp, KERNEL_STACK_TOP

    ; 清除堆栈帧指针
    xor rbp, rbp

    ; 跳转到 Rust main
    call kernel_main

    cli
    hlt
    jmp $
```

---
#### arch/x86_64/cpu.rs

用途: CPU 功能检查和初始化

实现思路:
- 使用 CPUID 指令检查 CPU 特性
- 验证必需特性（long mode, PAE 等）
- 启用可选特性（SSE, AVX, TSX 等）
- 检测 CPU 数量和拓扑

实现方法:
pub struct CpuFeatures {
    pub has_long_mode: bool,
    pub has_pae: bool,
    pub has_pse: bool,
    pub has_msr: bool,
    pub has_apic: bool,
    pub has_x2apic: bool,
    pub has_sse: bool,
    pub has_avx: bool,
    pub has_rdrand: bool,
    pub num_cores: u32,
}

pub fn detect_features() -> CpuFeatures {
    unsafe {
        let (eax, ebx, ecx, edx) = cpuid(0x80000001);
        CpuFeatures {
            has_long_mode: (edx & (1 << 29)) != 0,
            has_pae: (edx & (1 << 6)) != 0,
            // ...
        }
    }
}

pub fn enable_features() {
    // 启用 SMEP/SMAP（安全特性）
    unsafe {
        let mut cr4 = read_cr4();
        cr4 |= 1 << 20;  // SMEP
        cr4 |= 1 << 21;  // SMAP
        write_cr4(cr4);
    }
}

---
#### arch/x86_64/gdt.rs

用途: 全局描述符表（GDT）设置

实现思路:
- 定义 GDT 表，包含代码段、数据段、TSS（任务状态段）
- 在内核初始化时加载 GDT
- 为每个 CPU 维护独立的 GDT（多核支持）

代码:
#[repr(C, packed)]
pub struct GdtEntry {
    pub limit_low: u16,
    pub base_low: u16,
    pub base_mid: u8,
    pub access: u8,
    pub limit_high: u8,
    pub base_high: u8,
}

pub const GDT_NULL: usize = 0;
pub const GDT_KERNEL_CODE: usize = 1;
pub const GDT_KERNEL_DATA: usize = 2;
pub const GDT_USER_DATA: usize = 3;
pub const GDT_USER_CODE: usize = 4;
pub const GDT_TSS: usize = 5;

pub struct Gdt {
    table: [GdtEntry; 8],
    tss: TaskStateSegment,
}

impl Gdt {
    pub fn new() -> Self {
        let mut gdt = Gdt {
            table: [GdtEntry::null(); 8],
            tss: TaskStateSegment::new(),
        };
        gdt.table[GDT_KERNEL_CODE] = GdtEntry::kernel_code();
        gdt.table[GDT_KERNEL_DATA] = GdtEntry::kernel_data();
        gdt.table[GDT_TSS] = GdtEntry::tss(&gdt.tss);
        gdt
    }

    pub fn load(&self) {
        unsafe {
            asm!("lgdt [{}]", in(reg) &self.table);
        }
    }
}

---
#### arch/x86_64/idt.rs

用途: 中断描述符表（IDT）设置

实现思路:
- 定义 256 个中断门（Interrupt Gate）
- 注册异常处理器（CPU 异常 #0-#31）
- 注册硬件中断处理器（#32-#255）
- 注册系统调用门（#48 或 syscall 指令）

实现方法:
#[repr(C, packed)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub segment_selector: u16,
    pub stack_table_index: u8,
    pub attributes: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    pub reserved: u32,
}

pub struct Idt {
    table: [IdtEntry; 256],
}

impl Idt {
    pub fn new() -> Self {
        Idt { table: [IdtEntry::null(); 256] }
    }

    pub fn set_handler(&mut self, index: usize, handler: extern "x86-interrupt" fn()) {
        let addr = handler as u64;
        self.table[index] = IdtEntry {
            offset_low: (addr & 0xFFFF) as u16,
            segment_selector: GDT_KERNEL_CODE as u16 * 8,
            stack_table_index: 0,
            attributes: 0x8E,  // 中断门，DPL=0，Present
            offset_mid: ((addr >> 16) & 0xFFFF) as u16,
            offset_high: (addr >> 32) as u32,
            reserved: 0,
        };
    }

    pub fn set_user_handler(&mut self, index: usize, handler: extern "x86-interrupt" fn()) {
        // 类似，但 DPL=3（用户态可触发）
    }

    pub fn load(&self) {
        unsafe {
            asm!("lidt [{}]", in(reg) &self.table);
        }
    }
}

---
#### arch/x86_64/interrupt/entry.asm

用途: 中断处理程序的汇编入口点

实现思路:
- 定义 256 个中断入口存根
- 每个存根保存现场、调用相应的 Rust 处理器、恢复现场
- 处理错误码和异常号的传递

代码框架:
```asm
; interrupt/entry.asm
extern interrupt_handler

%macro INTERRUPT_NO_ERROR 1
align 4
interrupt_%1:
    push 0              ; 伪造错误码
    push %1             ; 异常号
    jmp common_interrupt
%endmacro

%macro INTERRUPT_ERROR 1
align 4
interrupt_%1:
    ; 错误码已由 CPU 压入栈
    push %1             ; 异常号
    jmp common_interrupt
%endmacro

; 异常处理器
INTERRUPT_NO_ERROR 0    ; #DE - Divide Error
INTERRUPT_NO_ERROR 1    ; #DB - Debug Exception
; ...
INTERRUPT_ERROR 8       ; #DF - Double Fault（有错误码）
; ...

common_interrupt:
    ; 保存通用寄存器
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11

    ; 调用 Rust 处理器
    mov rdi, rsp        ; 指向异常帧
    call interrupt_handler

    ; 恢复寄存器
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax

    add rsp, 16         ; 跳过错误码和异常号
    iretq
```

---
#### arch/x86_64/interrupt/handler.rs

用途: Rust 侧的中断处理器实现

实现思路:
- 定义异常帧结构体
- 为各类异常提供处理函数
- 分发硬件中断到驱动程序
- 处理系统调用

代码：
#[repr(C)]
pub struct ExceptionFrame {
    pub error_code: u64,
    pub exception_num: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub extern "C" fn interrupt_handler(frame: &mut ExceptionFrame) {
    match frame.exception_num {
        0 => divide_error(frame),
        1 => debug_exception(frame),
        13 => general_protection_fault(frame),
        14 => page_fault(frame),
        32..=255 => {
            let irq = frame.exception_num - 32;
            handle_hardware_interrupt(irq as u8, frame);
        }
        _ => unhandled_exception(frame),
    }
}

fn page_fault(frame: &mut ExceptionFrame) {
    let fault_addr = unsafe { read_cr2() };
    let error_code = frame.error_code;

    // 判断是内核态还是用户态缺页
    if (error_code & 0x4) == 0 {
        // 内核态缺页
        panic!("Kernel page fault at {:#x}", fault_addr);
    } else {
        // 用户态缺页
        if !handle_user_page_fault(fault_addr, error_code as u32) {
            // 无效的访问
            send_signal_to_current_task(SIGSEGV);
        }
    }
}

fn general_protection_fault(frame: &mut ExceptionFrame) {
    if is_user_mode(frame) {
        send_signal_to_current_task(SIGSEGV);
    } else {
        panic!("Kernel GPF at {:#x}", frame.rip);
    }
}

---
#### arch/x86_64/interrupt/apic.rs

用途: 高级可编程中断控制器（APIC）初始化和管理

实现思路:
- 检测 Local APIC 的支持和位置（通过 CPUID 和 MSR）
- 初始化 Local APIC（使能、设置 LVT 表项）
- 支持 x2APIC 模式（改进的 APIC 模式，效率更高）
- 提供 IPI（处理器间中断）机制

实现方法:
pub const IA32_APIC_BASE_MSR: u32 = 0x1B;
pub const APIC_BASE_ENABLE: u64 = 1 << 11;

pub struct LocalApic {
    base_addr: *mut u32,
}

impl LocalApic {
    pub fn new() -> Self {
        // 读取 APIC 基地址
        let msr = unsafe { rdmsr(IA32_APIC_BASE_MSR) };
        let base_addr = ((msr & 0xFFFFF000) as usize) as *mut u32;
        LocalApic { base_addr }
    }

    pub fn enable(&mut self) {
        // 启用 APIC 和 SVR（中断向量寄存器）
        unsafe {
            let mut svr = self.read(0xF0);  // SVR 偏移
            svr |= 0x100;  // 启用位
            svr = (svr & 0xFFFFFF00) | 0xFF;  // 设置向量为 0xFF
            self.write(0xF0, svr);
        }
    }

    pub fn send_ipi(&mut self, cpu_id: u8, vector: u8) {
        unsafe {
            // 设置 ICR（中断命令寄存器）
            let icr_low = (cpu_id as u32) << 24 | vector as u32;
            self.write(0x300, icr_low);
        }
    }

    unsafe fn read(&self, offset: u32) -> u32 {
        *self.base_addr.offset((offset >> 2) as isize)
    }

    unsafe fn write(&mut self, offset: u32, value: u32) {
        *self.base_addr.offset((offset >> 2) as isize) = value;
    }
}

---
#### arch/x86_64/interrupt/ioapic.rs

用途: I/O 高级可编程中断控制器（I/O APIC）配置

实现思路:
- 通过 ACPI MADT 表发现 I/O APIC
- 配置 I/O APIC 重定向表项，将外设中断路由到 CPU
- 支持 MSI（Message Signaled Interrupts）

代码:
pub struct IoApic {
    base_addr: *mut u32,
}

impl IoApic {
    pub fn new(base_addr: usize) -> Self {
        IoApic { base_addr: base_addr as *mut u32 }
    }

    pub fn configure_irq(&mut self, irq: u8, vector: u8, cpu: u8) {
        // IRQ 通常映射为 vector + 32
        let redirection_entry = ((cpu as u64) << 56) | (vector as u64);
        self.write_redirection(irq as u32, redirection_entry);
    }

    fn write_redirection(&mut self, index: u32, value: u64) {
        unsafe {
            let ioregsel = self.base_addr;
            let iowin = self.base_addr.offset(4);

            *ioregsel = 0x10 + (index * 2);
            *iowin = (value & 0xFFFFFFFF) as u32;

            *ioregsel = 0x11 + (index * 2);
            *iowin = ((value >> 32) & 0xFFFFFFFF) as u32;
        }
    }
}

---
#### arch/x86_64/interrupt/msi.rs

用途: MSI/MSI-X（消息信号中断）配置

实现思路:
- 在 PCI 设备中查找 MSI 能力结构
- 为设备分配 MSI 中断向量
- 配置设备 MSI 地址和数据寄存器

实现方法:
pub struct MsiCapability {
    pub offset: u16,
    pub is_64bit: bool,
    pub supports_masking: bool,
}

pub fn configure_device_msi(
    pci_device: &PciDevice,
    vector: u8,
    cpu: u8,
) -> Option<()> {
    let cap = find_msi_capability(pci_device)?;

    // MSI 中断地址和数据
    let msi_addr = 0xFEE0_0000u32 | ((cpu as u32) << 12);
    let msi_data = vector as u32;

    // 写入 PCI 配置空间
    pci_device.write_config_u32(cap.offset as u16, msi_addr);
    if cap.is_64bit {
        pci_device.write_config_u32(cap.offset as u16 + 4, 0);
        pci_device.write_config_u32(cap.offset as u16 + 8, msi_data);
    } else {
        pci_device.write_config_u32(cap.offset as u16 + 4, msi_data);
    }

    // 启用 MSI
    let control = pci_device.read_config_u16(cap.offset as u16);
    pci_device.write_config_u16(cap.offset as u16, control | 0x0001);

    Some(())
}

---
#### arch/x86_64/context/switch.asm

用途: 进程上下文切换的汇编代码

实现思路:
- 保存当前进程的寄存器状态到内核栈
- 切换到新进程的内核栈和页表
- 恢复新进程的寄存器并返回

代码:
```asm
; context/switch.asm
; void context_switch(old_rsp: *mut u64, new_rsp: u64)
global context_switch
context_switch:
    ; rdi = 指向旧 rsp 指针的地址
    ; rsi = 新进程的 rsp

    ; 保存旧进程的寄存器
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    ; 保存当前 rsp 到旧进程的 PCB
    mov [rdi], rsp

    ; 切换到新进程的栈
    mov rsp, rsi

    ; 如果需要切换页表（可选，多进程时）
    ; 从栈上读取新的 CR3（页表基址）并加载
    mov rax, [rsp + 48]
    mov cr3, rax

    ; 恢复新进程的寄存器
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    ret
```

---

#### arch/x86_64/memory.rs

用途: x86-64 平台内存管理常量和工具函数

实现思路:
- 定义 x86-64 特定的内存布局常量
- 提供地址转换和验证工具
- 由 `mm/vmm.rs` 通用层导入使用

代码:
```rust
// 页大小和相关常量
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
pub const PAGE_MASK: u64 = 0xFFF;
pub const PAGE_ADDR_MASK: u64 = !0xFFF;

// 大页和巨页
pub const LARGE_PAGE_SIZE: usize = 0x200_000;     // 2MB
pub const LARGE_PAGE_SHIFT: usize = 21;
pub const HUGE_PAGE_SIZE: usize = 0x4000_0000;    // 1GB
pub const HUGE_PAGE_SHIFT: usize = 30;

// 虚拟地址空间布局 (x86-64 canonical form)
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_END: u64 = 0xFFFF_FFFF_FFFF_FFFF;
pub const USER_BASE: u64 = 0x0000_0000_0000_0000;
pub const USER_END: u64 = 0x0000_7FFF_FFFF_FFFF;

// 内核空间预留区域
pub const KERNEL_HEAP_BASE: u64 = 0xFFFF_FF00_0000_0000;
pub const KERNEL_HEAP_END: u64 = 0xFFFF_FF80_0000_0000;
pub const KERNEL_STACK_BASE: u64 = 0xFFFF_FE00_0000_0000;

// 用户空间区域
pub const USER_HEAP_MAX: u64 = 0x0000_5555_5555_5000;   // brk() 上限
pub const USER_STACK_BASE: u64 = 0x0000_7FFF_FFFF_F000; // 向下增长

pub fn is_user_address(addr: u64) -> bool {
    USER_BASE <= addr && addr <= USER_END
}

pub fn is_kernel_address(addr: u64) -> bool {
    addr >= KERNEL_BASE
}

pub fn is_canonical(addr: u64) -> bool {
    is_user_address(addr) || is_kernel_address(addr)
}
```

---

#### arch/x86_64/interrupt/handler.rs

用途: x86-64 系统调用异常处理（与通用层 syscall/dispatch.rs 分离）

实现思路:
- 处理 syscall 指令触发的异常
- 将 x86-64 寄存器映射到通用参数
- 调用通用的系统调用分发器
- 处理异常栈帧和返回

代码补充:
```rust
// 在 arch/x86_64/interrupt/handler.rs 中添加
pub extern "x86-interrupt" fn syscall_handler(frame: &mut ExceptionFrame) {
    // x86-64 syscall 指令约定：
    // 系统调用号在 rax，参数在 rdi, rsi, rdx, r10, r8, r9
    let number = frame.rax as usize;
    let arg0 = frame.rdi;
    let arg1 = frame.rsi;
    let arg2 = frame.rdx;
    let arg3 = frame.r10;
    let arg4 = frame.r8;
    let arg5 = frame.r9;

    // 调用通用分发器（位于 syscall/dispatch.rs）
    let result = syscall_dispatch(number, arg0, arg1, arg2, arg3, arg4, arg5);
    
    // 返回值放入 rax
    frame.rax = result as u64;
}
```

---

#### arch/x86_64/vdso.rs

用途: VDSO（Virtual Dynamic Shared Object）快速路径实现

实现思路:
- 内核在用户空间映射只读 VDSO 页面
- 提供高频系统调用的无上下文切换实现
- 在 ELF 加载时通过 auxv 通知 libc VDSO 地址
- 使用 RDTSC、HPET 或内核维护的时间戳

设计要点:
- **VDSO 映射**：在进程虚拟地址空间中固定位置（如 0xffffffffff600000）
- **时间戳更新**：内核定时器中断更新 VDSO 的时钟信息
- **后备系统调用**：若 VDSO 数据过期或不可用，调用真实系统调用
- **Linux 兼容**：采用与 Linux 相同的 VDSO 接口和符号

实现框架:
```rust
// VDSO 符号定义
#[repr(C)]
pub struct VdsoData {
    pub seq: u64,                    // 序列号，用于无锁同步
    pub tv_sec: i64,                 // 秒
    pub tv_nsec: u64,                // 纳秒
    pub clock_tsc_cycles: u64,       // TSC 周期
    pub clock_tsc_khz: u32,          // TSC 频率
    pub cpu_id: u32,                 // CPU ID
}

// VDSO 内部汇编函数（会被 libc 调用）
global __vdso_gettimeofday
__vdso_gettimeofday:
    ; rdi = struct timeval *tv
    ; rsi = struct timezone *tz（已废弃，忽略）
    ; 从 VDSO 数据区读取时间
    mov rax, qword [vdso_data]    ; 获取 VdsoData 地址
    ; ... 转换并填充 timeval 结构
    ret

global __vdso_clock_gettime
__vdso_clock_gettime:
    ; rdi = clockid_t which_clock
    ; rsi = struct timespec *ts
    ; 根据 clock id 返回对应的时间
    cmp rdi, CLOCK_REALTIME
    je .realtime
    cmp rdi, CLOCK_MONOTONIC
    je .monotonic
    ; ... 其他 clock id
.realtime:
    mov rax, qword [vdso_data]
    ; ... 填充 timespec 结构
    ret

global __vdso_getcpu
__vdso_getcpu:
    ; rdi = unsigned *cpu
    ; rsi = unsigned *node（可选）
    ; rdx = void *tcache（cache 指针，可选）
    mov rax, qword [vdso_data + CPU_ID_OFFSET]
    mov [rdi], eax
    ret
```

**VDSO 初始化** 在进程加载时：
```rust
pub fn setup_vdso(process: &mut ProcessControlBlock) {
    // 1. 分配物理页（驻留内存）
    let vdso_paddr = PMM.alloc(0)?;
    
    // 2. 在用户地址空间映射（只读）
    let vdso_vaddr = 0xffffffffff600000u64;
    process.page_table.map(vdso_vaddr, vdso_paddr, PAGE_RO | PAGE_USER)?;
    
    // 3. 初始化 VDSO 数据
    let vdso_data = VdsoData {
        seq: 0,
        tv_sec: current_timestamp() / 1_000_000_000,
        tv_nsec: current_timestamp() % 1_000_000_000,
        // ...
    };
    unsafe {
        *(vdso_paddr as *mut VdsoData) = vdso_data;
    }
    
    // 4. 在 auxv 中记录 VDSO 地址
    process.auxv.push((AT_SYSINFO_EHDR, vdso_vaddr));
}
```

**VDSO 更新** 在定时器中断中：
```rust
pub fn update_vdso_timestamp() {
    // 在每个时钟滴答时调用
    for process in TASK_MANAGER.all_processes() {
        if let Ok(vdso_ptr) = process.get_vdso_data() {
            unsafe {
                let data = &mut *vdso_ptr;
                data.seq += 1;  // 递增序列号（开始更新）
                data.tv_sec = current_timestamp() / 1_000_000_000;
                data.tv_nsec = current_timestamp() % 1_000_000_000;
                data.seq += 1;  // 递增序列号（完成更新）
            }
        }
    }
}
```

---

### 3. Synchronization (sync/)

#### sync/spinlock.rs

用途: 自旋锁实现（最底层的同步原语）

实现思路:
- 基于原子操作的忙等待锁
- 支持 exponential backoff（减少 CPU 自旋浪费）
- 用于代码路径很短的临界区（中断处理、初始化）

代码:
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

pub struct Spinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

impl<T> Spinlock<T> {
    pub fn new(data: T) -> Self {
        Spinlock {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<T> {
        let mut backoff = 1u32;
        loop {
            // 尝试获取锁
            if self.locked.compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                return SpinlockGuard { lock: self };
            }

            // 指数退避
            for _ in 0..backoff {
                unsafe { asm!("pause") }  // 降低功耗
            }
            backoff = (backoff * 2).min(1024);
        }
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
}

impl<'a, T> Deref for SpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

---
#### sync/mutex.rs

用途: 互斥锁（基于 futex）

实现思路:
- 使用 futex（fast userspace mutex）系统调用
- 内核侧：fast path 使用原子操作，slow path 使用内核等待队列
- 支持公平性和优先级继承

实现方法:
pub struct Mutex<T> {
    state: AtomicU32,  // bit 0: locked, bit 1..31: waiter count
    data: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    pub fn new(data: T) -> Self {
        Mutex {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> MutexGuard<T> {
        loop {
            // Fast path: 尝试获取未锁定的互斥锁
            if self.state.compare_exchange(
                0,
                1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                return MutexGuard { mutex: self };
            }

            // Slow path: 调用 futex_wait
            let state = self.state.load(Ordering::Relaxed);
            if (state & 1) != 0 {
                // 已锁定，等待
                futex_wait(&self.state, state | 0x2);
            }
        }
    }
}

---
#### sync/wait_queue.rs

用途: 内核等待队列（进程/线程在条件上阻塞的机制）

实现思路:
- 维护等待某个事件的任务队列
- 支持 LRU 唤醒策略（最近未获得 CPU 的进程优先唤醒）
- 支持超时等待

代码:
pub struct WaitQueue {
    queue: Spinlock<VecDeque<WaitEntry>>,
}

struct WaitEntry {
    task: *mut Task,
    timeout: Option<u64>,
    wake_token: u64,
}

impl WaitQueue {
    pub fn new() -> Self {
        WaitQueue {
            queue: Spinlock::new(VecDeque::new()),
        }
    }

    pub fn wait(&self, task: &mut Task, timeout: Option<u64>) {
        let mut q = self.queue.lock();
        q.push_back(WaitEntry {
            task: task as *mut _,
            timeout,
            wake_token: task.wake_token,
        });
        drop(q);

        // 设置任务状态为 BLOCKED
        task.state = TaskState::Blocked;
        // 让出 CPU（在调度器中实现）
        schedule();
    }

    pub fn wake_one(&self) {
        let mut q = self.queue.lock();
        if let Some(entry) = q.pop_front() {
            unsafe {
                (*entry.task).state = TaskState::Ready;
                SCHEDULER.add_to_ready_queue(*entry.task);
            }
        }
    }

    pub fn wake_all(&self) {
        let mut q = self.queue.lock();
        while let Some(entry) = q.pop_front() {
            unsafe {
                (*entry.task).state = TaskState::Ready;
                SCHEDULER.add_to_ready_queue(*entry.task);
            }
        }
    }
}

---
#### sync/condvar.rs

用途: 条件变量（配合互斥锁使用的同步原语）

实现思路:
- 实现标准的 condition variable 语义
- 支持 wait（原子地释放互斥锁并等待）
- 支持 notify_one 和 notify_all

代码:
pub struct CondVar {
    wait_queue: WaitQueue,
}

impl CondVar {
    pub fn new() -> Self {
        CondVar {
            wait_queue: WaitQueue::new(),
        }
    }

    pub fn wait<T>(&self, guard: MutexGuard<T>) -> MutexGuard<T> {
        // 原子地释放互斥锁并等待信号
        drop(guard);
        self.wait_queue.wait(current_task(), None);
        // 重新获取互斥锁
        Mutex::lock(/* ... */)
    }

    pub fn wait_timeout<T>(&self, guard: MutexGuard<T>, timeout: Duration) -> MutexGuard<T> {
        drop(guard);
        let timeout_ns = timeout.as_nanos() as u64;
        self.wait_queue.wait(current_task(), Some(timeout_ns));
        Mutex::lock(/* ... */)
    }

    pub fn notify_one(&self) {
        self.wait_queue.wake_one();
    }

    pub fn notify_all(&self) {
        self.wait_queue.wake_all();
    }
}

---

### 4. Memory Management (mm/)

#### mm/pmm.rs

用途: 物理内存管理（页帧分配和回收）

实现思路:
- 使用伙伴系统（Buddy System）分配器，高效且支持任意大小分配
- 维护多个 zone（DMA zone, normal zone, high memory zone）
- 跟踪页面使用情况（free, allocated, reserved）
- 支持 NUMA 感知分配

实现方法:
pub const PAGE_SIZE: usize = 4096;
pub const MAX_ORDER: usize = 11;  // 2^11 pages = 8 MB

pub struct BuddyAllocator {
    free_lists: [SpinLock<LinkedList<PageFrame>>; MAX_ORDER],
    total_pages: usize,
    used_pages: Atomic<usize>,
}

impl BuddyAllocator {
    pub fn alloc(&self, order: usize) -> Option<PageFrame> {
        if order >= MAX_ORDER {
            return None;
        }

        // 尝试在 free_lists[order] 中获取
        {
            let mut list = self.free_lists[order].lock();
            if let Some(frame) = list.pop_front() {
                self.used_pages.fetch_add(1 << order, Ordering::Relaxed);
                return Some(frame);
            }
        }

        // 递归地从更大的块中分裂
        for split_order in (order + 1)..MAX_ORDER {
            let mut list = self.free_lists[split_order].lock();
            if let Some(mut frame) = list.pop_front() {
                drop(list);

                // 分裂成两个较小的块
                for o in (order..split_order).rev() {
                    let buddy = frame.split();
                    self.free_lists[o + 1].lock().push_back(buddy);
                }

                self.used_pages.fetch_add(1 << order, Ordering::Relaxed);
                return Some(frame);
            }
        }

        None
    }

    pub fn free(&self, mut frame: PageFrame, order: usize) {
        self.used_pages.fetch_sub(1 << order, Ordering::Relaxed);

        // 检查伙伴是否空闲，尝试合并
        let mut current_order = order;
        loop {
            if current_order >= MAX_ORDER - 1 {
                self.free_lists[current_order].lock().push_back(frame);
                break;
            }

            let buddy_addr = frame.buddy_address(current_order);
            let mut list = self.free_lists[current_order].lock();

            // 查找伙伴
            if let Some(buddy_idx) = list.find_by_addr(buddy_addr) {
                let buddy = list.remove(buddy_idx);
                drop(list);

                // 合并
                frame = frame.merge(buddy);
                current_order += 1;
            } else {
                list.push_back(frame);
                break;
            }
        }
    }
}

---
#### mm/vmm.rs

用途: 虚拟内存管理（页表操作和地址映射）- 通用接口

实现思路:
- 定义虚拟内存管理的通用接口（不涉及具体页表结构）
- 维护虚拟地址空间布局、VMA（虚拟内存区域）、地址映射关系
- 支持大页、写时复制（COW）和按需分页（demand paging）
- **平台特定部分**（四级页表、PML4/PDPT/PD/PT 等）应由 `arch/x86_64/mmu.rs` 或 `arch/x86_64/memory.rs` 实现

通用接口定义:
pub trait PageTableOps {
    unsafe fn translate(&self, vaddr: u64) -> Option<u64>;
    unsafe fn map(&mut self, vaddr: u64, paddr: u64, flags: u64) -> Result<()>;
    unsafe fn unmap(&mut self, vaddr: u64) -> Result<()>;
}

pub struct VirtualMemoryManager {
    page_table: Arc<dyn PageTableOps>,
    vma_list: BTreeMap<u64, VirtualMemoryArea>,
    // ... 地址空间布局由 arch 层通过 ARCH_CONFIG 提供
}

**x86-64 特定实现** 应在 `arch/x86_64/memory.rs` 中定义：
```rust
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
pub const USER_BASE: u64 = 0x0000_0000_0000_0000;
pub const KERNEL_END: u64 = 0xFFFF_FFFF_FFFF_FFFF;
pub const LARGE_PAGE_SIZE: usize = 0x200_000;      // 2MB
pub const HUGE_PAGE_SIZE: usize = 0x4000_0000;     // 1GB
```

实现示例（x86-64 特定）:
```rust
pub struct X86_64PageTable {
    pml4: *mut [PtEntry; 512],  // 四级页表根
}

impl PageTableOps for X86_64PageTable {
    pub unsafe fn translate(&self, vaddr: u64) -> Option<u64> {
        let indexes = [
            (vaddr >> 39) & 0x1FF,  // PML4 索引
            (vaddr >> 30) & 0x1FF,  // PDPT 索引
            (vaddr >> 21) & 0x1FF,  // PD 索引
            (vaddr >> 12) & 0x1FF,  // PT 索引
        ];

        let mut table = self.pml4;

        for (i, &idx) in indexes.iter().enumerate() {
            let entry = (*table)[idx as usize];
            if !entry.present() {
                return None;
            }

            if i == 3 {
                // 页表级别
                return Some(entry.address() + (vaddr & 0xFFF));
            }

            table = entry.address() as *mut _;
        }

        None
    }

    pub unsafe fn map(
        &mut self,
        vaddr: u64,
        paddr: u64,
        flags: u64,
    ) -> Result<()> {
        // ... 页表遍历和填充逻辑
    }

    pub unsafe fn unmap(&mut self, vaddr: u64) -> Result<()> {
        // ... 页表清除逻辑
    }
}
```

---
#### mm/heap.rs 和 mm/slub.rs

用途: 内存堆分配器（SLUB - Slab Unqueued Allocator）

实现思路:
- 使用 SLUB 算法（Linux 采用的现代 slab 分配器）
- 维护多个大小类别的对象缓存
- 支持 per-CPU 缓存（减少锁竞争）
- 自动合并空闲 slab

代码框架:
pub const SLUB_SIZE_CLASSES: &[usize] = &[
    8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192,
];

pub struct SlubAllocator {
    caches: [Spinlock<SlubCache>; SLUB_SIZE_CLASSES.len()],
    per_cpu_caches: PerCpuArray<SlubCache>,
}

pub struct SlubCache {
    slab_list: LinkedList<Slab>,
    free_list: LinkedList<Object>,
    partial_slabs: LinkedList<Slab>,
    object_size: usize,
}

impl SlubCache {
    pub fn allocate(&mut self) -> Option<*mut u8> {
        // 从 per-CPU 缓存分配
        // 如果空，从 partial slabs 分配
        // 如果没有 partial，创建新 slab
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        // 找到对象所属的 slab
        // 将对象标记为空闲
        // 如果 slab 完全空闲，回收给物理内存管理器
    }
}

pub unsafe impl GlobalAlloc for SlubAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().next_power_of_two();
        // 根据 size 选择合适的 cache
        // ... 分配逻辑
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // ... 回收逻辑
    }
}

---

### 5. Task Management (task/)

#### task/pcb.rs

用途: 进程控制块（PCB）结构定义

实现思路:
- 定义完整的进程元数据结构
- 包含进程状态、寄存器上下文、内存管理、文件描述符等
- 使用引用计数（Arc）便于多个调度器访问

代码:
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Zombie,
    Stopped,
}

pub struct ProcessControlBlock {
    // 标识
    pub pid: Pid,
    pub parent_pid: Option<Pid>,
    pub children_pids: Vec<Pid>,

    // 状态
    pub state: ProcessState,
    pub context: ExceptionFrame,
    pub kernel_stack: VirtualAddress,
    pub user_stack: VirtualAddress,

    // 内存管理
    pub page_table: Arc<PageTable>,
    pub vma_list: Vec<VirtualMemoryArea>,
    pub brk: VirtualAddress,

    // 调度相关
    pub priority: i32,
    pub cpu_affinity: CpuMask,
    pub last_cpu: u32,
    pub vruntime: u64,  // CFS 调度器用
    pub time_slice: u64,

    // 文件管理
    pub cwd: PathBuf,
    pub open_files: Vec<Option<FileDescriptor>>,
    pub umask: Mode,

    // 信号处理
    pub signal_handlers: [Option<SignalHandler>; 64],
    pub pending_signals: SignalSet,
    pub signal_mask: SignalSet,

    // 资源限制
    pub rlimits: ResourceLimits,

    // 时间统计
    pub utime: u64,  // 用户态时间
    pub stime: u64,  // 内核态时间
    pub start_time: u64,

    // 命名空间和 cgroup
    pub namespaces: NamespaceSet,
    pub cgroup: Arc<Cgroup>,
}

#[repr(C)]
pub struct ExceptionFrame {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

---
#### task/process.rs

用途: 进程生命周期管理（fork, exec, exit, wait）

实现思路:
- 实现 fork 系统调用（创建新进程，使用写时复制）
- 实现 exec 系统调用（替换进程镜像）
- 实现 exit 系统调用（进程终止）
- 实现 wait 系统调用（等待子进程）

代码框架:
pub fn sys_fork(parent_frame: &ExceptionFrame) -> Result<Pid> {
    let current = current_process();

    // 1. 创建新 PCB
    let mut child_pcb = current.pcb.clone();
    child_pcb.pid = allocate_new_pid();
    child_pcb.parent_pid = Some(current.pid);
    child_pcb.children_pids.clear();

    // 2. 复制页表（写时复制）
    child_pcb.page_table = current.page_table.clone_cow()?;

    // 3. 设置返回值（父进程返回子 PID，子进程返回 0）
    let mut child_frame = parent_frame.clone();
    child_frame.rax = 0;  // 子进程的返回值
    child_pcb.context = child_frame;

    // 4. 添加到调度器
    SCHEDULER.add_process(Arc::new(child_pcb))?;

    Ok(child_pcb.pid)
}

pub fn sys_exec(path: &str, args: &[&str], envp: &[&str]) -> Result<()> {
    let current = current_process_mut();

    // 1. 打开并验证可执行文件
    let file = VFS.open(path, OpenFlags::RDONLY)?;
    let elf_data = file.read_all()?;

    // 2. 解析 ELF 文件
    let elf = ElfParser::parse(&elf_data)?;

    // 3. 清空旧地址空间（除了内核空间）
    current.page_table.clear_user_space()?;
    current.vma_list.clear();
    current.open_files.retain(|fd| fd.is_some() && !fd.unwrap().close_on_exec);

    // 4. 加载 ELF 段
    for segment in &elf.segments {
        if segment.p_type == PT_LOAD {
            let vaddr = VirtualAddress(segment.p_vaddr as u64);
            let data = &elf_data[segment.p_offset..][..segment.p_filesz];

            // 分配内存并映射
            let num_pages = (segment.p_memsz + PAGE_SIZE - 1) / PAGE_SIZE;
            for i in 0..num_pages {
                let paddr = PMM.alloc(0)?;
                let prot = segment.flags_to_prot();
                current.page_table.map(vaddr + (i * PAGE_SIZE), paddr, prot)?;
            }

            // 复制段内容
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    vaddr.as_mut_ptr(),
                    data.len(),
                );
            }
        }
    }

    // 5. 设置用户栈
    let user_stack_top = USER_STACK_END;
    current.user_stack = VirtualAddress(user_stack_top);

    // 在栈上放置 argv 和 envp
    let mut sp = user_stack_top;
    // ... 压入参数

    // 6. 更新进程状态
    current.context.rip = elf.entry_point;
    current.context.rsp = sp;
    current.context.rax = 0;

    Ok(())
}

pub fn sys_exit(exit_code: i32) -> ! {
    let current = current_process_mut();

    // 1. 关闭所有打开的文件
    current.open_files.iter_mut().for_each(|fd| {
        if let Some(f) = fd.take() {
            let _ = f.close();
        }
    });

    // 2. 释放内存
    current.page_table.clear_user_space().ok();

    // 3. 发送信号给父进程
    if let Some(parent_pid) = current.parent_pid {
        if let Some(parent) = TASK_MANAGER.get_process(parent_pid) {
            parent.send_signal(SIGCHLD);
        }
    }

    // 4. 标记为 zombie 状态
    current.state = ProcessState::Zombie;
    current.exit_code = exit_code;

    // 5. 切换到其他进程
    schedule();
    unreachable!()
}

pub fn sys_wait(pid: Option<Pid>) -> Result<(Pid, i32)> {
    let current = current_process_mut();

    loop {
        // 查找符合条件的子进程
        for child_pid in &current.children_pids {
            if let Some(pid_to_wait) = pid {
                if *child_pid != pid_to_wait {
                    continue;
                }
            }

            if let Some(child) = TASK_MANAGER.get_process(*child_pid) {
                match child.state {
                    ProcessState::Zombie => {
                        let exit_code = child.exit_code;
                        TASK_MANAGER.remove_process(*child_pid);
                        return Ok((*child_pid, exit_code));
                    }
                    _ => continue,
                }
            }
        }

        // 没有找到 zombie 子进程，等待
        current.state = ProcessState::Blocked;
        current.wait_condition = WaitCondition::ChildExit;
        schedule();
    }
}

---
#### task/scheduler/cfs.rs

用途: 完全公平调度器（Completely Fair Scheduler）

实现思路:
- 使用红黑树维护就绪队列
- 基于 vruntime（虚拟运行时间）进行公平调度
- 支持 CPU 亲和性
- 支持 nice 值和优先级

代码:
pub struct CfsScheduler {
    run_queue: RbTree<Pid, ScheduleEntity>,
    min_granularity: u64,  // 最小调度粒度（ns）
    sched_latency: u64,    // 调度延迟目标（ns）
    update_period: u64,    // 更新周期（ns）
}

pub struct ScheduleEntity {
    pid: Pid,
    vruntime: u64,
    nice: i32,
    weight: u64,
    time_slice: u64,
}

impl CfsScheduler {
    pub fn add_task(&mut self, entity: ScheduleEntity) {
        self.run_queue.insert(entity.pid, entity);
    }

    pub fn pick_next_task(&mut self) -> Option<Pid> {
        // 选择 vruntime 最小的任务
        self.run_queue.min().map(|(pid, _)| *pid)
    }

    pub fn update_vruntime(&mut self, pid: Pid, delta_exec: u64) {
        if let Some(entity) = self.run_queue.get_mut(&pid) {
            // vruntime = vruntime + delta_exec / weight
            entity.vruntime += (delta_exec * 1024) / entity.weight;
        }
    }

    pub fn on_tick(&mut self, current_pid: Pid, elapsed_ns: u64) {
        if let Some(entity) = self.run_queue.get_mut(&current_pid) {
            entity.time_slice -= elapsed_ns;

            // 需要切换到新任务
            if entity.time_slice <= 0 {
                return;  // 调度器应该进行上下文切换
            }
        }
    }
}

---
#### task/namespace.rs

用途: 命名空间支持（容器隔离的基础）

实现思路:
- 实现 PID 命名空间（隔离进程 ID 空间）
- 实现网络命名空间（隔离网络资源）
- 实现挂载命名空间（隔离文件系统挂载点）
- 实现 UTS 命名空间（隔离主机名）
- 实现 IPC 命名空间（隔离 IPC 资源）
- 实现用户命名空间（用户 ID 映射）

代码框架:
pub struct NamespaceSet {
    pub pid_ns: Arc<PidNamespace>,
    pub net_ns: Arc<NetNamespace>,
    pub mnt_ns: Arc<MntNamespace>,
    pub uts_ns: Arc<UtsNamespace>,
    pub ipc_ns: Arc<IpcNamespace>,
    pub user_ns: Arc<UserNamespace>,
}

pub struct PidNamespace {
    parent: Option<Arc<PidNamespace>>,
    processes: Spinlock<HashMap<Pid, Arc<Pcb>>>,
    next_pid: AtomicU32,
}

impl PidNamespace {
    pub fn allocate_pid(&self) -> Pid {
        Pid(self.next_pid.fetch_add(1, Ordering::SeqCst))
    }

    pub fn get_process(&self, pid: Pid) -> Option<Arc<Pcb>> {
        if let Some(p) = self.processes.lock().get(&pid) {
            return Some(Arc::clone(p));
        }
        // 在父命名空间中查找
        self.parent.as_ref()?.get_process(pid)
    }
}

pub struct NetNamespace {
    interfaces: Spinlock<HashMap<u32, NetworkInterface>>,
    routing_table: Spinlock<RoutingTable>,
    sockets: Spinlock<HashMap<SocketFd, Socket>>,
}

pub struct MntNamespace {
    mount_points: Spinlock<HashMap<PathBuf, MountPoint>>,
}

pub struct UtsNamespace {
    hostname: Spinlock<String>,
    domainname: Spinlock<String>,
}

---
#### task/cgroup.rs

用途: cgroup v2 资源控制

实现思路:
- 实现 cgroup v2（统一的资源管理接口）
- 支持 CPU、内存、I/O 等资源限制
- 支持层级结构
- 支持进程组控制

代码:
pub struct Cgroup {
    name: String,
    parent: Option<Arc<Cgroup>>,
    children: Spinlock<Vec<Arc<Cgroup>>>,
    processes: Spinlock<Vec<Pid>>,

    // 资源控制
    cpu_limit: CpuLimit,
    memory_limit: u64,
    io_weight: u32,
    pids_limit: u32,
}

pub struct CpuLimit {
    max_bandwidth: u64,   // 微秒/周期
    period: u64,          // 周期（微秒）
}

impl Cgroup {
    pub fn new(name: String) -> Arc<Self> {
        Arc::new(Cgroup {
            name,
            parent: None,
            children: Spinlock::new(Vec::new()),
            processes: Spinlock::new(Vec::new()),
            cpu_limit: CpuLimit::default(),
            memory_limit: u64::MAX,
            io_weight: 100,
            pids_limit: 32768,
        })
    }

    pub fn add_process(&self, pid: Pid) -> Result<()> {
        let mut procs = self.processes.lock();
        if procs.len() >= self.pids_limit as usize {
            return Err(Error::NoMemory);
        }
        procs.push(pid);
        Ok(())
    }

    pub fn get_memory_usage(&self) -> u64 {
        // 统计所有进程的内存使用
        let procs = self.processes.lock();
        procs.iter()
            .filter_map(|pid| TASK_MANAGER.get_process(*pid))
            .map(|p| p.get_memory_usage())
            .sum()
    }
}

---

### 6. Virtual File System (fs/)

#### fs/vfs.rs

用途: VFS 抽象层（所有文件系统的通用接口）

实现思路:
- 定义 FileSystem trait（表示一个挂载的文件系统）
- 定义 Inode trait（表示文件/目录）
- 定义 File trait（已打开的文件）
- 提供文件系统无关的路径解析

代码:
pub trait FileSystem: Send + Sync {
    fn root_inode(&self) -> Arc<dyn Inode>;
    fn name(&self) -> &str;
    fn flags(&self) -> FsFlags;
}

pub trait Inode: Send + Sync {
    fn inode_number(&self) -> u64;
    fn size(&self) -> u64;
    fn mode(&self) -> Mode;
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
    fn created_at(&self) -> SystemTime;
    fn modified_at(&self) -> SystemTime;
    fn accessed_at(&self) -> SystemTime;

    fn is_dir(&self) -> bool { (self.mode() & S_IFDIR) != 0 }
    fn is_file(&self) -> bool { (self.mode() & S_IFREG) != 0 }
    fn is_symlink(&self) -> bool { (self.mode() & S_IFLNK) != 0 }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;
    fn write_at(&self, offset: u64, buf: &[u8]) -> Result<usize>;

    fn readdir(&self) -> Result<Vec<DirEntry>>;
    fn lookup(&self, name: &str) -> Result<Arc<dyn Inode>>;
    fn create(&self, name: &str, mode: Mode) -> Result<Arc<dyn Inode>>;
    fn mkdir(&self, name: &str, mode: Mode) -> Result<Arc<dyn Inode>>;
    fn unlink(&self, name: &str) -> Result<()>;
    fn rmdir(&self, name: &str) -> Result<()>;

    fn get_attr(&self) -> Result<FileAttr>;
    fn set_attr(&self, attr: FileAttr) -> Result<()>;
}

pub trait File: Send + Sync {
    fn inode(&self) -> Arc<dyn Inode>;
    fn flags(&self) -> OpenFlags;
    fn position(&self) -> u64;
    fn seek(&mut self, offset: i64, whence: SeekWhence) -> Result<u64>;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn sync(&self) -> Result<()>;
}

---
#### fs/vfs.rs 中的 VFS 管理器

用途: 全局文件系统管理

实现思路:
- 维护挂载点映射
- 路径解析和遍历
- 打开文件的全局缓存

代码:
pub struct VfsManager {
    mount_points: Spinlock<HashMap<PathBuf, Arc<dyn FileSystem>>>,
    open_files: Spinlock<HashMap<Inode, Arc<dyn File>>>,
    dcache: Spinlock<Dcache>,  // 目录项缓存
    pagecache: PageCache,       // 页缓存
}

impl VfsManager {
    pub fn open(&self, path: &str, flags: OpenFlags) -> Result<Arc<dyn File>> {
        let (inode, _) = self.resolve_path(path)?;

        // 检查权限
        self.check_permission(&inode, flags)?;

        // 创建 File 对象
        let file = Arc::new(VfsFile::new(inode, flags));
        Ok(file)
    }

    fn resolve_path(&self, path: &str) -> Result<(Arc<dyn Inode>, Arc<dyn FileSystem>)> {
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let (mut inode, mut fs) = if path.starts_with('/') {
            // 绝对路径，从根文件系统开始
            let root_fs = self.mount_points.lock()
                .get(Path::new("/"))
                .ok_or(Error::NotFound)?;
            (root_fs.root_inode(), Arc::clone(root_fs))
        } else {
            // 相对路径
            let current_cwd = current_process().cwd.clone();
            self.resolve_path(current_cwd.to_str()?)?
        };

        for component in components {
            // 检查是否需要切换文件系统（挂载点）
            if let Some(mounted_fs) = self.mount_points.lock().get(&inode.path().join(component)) {
                inode = mounted_fs.root_inode();
                fs = Arc::clone(mounted_fs);
            } else {
                inode = inode.lookup(component)?;
            }
        }

        Ok((inode, fs))
    }
}

---
#### fs/filesystems/exfat.rs

用途: exFAT 文件系统驱动

实现思路:
- 支持大文件（> 4GB）
- 支持大块（4KB 及以上）
- 简化的 FAT 结构
- 跨平台兼容性

代码框架:
pub struct ExfatFilesystem {
    device: Arc<dyn BlockDevice>,
    superblock: ExfatSuperblock,
    inode_cache: Spinlock<HashMap<u64, Arc<ExfatInode>>>,
}

#[repr(C, packed)]
pub struct ExfatSuperblock {
    // ExFAT 引导扇区结构
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub reserved: [u8; 53],
    pub partition_offset: u64,
    pub volume_length: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub first_cluster_of_root: u32,
    pub volume_serial_number: u32,
    pub file_system_revision: [u8; 2],
    pub volume_flags: u16,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
}

pub struct ExfatInode {
    cluster: u32,
    size: u64,
    is_dir: bool,
    attributes: u16,
    created_at: SystemTime,
    modified_at: SystemTime,
}

impl Inode for ExfatInode {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        // 从 FAT 链表遍历集群
        // 读取数据
        Ok(buf.len())
    }

    fn readdir(&self) -> Result<Vec<DirEntry>> {
        // 遍历目录集群，解析目录项
        Ok(Vec::new())
    }
}

---

### 7. Network (net/)

#### net/ip/ipv6.rs

用途: IPv6 协议实现（现代网络优先选择 IPv6）

实现思路:
- 解析和生成 IPv6 数据包
- 实现 ICMPv6（邻居发现、路由通告）
- 支持 IPv6 地址自动配置
- 支持 IPv6 前缀委托（多子网）

代码框架:
#[repr(C, packed)]
pub struct Ipv6Header {
    pub version_class_label: u32,  // 4 bits version + 8 bits traffic class + 20 bits flow label
    pub payload_length: u16,
    pub next_header: u8,
    pub hop_limit: u8,
    pub source: Ipv6Addr,
    pub destination: Ipv6Addr,
}

pub struct Ipv6Stack {
    addresses: Spinlock<Vec<Ipv6Addr>>,
    routes: Spinlock<RoutingTable>,
    neighbors: Spinlock<NeighborCache>,
}

impl Ipv6Stack {
    pub fn receive_packet(&self, packet: &[u8]) -> Result<()> {
        let hdr = Ipv6Header::from_bytes(packet)?;

        match hdr.next_header {
            6 => {
                // TCP
                tcp_receive(&packet[40..]);
            }
            17 => {
                // UDP
                udp_receive(&packet[40..]);
            }
            58 => {
                // ICMPv6
                self.icmpv6_receive(&packet[40..])?;
            }
            _ => {}
        }

        Ok(())
    }

    fn icmpv6_receive(&self, icmpv6_packet: &[u8]) -> Result<()> {
        let icmpv6_type = icmpv6_packet[0];

        match icmpv6_type {
            135 => {
                // 邻居请求
                self.handle_neighbor_solicitation(icmpv6_packet)?;
            }
            134 => {
                // 路由通告
                self.handle_router_advertisement(icmpv6_packet)?;
            }
            128 => {
                // Echo request (ping)
                self.send_echo_reply(icmpv6_packet)?;
            }
            _ => {}
        }

        Ok(())
    }
}

---
#### net/io_uring.rs

用途: io_uring 异步 I/O 接口（现代高性能异步 I/O）

实现思路:
- 实现 io_uring 系统调用接口
- 维护提交队列和完成队列
- 支持各种 I/O 操作（read, write, connect, accept 等）
- 支持链接操作（链式执行多个 I/O）

代码:
pub struct IoUring {
    sq: Spinlock<SubmitQueue>,
    cq: Spinlock<CompleteQueue>,
    registered_buffers: Spinlock<Vec<BufferReg>>,
    sq_mmap: usize,
    cq_mmap: usize,
}

pub struct SubmitQueue {
    entries: VecDeque<IouringOp>,
    head: u32,
    tail: u32,
}

pub struct CompleteQueue {
    entries: VecDeque<IoEvent>,
    head: u32,
    tail: u32,
}

pub enum IouringOp {
    Read { fd: i32, buf: u64, len: usize },
    Write { fd: i32, buf: u64, len: usize },
    Connect { fd: i32, addr: u64, addrlen: u32 },
    Accept { fd: i32, addr: u64, addrlen: u32 },
    Timeout { ts: TimeSpec, },
}

pub struct IoEvent {
    user_data: u64,
    result: i32,
    flags: u32,
}

impl IoUring {
    pub fn submit_op(&self, op: IouringOp, user_data: u64) -> Result<()> {
        let mut sq = self.sq.lock();
        sq.entries.push_back(op);
        // 发送门铃信号给内核
        Ok(())
    }

    pub fn get_events(&self, min_complete: u32, timeout: Option<Duration>) -> Result<Vec<IoEvent>> {
        let cq = self.cq.lock();
        let mut events = Vec::new();
        while let Some(event) = cq.entries.pop_front() {
            events.push(event);
        }
        Ok(events)
    }
}

pub fn sys_io_uring_setup(entries: u32) -> Result<Arc<IoUring>> {
    let io_uring = Arc::new(IoUring {
        sq: Spinlock::new(SubmitQueue::new(entries as usize)),
        cq: Spinlock::new(CompleteQueue::new(entries as usize)),
        registered_buffers: Spinlock::new(Vec::new()),
        sq_mmap: 0,
        cq_mmap: 0,
    });
    Ok(io_uring)
}

pub fn sys_io_uring_enter(io_uring: &Arc<IoUring>, to_submit: u32, min_complete: u32) -> Result<u32> {
    let sq = io_uring.sq.lock();
    let submitted = (sq.entries.len() as u32).min(to_submit);

    // 处理提交的操作
    for _ in 0..submitted {
        if let Some(op) = sq.entries.pop_front() {
            // 执行 I/O 操作
            execute_io_operation(op)?;
        }
    }

    Ok(submitted)
}

---

### 8. Device Drivers (drivers/)

#### drivers/block/nvme.rs

用途: NVMe（现代高速存储）驱动

实现思路:
- 通过 PCI 发现 NVMe 设备
- 实现 NVMe 命令提交和完成处理
- 支持多个队列对（queue pairs）
- 支持 MSI-X 中断

代码框架:
pub struct NvmeController {
    pci_device: Arc<PciDevice>,
    bar0: *mut u8,           // BAR0 寄存器地址
    admin_queue: NvmeQueue,
    io_queues: Vec<NvmeQueue>,
    ns_list: Vec<NvmeNamespace>,
}

pub struct NvmeQueue {
    sqe_base: u64,     // 提交队列入口基地址
    cqe_base: u64,     // 完成队列入口基地址
    sq_head: u16,
    sq_tail: u16,
    cq_head: u16,
    cq_phase: bool,
}

impl NvmeController {
    pub fn new(pci_device: Arc<PciDevice>) -> Result<Self> {
        let mut ctrl = NvmeController {
            pci_device,
            bar0: null_mut(),
            admin_queue: NvmeQueue::new(),
            io_queues: Vec::new(),
            ns_list: Vec::new(),
        };

        // 1. 初始化控制器
        ctrl.enable_controller()?;

        // 2. 创建 I/O 队列对
        for i in 0..num_cpus() {
            ctrl.create_io_queue_pair(i)?;
        }

        // 3. 识别名字空间
        ctrl.identify_namespaces()?;

        Ok(ctrl)
    }

    pub fn submit_read(&self, ns_id: u32, lba: u64, num_blocks: u16, buffer: &mut [u8]) -> Result<()> {
        // 构造 NVMe Read 命令
        let cmd = NvmeCommand::Read {
            ns_id,
            lba,
            num_blocks,
        };

        // 提交到 I/O 队列
        self.io_queues[cpu_id()].submit(cmd)?;

        Ok(())
    }
}

impl BlockDevice for NvmeController {
    fn read(&self, sector: u64, count: u32, buffer: &mut [u8]) -> Result<u32> {
        // LBA 通常 = sector / 8（假设 4KB 块）
        let lba = sector >> 3;
        let num_blocks = ((count + 7) >> 3) as u16;

        self.submit_read(1, lba, num_blocks, buffer)?;
        Ok(count)
    }

    fn write(&self, sector: u64, count: u32, buffer: &[u8]) -> Result<u32> {
        // 类似 read
        Ok(count)
    }
}

---

### 9. System Calls (syscall/)

#### syscall/dispatch.rs

用途: 系统调用分发器（通用层，不涉及平台特定细节）

实现思路:
- 中央分发点，根据系统调用号路由到相应处理器
- 定义 Linux 标准的系统调用号和 ABI
- 统计系统调用使用情况（用于 eBPF 跟踪）
- 处理权限检查和审计
- **平台特定部分**（寄存器映射）由 `arch/x86_64/interrupt/handler.rs` 处理

代码（通用层）:
pub const SYS_READ: usize = 0;
pub const SYS_WRITE: usize = 1;
pub const SYS_OPEN: usize = 2;
pub const SYS_CLOSE: usize = 3;
pub const SYS_FORK: usize = 57;
pub const SYS_EXECVE: usize = 59;
pub const SYS_EXIT: usize = 60;
pub const SYS_WAIT4: usize = 114;
pub const SYS_CLOCK_GETTIME: usize = 228;
pub const SYS_IO_URING_SETUP: usize = 425;
// ... 更多系统调用号

// 通用分发函数，接收已映射的参数（不知道 x86-64 寄存器）
pub fn syscall_dispatch(
    number: usize,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> i64 {
    let result = match number {
        SYS_READ => sys_read(arg0 as i32, arg1 as *mut u8, arg2) as i64,
        SYS_WRITE => sys_write(arg0 as i32, arg1 as *const u8, arg2) as i64,
        SYS_OPEN => sys_open(arg0 as *const u8, arg1 as i32, arg2 as u32) as i64,
        SYS_CLOSE => sys_close(arg0 as i32) as i64,
        SYS_FORK => sys_fork() as i64,
        SYS_EXECVE => sys_execve(arg0 as *const u8, arg1 as *const *const u8) as i64,
        SYS_EXIT => sys_exit(arg0 as i32),
        SYS_CLOCK_GETTIME => sys_clock_gettime(arg0, arg1 as *mut TimeSpec) as i64,
        SYS_IO_URING_SETUP => sys_io_uring_setup(arg0 as u32) as i64,
        _ => -ENOSYS as i64,
    };

    result
}

**x86-64 特定实现** 应在 `arch/x86_64/interrupt/handler.rs` 中：
```rust
// x86-64 中断处理，将寄存器映射到通用参数
pub extern "x86-interrupt" fn syscall_handler(frame: &mut ExceptionFrame) {
    // x86-64 syscall 指令约定：
    // 系统调用号在 rax，参数在 rdi, rsi, rdx, r10, r8, r9
    let number = frame.rax as usize;
    let arg0 = frame.rdi;
    let arg1 = frame.rsi;
    let arg2 = frame.rdx;
    let arg3 = frame.r10;
    let arg4 = frame.r8;
    let arg5 = frame.r9;

    // 调用通用分发器（位于 syscall/dispatch.rs）
    let result = syscall_dispatch(number, arg0, arg1, arg2, arg3, arg4, arg5);
    
    // 返回值放入 rax
    frame.rax = result as u64;
}
```

---

#### syscall/io_uring.rs

**用途**: io_uring 系统调用实现

**实现思路**:
- io_uring_setup：创建 io_uring 实例
- io_uring_enter：提交 I/O 操作并等待完成
- io_uring_register：注册缓冲区、文件等资源

**代码**:
```rust
pub fn sys_io_uring_setup(entries: u32) -> Result<i32> {
    let io_uring = Arc::new(IoUring::new(entries as usize)?);
    let current = current_process_mut();

    // 分配文件描述符
    let fd = current.alloc_fd();
    current.open_files[fd] = Some(FileDescriptor::IoUring(io_uring));

    Ok(fd as i32)
}

pub fn sys_io_uring_enter(fd: i32, to_submit: u32, min_complete: u32, flags: u32, timeout: u64) -> Result<u32> {
    let current = current_process();
    let io_uring = match &current.open_files[fd as usize] {
        Some(FileDescriptor::IoUring(uring)) => uring,
        _ => return Err(Error::BadFileDescriptor),
    };

    let submitted = io_uring.submit(to_submit)?;

    // 如果需要等待完成事件
    if min_complete > 0 {
        io_uring.wait_completion(min_complete, timeout)?;
    }

    Ok(submitted)
}

pub fn sys_io_uring_register(fd: i32, opcode: u32, arg: u64, nr_args: u32) -> Result<()> {
    let current = current_process();
    let io_uring = match &current.open_files[fd as usize] {
        Some(FileDescriptor::IoUring(uring)) => uring,
        _ => return Err(Error::BadFileDescriptor),
    };

    match opcode {
        IORING_REGISTER_BUFFERS => {
            // 注册缓冲区以供 I/O 使用
            io_uring.register_buffers(arg as *const IoringIovec, nr_args)?;
        }
        IORING_REGISTER_FILES => {
            // 注册文件描述符以供 I/O 使用
            io_uring.register_files(arg as *const i32, nr_args)?;
        }
        _ => return Err(Error::Invalid),
    }

    Ok(())
}
```

---

### 10. Library Functions (lib/)

#### lib/collections/lockfree/

用途: 无锁数据结构（高性能并发编程）

实现思路:
- 使用原子操作（CAS - Compare-and-Swap）实现无锁算法
- 减少锁竞争，提高多核可扩展性
- 支持任意数量的读者/写者并发访问

代码框架:
pub struct LockfreeStack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    data: T,
    next: AtomicPtr<Node<T>>,
}

impl<T> LockfreeStack<T> {
    pub fn push(&self, value: T) {
        let new_node = Box::into_raw(Box::new(Node {
            data: value,
            next: AtomicPtr::new(null_mut()),
        }));

        loop {
            let head = self.head.load(Ordering::Acquire);
            unsafe {
                (*new_node).next.store(head, Ordering::Release);
            }

            match self.head.compare_exchange(
                head,
                new_node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head.is_null() {
                return None;
            }

            let next = unsafe { (*head).next.load(Ordering::Acquire) };

            match self.head.compare_exchange(
                head,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(unsafe {
                        (*Box::from_raw(head)).data
                    });
                }
                Err(_) => continue,
            }
        }
    }
}

pub struct LockfreeHashMap<K, V> {
    buckets: Vec<LockfreeChain<K, V>>,
}

struct LockfreeChain<K, V> {
    head: AtomicPtr<HashNode<K, V>>,
}

struct HashNode<K, V> {
    key: K,
    value: V,
    next: AtomicPtr<HashNode<K, V>>,
}

impl<K: Hash + Eq, V> LockfreeHashMap<K, V> {
    pub fn insert(&self, key: K, value: V) {
        let hash = hash(&key) as usize;
        let bucket_idx = hash % self.buckets.len();
        self.buckets[bucket_idx].insert(key, value);
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let hash = hash(key) as usize;
        let bucket_idx = hash % self.buckets.len();
        self.buckets[bucket_idx].find(key)
    }
}

---
#### lib/collections/radix_tree.rs

用途: Radix 树（页表查找加速）

实现思路:
- 紧凑的树结构，支持 O(k) 查找（k 为键长）
- 用于虚拟地址到物理地址的快速映射
- 支持范围查询

代码:
pub struct RadixTree<T> {
    root: Option<Box<RadixNode<T>>>,
}

pub enum RadixNode<T> {
    Leaf(T),
    Internal {
        children: [Option<Box<RadixNode<T>>>; 256],
        prefix_bits: u8,
    },
}

impl<T> RadixTree<T> {
    pub fn insert(&mut self, key: u64, value: T) {
        // 从高位到低位遍历 key，每次 8 bits
        // ...
    }

    pub fn lookup(&self, key: u64) -> Option<&T> {
        let mut node = self.root.as_ref()?;

        for shift in (0..8).rev() {
            let index = ((key >> (shift * 8)) & 0xFF) as usize;

            match node.as_ref() {
                RadixNode::Leaf(val) => return Some(val),
                RadixNode::Internal { children, .. } => {
                    node = children[index].as_ref()?;
                }
            }
        }

        None
    }
}

---

### 11. Linux Compatibility Layer (compat/)

#### compat/loader.rs

用途: ELF 加载器，支持动态链接的 Linux 程序

实现思路:
- 解析 ELF 文件头和程序头
- 支持 PT_INTERP（动态链接器）段
- 加载共享库依赖
- 构建进程初始栈（argc、argv、envp、auxv）
- 支持 PIE (Position Independent Executable)

代码框架:
```rust
pub struct ElfLoader {
    base_addr: u64,
    entry_point: u64,
    interpreter: Option<String>,
    segments: Vec<LoadedSegment>,
}

pub struct LoadedSegment {
    vaddr: u64,
    size: usize,
    offset: usize,
    flags: u32,
}

impl ElfLoader {
    pub fn load_linux_binary(path: &str) -> Result<Arc<ProcessControlBlock>> {
        let file = VFS.open(path, OpenFlags::RDONLY)?;
        let elf_data = file.read_all()?;
        
        // 1. 解析 ELF 头
        let elf = ElfParser::parse(&elf_data)?;
        
        // 2. 检查是否需要动态链接器
        let interpreter = elf.find_interpreter()?;
        
        let (entry, base) = if let Some(interp_path) = interpreter {
            // 动态链接程序，需要加载 ld-linux.so
            let interp_elf = Self::load_interpreter(&interp_path)?;
            (interp_elf.entry_point, interp_elf.base_addr)
        } else {
            // 静态链接程序
            (elf.entry_point, 0)
        };
        
        // 3. 创建进程并设置地址空间
        let mut pcb = ProcessControlBlock::new();
        pcb.personality = Personality::PER_LINUX;
        
        // 4. 加载 ELF 段到内存
        for segment in &elf.segments {
            if segment.p_type == PT_LOAD {
                Self::load_segment(&mut pcb, segment, &elf_data)?;
            }
        }
        
        // 5. 设置用户栈并构建辅助向量
        let stack_top = Self::setup_initial_stack(
            &mut pcb,
            &elf,
            entry,
            base,
        )?;
        
        // 6. 设置进程入口点
        pcb.context.rip = entry;
        pcb.context.rsp = stack_top;
        
        Ok(Arc::new(pcb))
    }
    
    fn load_interpreter(path: &str) -> Result<ElfLoader> {
        // 加载 /lib64/ld-linux-x86-64.so.2
        // 或 /lib/ld-musl-x86_64.so.1
        let file = VFS.open(path, OpenFlags::RDONLY)?;
        let elf_data = file.read_all()?;
        let elf = ElfParser::parse(&elf_data)?;
        
        // 动态链接器通常是 PIE，需要选择基地址
        let base_addr = 0x7ffff7dd0000u64; // 典型的动态链接器加载地址
        
        Ok(ElfLoader {
            base_addr,
            entry_point: base_addr + elf.entry_point,
            interpreter: None,
            segments: Vec::new(),
        })
    }
}
```

---

#### compat/auxv.rs

用途: 辅助向量（Auxiliary Vector）构建

实现思路:
- 在进程栈上构建 auxv，传递内核信息给用户态
- Linux 动态链接器和 libc 依赖 auxv 获取系统信息
- 包含 VDSO 地址、页大小、程序头位置等

代码:
```rust
// Linux auxv 类型常量
pub const AT_NULL: u64 = 0;
pub const AT_IGNORE: u64 = 1;
pub const AT_EXECFD: u64 = 2;
pub const AT_PHDR: u64 = 3;          // 程序头表地址
pub const AT_PHENT: u64 = 4;         // 程序头表项大小
pub const AT_PHNUM: u64 = 5;         // 程序头表项数量
pub const AT_PAGESZ: u64 = 6;        // 页大小
pub const AT_BASE: u64 = 7;          // 解释器基地址
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;         // 程序入口点
pub const AT_NOTELF: u64 = 10;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_PLATFORM: u64 = 15;     // 平台字符串
pub const AT_HWCAP: u64 = 16;        // 硬件能力标志
pub const AT_CLKTCK: u64 = 17;       // 时钟频率
pub const AT_SECURE: u64 = 23;       // 安全模式标志
pub const AT_RANDOM: u64 = 25;       // 16 字节随机数
pub const AT_HWCAP2: u64 = 26;
pub const AT_EXECFN: u64 = 31;       // 可执行文件路径
pub const AT_SYSINFO_EHDR: u64 = 33; // VDSO 地址

pub struct AuxvBuilder {
    entries: Vec<(u64, u64)>,
}

impl AuxvBuilder {
    pub fn new() -> Self {
        AuxvBuilder { entries: Vec::new() }
    }
    
    pub fn add(&mut self, typ: u64, val: u64) {
        self.entries.push((typ, val));
    }
    
    pub fn build_for_linux_process(
        &mut self,
        elf: &ElfParser,
        interp_base: u64,
        vdso_addr: u64,
    ) {
        self.add(AT_PHDR, elf.phdr_addr);
        self.add(AT_PHENT, elf.phdr_entry_size as u64);
        self.add(AT_PHNUM, elf.phdr_count as u64);
        self.add(AT_PAGESZ, PAGE_SIZE as u64);
        self.add(AT_BASE, interp_base);
        self.add(AT_ENTRY, elf.entry_point);
        self.add(AT_UID, current_process().uid as u64);
        self.add(AT_EUID, current_process().euid as u64);
        self.add(AT_GID, current_process().gid as u64);
        self.add(AT_EGID, current_process().egid as u64);
        self.add(AT_PLATFORM, /* "x86_64" string addr */);
        self.add(AT_HWCAP, detect_cpu_capabilities());
        self.add(AT_CLKTCK, 100); // 100 Hz
        self.add(AT_RANDOM, /* 16-byte random buffer addr */);
        self.add(AT_SYSINFO_EHDR, vdso_addr);
        self.add(AT_SECURE, 0);
        self.add(AT_NULL, 0); // 终止符
    }
    
    pub fn write_to_stack(&self, stack_ptr: &mut u64) {
        for (typ, val) in self.entries.iter().rev() {
            *stack_ptr -= 8;
            unsafe { *(*stack_ptr as *mut u64) = *val; }
            *stack_ptr -= 8;
            unsafe { *(*stack_ptr as *mut u64) = *typ; }
        }
    }
}
```

---

#### compat/syscall_translator.rs

用途: Linux 系统调用号到 AXIS 系统调用的转换

实现思路:
- 某些 Linux 系统调用需要适配或模拟
- 处理过时的系统调用（如 fork、vfork）
- 记录不兼容的系统调用
- **注意！下方代码所示的调用号转换仅为示意，编码时以当时具体情况为准！**
- *如 Linux 与 AXIS 的系统调用号本身相同，这部分是否可以适当简化？*

代码:
```rust
pub struct SyscallTranslator;

impl SyscallTranslator {
    pub fn translate_linux_syscall(
        linux_nr: usize,
        args: &[u64; 6],
    ) -> Result<i64> {
        match linux_nr {
            // 文件 I/O
            0 => sys_read(args[0] as i32, args[1] as *mut u8, args[2]),
            1 => sys_write(args[0] as i32, args[1] as *const u8, args[2]),
            2 => sys_open(args[0] as *const u8, args[1] as i32, args[2] as u32),
            3 => sys_close(args[0] as i32),
            
            // 进程管理
            57 => sys_fork(),
            58 => sys_vfork(), // 映射到 fork + COW
            59 => sys_execve(args[0] as *const u8, args[1] as *const *const u8, args[2] as *const *const u8),
            60 => sys_exit(args[0] as i32),
            61 => sys_wait4(args[0] as i32, args[1] as *mut i32, args[2] as i32),
            
            // 内存管理
            9 => sys_mmap(args[0], args[1], args[2] as i32, args[3] as i32, args[4] as i32, args[5] as i64),
            10 => sys_mprotect(args[0], args[1], args[2] as i32),
            11 => sys_munmap(args[0], args[1]),
            12 => sys_brk(args[0]),
            
            // 信号
            13 => sys_rt_sigaction(args[0] as i32, args[1] as *const SigAction, args[2] as *mut SigAction),
            14 => sys_rt_sigprocmask(args[0] as i32, args[1] as *const SigSet, args[2] as *mut SigSet),
            15 => sys_rt_sigreturn(),
            
            // 时间
            228 => sys_clock_gettime(args[0] as i32, args[1] as *mut TimeSpec),
            230 => sys_clock_nanosleep(args[0] as i32, args[1] as i32, args[2] as *const TimeSpec, args[3] as *mut TimeSpec),
            
            // 网络
            41 => sys_socket(args[0] as i32, args[1] as i32, args[2] as i32),
            42 => sys_connect(args[0] as i32, args[1] as *const SockAddr, args[2] as u32),
            43 => sys_accept(args[0] as i32, args[1] as *mut SockAddr, args[2] as *mut u32),
            
            // 现代异步 I/O
            425 => sys_io_uring_setup(args[0] as u32, args[1] as *const IoUringParams),
            426 => sys_io_uring_enter(args[0] as i32, args[1] as u32, args[2] as u32, args[3] as u32),
            427 => sys_io_uring_register(args[0] as i32, args[1] as u32, args[2], args[3] as u32),
            
            // 不支持的系统调用
            _ => {
                log::warn!("Unsupported Linux syscall: {}", linux_nr);
                Err(Error::NotImplemented)
            }
        }
    }
}
```

---

#### compat/signal_adapter.rs

用途: Linux 信号语义适配

实现思路:
- Linux 信号处理函数调用约定（sigaction、sigreturn）
- 支持实时信号（SIGRTMIN - SIGRTMAX）
- 信号栈切换（SA_ONSTACK）
- **注意！下方代码所示的信号转换仅为示意，编码时以当时具体情况为准！**
- *如 Linux 与 AXIS 的信号编号本身相同，这部分是否可以适当简化？*

代码:
```rust
// Linux 信号编号
pub const LINUX_SIGHUP: i32 = 1;
pub const LINUX_SIGINT: i32 = 2;
pub const LINUX_SIGQUIT: i32 = 3;
pub const LINUX_SIGILL: i32 = 4;
pub const LINUX_SIGTRAP: i32 = 5;
pub const LINUX_SIGABRT: i32 = 6;
pub const LINUX_SIGBUS: i32 = 7;
pub const LINUX_SIGFPE: i32 = 8;
pub const LINUX_SIGKILL: i32 = 9;
pub const LINUX_SIGUSR1: i32 = 10;
pub const LINUX_SIGSEGV: i32 = 11;
pub const LINUX_SIGUSR2: i32 = 12;
pub const LINUX_SIGPIPE: i32 = 13;
pub const LINUX_SIGALRM: i32 = 14;
pub const LINUX_SIGTERM: i32 = 15;
pub const LINUX_SIGCHLD: i32 = 17;
pub const LINUX_SIGCONT: i32 = 18;
pub const LINUX_SIGSTOP: i32 = 19;

pub struct SignalAdapter;

impl SignalAdapter {
    pub fn linux_to_axis_signal(linux_sig: i32) -> Option<i32> {
        // 映射 Linux 信号到 AXIS 内部信号
        match linux_sig {
            LINUX_SIGHUP => Some(AXIS_SIGHUP),
            LINUX_SIGINT => Some(AXIS_SIGINT),
            LINUX_SIGSEGV => Some(AXIS_SIGSEGV),
            LINUX_SIGTERM => Some(AXIS_SIGTERM),
            LINUX_SIGCHLD => Some(AXIS_SIGCHLD),
            _ => None,
        }
    }
    
    pub fn setup_signal_frame(
        process: &mut ProcessControlBlock,
        signum: i32,
        handler: u64,
    ) -> Result<()> {
        // 在用户栈上构建 Linux 信号栈帧
        let mut sp = process.context.rsp;
        
        // 保存原始上下文
        sp -= core::mem::size_of::<SigContext>() as u64;
        let sigctx = sp as *mut SigContext;
        unsafe {
            (*sigctx).rip = process.context.rip;
            (*sigctx).rsp = process.context.rsp;
            (*sigctx).rax = process.context.rax;
            // ... 保存其他寄存器
        }
        
        // 设置返回地址为 sigreturn
        sp -= 8;
        unsafe { *(sp as *mut u64) = process.vdso_sigreturn_addr; }
        
        // 调用信号处理函数
        process.context.rip = handler;
        process.context.rsp = sp;
        process.context.rdi = signum as u64; // 第一个参数是信号编号
        
        Ok(())
    }
}
```

---

#### compat/procfs_emulation.rs

用途: /proc 文件系统模拟

实现思路:
- 许多 Linux 程序依赖 /proc 获取系统信息
- 模拟关键的 /proc 文件（/proc/self、/proc/cpuinfo、/proc/meminfo）
- 懒加载动态生成文件内容
- **注意！AXIS 本身已实现 procfs，这部分是否需要模拟有待商榷，届时视具体情况而定！**

代码:
```rust
pub struct ProcfsEmulation;

impl ProcfsEmulation {
    pub fn read_proc_file(path: &str) -> Result<Vec<u8>> {
        match path {
            "/proc/self/exe" => {
                // 返回当前进程可执行文件路径
                let current = current_process();
                Ok(current.exe_path.as_bytes().to_vec())
            }
            
            "/proc/self/maps" => {
                // 返回进程内存映射
                let current = current_process();
                let mut output = String::new();
                for vma in &current.vma_list {
                    output.push_str(&format!(
                        "{:016x}-{:016x} {} {:08x} 00:00 0    {}\n",
                        vma.start,
                        vma.end,
                        vma.prot_to_string(),
                        vma.offset,
                        vma.name,
                    ));
                }
                Ok(output.into_bytes())
            }
            
            "/proc/cpuinfo" => {
                // 返回 CPU 信息
                let cpu_info = format!(
                    "processor\t: 0\n\
                     vendor_id\t: GenuineIntel\n\
                     cpu family\t: 6\n\
                     model\t\t: 142\n\
                     model name\t: {}\n\
                     stepping\t: 12\n\
                     cpu MHz\t\t: 2400.000\n\
                     cache size\t: 8192 KB\n\
                     physical id\t: 0\n\
                     siblings\t: {}\n\
                     core id\t\t: 0\n\
                     cpu cores\t: {}\n\
                     flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov\n\n",
                    cpu_brand_string(),
                    num_cpus(),
                    num_cpus(),
                );
                Ok(cpu_info.into_bytes())
            }
            
            "/proc/meminfo" => {
                // 返回内存信息
                let pmm = PMM.lock();
                let mem_info = format!(
                    "MemTotal:       {} kB\n\
                     MemFree:        {} kB\n\
                     MemAvailable:   {} kB\n\
                     Buffers:        {} kB\n\
                     Cached:         {} kB\n",
                    pmm.total_pages * 4,
                    pmm.free_pages * 4,
                    pmm.free_pages * 4,
                    0,
                    0,
                );
                Ok(mem_info.into_bytes())
            }
            
            _ => Err(Error::NotFound),
        }
    }
}
```

---

#### compat/personality.rs

用途: 进程个性（Personality）设置

实现思路:
- Linux personality 系统调用允许进程选择执行域
- PER_LINUX 是默认的 Linux 执行域
- 影响系统调用处理、信号传递、地址空间布局

代码:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    PER_LINUX = 0x0000,
    PER_LINUX_32BIT = 0x0008,
    PER_BSD = 0x0006,
}

impl ProcessControlBlock {
    pub fn set_personality(&mut self, pers: Personality) -> Result<()> {
        self.personality = pers;
        
        match pers {
            Personality::PER_LINUX => {
                // 启用 Linux 兼容层
                self.syscall_handler = SyscallTranslator::translate_linux_syscall;
                self.signal_adapter = Some(SignalAdapter);
            }
            _ => {
                return Err(Error::NotSupported);
            }
        }
        
        Ok(())
    }
}

pub fn sys_personality(persona: u64) -> Result<u64> {
    let current = current_process_mut();
    let old_personality = current.personality as u64;
    
    if persona != 0xFFFFFFFF {
        // 设置新的 personality
        let new_pers = match persona as u32 {
            0x0000 => Personality::PER_LINUX,
            0x0008 => Personality::PER_LINUX_32BIT,
            _ => return Err(Error::Invalid),
        };
        
        current.set_personality(new_pers)?;
    }
    
    Ok(old_personality)
}
```
