# AXIS 内核实现计划

## 项目概述

**AXIS (AXIS eXecute Instructions Steadily)** 是一个用 Rust 编写的 amd64 架构类 Unix 宏内核。

### 设计目标
- 完整的类 Unix 宏内核架构
- 支持多种引导方式（自定义 bootloader、GRUB、UEFI）
- 充分利用 Rust 的内存安全特性
- 模块化、可维护的代码结构

---

## 阶段一：项目基础架构搭建

### 1.1 项目结构设计
```
AXIS/
├── bootloader/          # 自定义引导加载器
│   ├── stage1/         # MBR 引导扇区 (512字节)
│   ├── stage2/         # 第二阶段加载器
│   └── uefi/           # UEFI 引导支持
├── kernel/             # 内核主体
│   ├── arch/           # 架构相关代码
│   │   └── x86_64/
│   ├── mm/             # 内存管理
│   ├── proc/           # 进程管理
│   ├── fs/             # 文件系统
│   ├── drivers/        # 设备驱动
│   ├── syscall/        # 系统调用
│   └── lib/            # 内核库函数
├── libc/               # C 标准库实现
├── userspace/          # 用户空间程序
├── tools/              # 构建工具
└── docs/               # 文档
```

### 1.2 构建系统配置
- **Cargo workspace** 配置多个 crate
- **自定义链接脚本** 用于内核内存布局
- **构建脚本** 用于编译汇编代码和生成镜像
- **QEMU 测试脚本** 自动化测试

### 1.3 开发环境要求
- Rust nightly (需要 unstable 特性)
- QEMU 模拟器
- NASM 汇编器
- GNU binutils (ld, objcopy)
- GRUB2 工具链 (grub-mkrescue)
- OVMF (UEFI 固件)

---

## 阶段二：引导加载器实现

### 2.1 自定义 Bootloader (BIOS Legacy)

**Stage 1 - MBR Boot Sector**
- 实时模式 (Real Mode) 16位汇编
- 从磁盘加载 Stage 2
- 512 字节限制
- 文件：`bootloader/stage1/boot.asm`

**Stage 2 - Protected Mode Loader**
- 切换到保护模式 (32位)
- 启用 A20 地址线
- 加载内核到内存
- 设置基本的 GDT
- 解析 ELF 内核镜像
- 跳转到内核入口
- 文件：`bootloader/stage2/loader.asm`

### 2.2 UEFI Bootloader
- Rust 实现的 UEFI 应用程序
- 使用 `uefi` crate
- GOP (Graphics Output Protocol) 支持
- 加载内核并传递引导信息
- 文件：`bootloader/uefi/src/main.rs`

### 2.3 Multiboot2 支持
- 兼容 GRUB2 引导
- Multiboot2 头结构
- 解析引导信息（内存映射、模块等）

---

## 阶段三：内核核心功能

### 3.1 启动与初始化

**早期启动 (arch/x86_64/boot.asm)**
- 验证 CPU 支持 (CPUID 检查 long mode)
- 设置临时页表
- 切换到 Long Mode (64位)
- 设置栈指针
- 跳转到 Rust 入口点

**内核主函数 (kernel/src/main.rs)**
```rust
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // 1. 初始化串口输出/VGA
    // 2. 初始化 GDT/IDT
    // 3. 初始化物理内存管理器
    // 4. 初始化虚拟内存
    // 5. 初始化中断控制器 (PIC/APIC)
    // 6. 初始化时钟
    // 7. 初始化进程调度器
    // 8. 挂载根文件系统
    // 9. 启动 init 进程
    loop {}
}
```

### 3.2 内存管理 (mm/)

**物理内存管理 (mm/pmm.rs)**
- 位图/伙伴系统分配器
- 基于 Multiboot/UEFI 内存映射
- 物理页帧分配/释放
- 内存区域管理（DMA、内核、可用）

**虚拟内存管理 (mm/vmm.rs)**
- 四级页表 (PML4)
- 页表映射/解除映射
- 内核地址空间布局
  - 0xFFFF_8000_0000_0000 - 内核代码/数据（高半核）
  - 0xFFFF_FF00_0000_0000 - 物理内存直接映射
- 用户地址空间管理

**堆分配器 (mm/heap.rs)**
- 实现 `GlobalAlloc` trait
- 支持 `alloc` crate
- Slab 分配器用于小对象
- 页分配器用于大对象

### 3.3 中断与异常处理 (arch/x86_64/interrupt/)

**IDT 设置 (idt.rs)**
- 256 个中断描述符
- 异常处理程序 (0-31)
- 硬件中断 (32-255)
- 系统调用入口 (int 0x80 或 syscall)

**异常处理器**
- Page Fault (#PF) - 处理缺页异常
- Double Fault (#DF) - 双重异常
- General Protection Fault (#GP)
- 其他 CPU 异常

**中断控制器**
- PIC 8259 初始化和屏蔽
- APIC/x2APIC 支持（多核）
- 中断路由

**时钟中断**
- PIT (Programmable Interval Timer)
- APIC Timer
- 时间片轮转调度

### 3.4 进程管理 (proc/)

**进程控制块 (proc/task.rs)**
```rust
pub struct Task {
    pid: Pid,
    state: TaskState,      // Running, Ready, Blocked, Zombie
    context: Context,      // 寄存器上下文
    page_table: PageTable, // 页表
    kernel_stack: usize,   // 内核栈
    user_stack: usize,     // 用户栈
    open_files: Vec<File>, // 文件描述符表
    cwd: PathBuf,          // 当前工作目录
    parent: Option<Pid>,
    children: Vec<Pid>,
    signals: SignalSet,
    // ...
}
```

**调度器 (proc/scheduler.rs)**
- 多级反馈队列调度
- 时间片管理
- 上下文切换 (context_switch.asm)
- 进程优先级

**进程创建**
- fork() - 写时复制 (COW)
- exec() - 加载 ELF 可执行文件
- exit() - 进程终止
- wait() - 等待子进程

**线程支持**
- 用户级线程
- 内核线程
- 线程本地存储 (TLS)

### 3.5 系统调用 (syscall/)

**系统调用接口**
```rust
// syscall/mod.rs
pub fn syscall_handler(number: usize, args: &[usize]) -> isize {
    match number {
        SYS_READ => sys_read(...),
        SYS_WRITE => sys_write(...),
        SYS_OPEN => sys_open(...),
        SYS_FORK => sys_fork(),
        SYS_EXEC => sys_exec(...),
        // ... 100+ 系统调用
        _ => -ENOSYS,
    }
}
```

**实现类 Unix 系统调用**
- 文件操作: read, write, open, close, lseek
- 进程控制: fork, exec, exit, wait, kill
- 内存管理: brk, mmap, munmap
- 信号处理: signal, sigaction, kill
- 目录操作: chdir, getcwd, mkdir
- 网络: socket, bind, listen, accept
- ...

### 3.6 虚拟文件系统 (fs/)

**VFS 抽象层 (fs/vfs.rs)**
```rust
pub trait FileSystem {
    fn mount(&self, mount_point: &Path) -> Result<()>;
    fn unmount(&self) -> Result<()>;
    fn open(&self, path: &Path, flags: OpenFlags) -> Result<Inode>;
    // ...
}

pub trait Inode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, offset: usize, buf: &[u8]) -> Result<usize>;
    fn readdir(&self) -> Result<Vec<DirEntry>>;
    // ...
}
```

**具体文件系统实现**
- **tmpfs** - 内存文件系统（用于 /tmp）
- **devfs** - 设备文件系统（/dev）
- **procfs** - 进程信息文件系统（/proc）
- **ext2** - 实际磁盘文件系统（可选）

**文件描述符**
- 每进程文件描述符表
- stdin (0), stdout (1), stderr (2)
- 管道支持

### 3.7 设备驱动 (drivers/)

**字符设备**
- 串口 (UART 16550)
- VGA 文本模式
- 键盘 (PS/2)
- 虚拟终端 (TTY)

**块设备**
- ATA/IDE 磁盘
- AHCI (SATA)
- RAM Disk

**其他设备**
- RTC (实时时钟)
- PCI 设备枚举
- 网络设备 (E1000, virtio-net)

---

## 阶段四：高级特性

### 4.1 多核支持 (SMP)
- AP (Application Processor) 启动
- Per-CPU 数据结构
- 自旋锁/互斥锁
- CPU 间中断 (IPI)
- 负载均衡

### 4.2 同步原语
- 自旋锁 (Spinlock)
- 互斥锁 (Mutex)
- 信号量 (Semaphore)
- 读写锁 (RwLock)
- 条件变量

### 4.3 内存高级特性
- 写时复制 (Copy-on-Write)
- 请求分页 (Demand Paging)
- 页面换出/换入 (Swapping)
- 共享内存 (mmap shared)
- 内存压缩

### 4.4 网络协议栈
- 以太网层
- IP 层
- TCP/UDP
- Socket 接口

### 4.5 图形支持
- Framebuffer 驱动
- GOP (UEFI)
- VBE (BIOS)
- 简单窗口系统

---

## 阶段五：用户空间

### 5.1 C 标准库 (libc/)
- 基本 libc 函数实现
- 系统调用包装
- 动态内存分配
- 字符串处理
- 文件 I/O

### 5.2 基础工具 (userspace/)
- **init** - 初始化进程
- **shell** - 命令行解释器
- **ls, cat, echo** - 基本命令
- **ps, top** - 进程查看
- **test** - 测试程序

### 5.3 动态链接
- ELF 动态链接器
- 共享库加载
- 符号解析

---

## 阶段六：测试与优化

### 6.1 自动化测试
- 单元测试（Rust test）
- 集成测试（QEMU 自动化）
- 压力测试
- 性能基准测试

### 6.2 调试工具
- GDB 调试支持
- 内核日志系统
- Panic 处理
- 栈回溯

### 6.3 性能优化
- 热点分析
- 缓存优化
- 锁竞争优化
- 系统调用开销减少

---

## 技术难点与解决方案

### 难点 1: Rust + 裸机编程
**挑战**: no_std 环境，需要自己实现 panic handler、内存分配器
**方案**: 
- 使用 `core` crate 而非 `std`
- 实现 `GlobalAlloc` trait
- 自定义 panic handler
- 使用 `embedded-hal` 生态

### 难点 2: 内联汇编
**挑战**: 关键的底层操作需要汇编（页表切换、中断返回等）
**方案**: 
- 使用 Rust 的 `asm!` 宏
- 独立的 `.asm` 文件用于启动代码
- 包装为安全的 Rust 接口

### 难点 3: 多种引导方式
**挑战**: 需要支持 Legacy BIOS、GRUB、UEFI
**方案**: 
- 统一的内核入口接口
- 引导信息结构抽象
- 条件编译不同的启动代码

### 难点 4: 内存安全与性能
**挑战**: 内核需要高性能，但 Rust 安全检查有开销
**方案**: 
- 在安全边界使用 `unsafe`
- 零成本抽象设计
- 关键路径优化
- 合理使用 `MaybeUninit` 等

### 难点 5: 并发与同步
**挑战**: 内核并发编程复杂，容易死锁
**方案**: 
- 使用 Rust 类型系统保证安全
- `Send` 和 `Sync` trait
- 自旋锁使用 `lock_api` crate
- 锁顺序规范

---

## 实施建议

### 优先级排序
1. **最高优先级**: 启动 + 串口输出 + 基本异常处理
2. **高优先级**: 物理/虚拟内存管理 + 堆分配器
3. **中优先级**: 进程管理 + 系统调用 + VFS
4. **低优先级**: 文件系统 + 设备驱动 + 网络栈
5. **可选**: 多核 + 图形 + 高级特性

### 迭代开发策略
- **Sprint 1-2周**: 完成一个可启动的最小内核
- **Sprint 2-4周**: 内存管理 + 简单进程
- **Sprint 3-8周**: 完整进程管理 + 系统调用
- **Sprint 4-12周**: VFS + 文件系统 + 驱动
- **Sprint 5-16周**: 用户空间 + Shell
- **Sprint 6+**: 优化和高级特性

### 开发工具链
```bash
# 构建命令
make build          # 编译内核
make bootloader     # 编译引导加载器
make image          # 生成启动镜像

# 测试命令
make run-qemu       # QEMU 测试
make run-grub       # GRUB 引导测试
make run-uefi       # UEFI 测试
make debug          # GDB 调试

# 工具命令
make clean          # 清理
make doc            # 生成文档
make test           # 运行测试
```

---

## 参考资源

### 书籍与文档
- *Operating Systems: Three Easy Pieces*
- *The Linux Programming Interface*
- *Understanding the Linux Kernel*
- Rust Embedded Book
- OSDev Wiki

### 开源项目参考
- **Redox OS** - Rust 微内核操作系统
- **Tock OS** - Rust 嵌入式 OS
- **Linux Kernel** - 设计参考
- **xv6** - 教学用 Unix 内核
- **SerenityOS** - 现代类 Unix 系统

### 硬件手册
- Intel Software Developer Manual (SDM)
- AMD64 Architecture Programmer's Manual
- UEFI Specification
- Multiboot2 Specification

---

## 预期成果

### 最小可行产品 (MVP)
- 可在 QEMU 中启动
- 支持多种引导方式
- 基本内存管理
- 简单的进程调度
- 串口/VGA 输出
- 几个系统调用

### 完整版本
- 完整的类 Unix 内核
- 用户空间 Shell
- 基本文件系统
- 多个设备驱动
- 可运行简单用户程序
- 文档完善

### 扩展目标
- 多核支持
- 网络协议栈
- 图形界面
- 更多驱动和文件系统
- POSIX 兼容
