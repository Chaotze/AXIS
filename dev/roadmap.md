# AXIS 落地路线计划文档

参照 [架构设计文档](arch.md) 中的 [项目结构树](arch.md#项目结构) 和 [部分模块设计方案](arch.md#部分模块详解)，形成**模块化、可维护、高组件复用率、低重复冗余、风格规整统一的高质量 AXIS 内核**代码。

<br>



## 阶段 0: 基础设施搭建

0.1 项目初始化

- 创建根目录结构
  - mkdir -p AXIS/{bootloader,kernel,dev}
  - 创建 Cargo.toml（workspace 配置）
  - 创建 .cargo/config.toml（编译器配置）
  - 创建 .gitignore
- 配置文件
  - kernel/x86_64-unknown-axis.json（自定义 target）
  - kernel/kernel.ld（链接脚本）
  - bootloader/bios/stage2.ld（Stage2 链接脚本）
- 构建系统
  - Makefile（主构建入口）
  - build.sh（Shell 构建脚本）
  - run_qemu.sh（QEMU BIOS 模式测试）
  - run_uefi.sh（QEMU UEFI 模式测试）
  - run_grub.sh（GRUB 引导测试）

<br>



## 阶段 1: Bootloader 实现

1.1 BIOS 引导模式

文件清单：
- bootloader/bios/Cargo.toml
- bootloader/bios/stage1.asm（512 字节 MBR）
- bootloader/bios/stage2.asm（保护模式/长模式切换）
- bootloader/bios/stage2.ld
- bootloader/bios/src/lib.rs（ELF 解析、内核加载）

实现顺序：
1. Stage1
  - 初始化寄存器和堆栈
  - BIOS INT 13h 磁盘读取
  - 加载 Stage2 到 0x7e00
  - 切换到保护模式
  - 跳转到 Stage2
2. Stage2 汇编部分
  - 验证 CPU 支持长模式（CPUID）
  - 启用 PAE（Physical Address Extension）
  - 建立临时 4 级页表
  - 启用分页和长模式
  - 跳转到 Rust Stage2
3. Stage2 Rust 部分
  - ELF 文件解析器
  - 加载内核段到内存
  - 构建 Multiboot2 引导信息
  - 跳转到内核入口

1.2 UEFI 引导模式

文件清单：
- bootloader/uefi/Cargo.toml
- bootloader/uefi/build.rs
- bootloader/uefi/src/main.rs
- bootloader/uefi/src/graphics.rs
- bootloader/uefi/src/memory.rs

实现顺序：
1. 初始化 UEFI 服务
2. 获取内存映射
3. 加载内核 ELF
4. 初始化图形输出（GOP）
5. ExitBootServices 并跳转到内核

1.3 通用引导代码

文件清单：
- bootloader/common/multiboot2.rs（Multiboot2 协议定义）
- bootloader/common/boot_info.rs（引导信息结构体）

阶段目标：
能够从 BIOS 和 UEFI 固件引导并进入内核

验收标准：
- [ ] BIOS 模式下能够从 MBR 加载 Stage2
- [ ] Stage2 成功切换到 64 位长模式
- [ ] 正确解析内核 ELF 文件并加载所有段
- [ ] UEFI 模式下能够使用 GOP 初始化图形输出
- [ ] 构建完整的 Multiboot2 引导信息（内存映射、RSDP 地址等）
- [ ] 成功跳转到内核入口点，让内核输出「AXIS: AXIS eXecute Instructions Steadily」
- [ ] 在 QEMU 中验证两种引导模式都能工作

输出物：
- 可引导的磁盘镜像（BIOS 和 UEFI）
- Bootloader 单元测试通过

<br>



## 阶段 2: 内核基础架构

2.1 启动和初始化

文件清单：
- kernel/Cargo.toml
- kernel/build.rs
- kernel/src/main.rs（内核入口）
- kernel/src/lib.rs
- kernel/src/panic.rs
- kernel/src/prelude.rs
- kernel/src/config.rs

关键任务：
- 验证引导信息
- 初始化串口输出
- 实现 panic handler
- 打印启动 banner

2.2 架构特定代码（x86_64）

2.2.1 CPU 和内存初始化

文件清单：
- kernel/src/arch/mod.rs
- kernel/src/arch/x86_64/mod.rs
- kernel/src/arch/x86_64/boot.asm
- kernel/src/arch/x86_64/cpu.rs
- kernel/src/arch/x86_64/gdt.rs
- kernel/src/arch/x86_64/memory.rs
- kernel/src/arch/x86_64/paging.rs

实现顺序：
1. boot.asm：验证长模式、初始化栈、调用 kernel_main
2. cpu.rs：CPUID 检测、特性启用（SSE、AVX、SMEP/SMAP）
3. gdt.rs：GDT 设置、TSS 初始化
4. memory.rs：内存布局常量、地址转换工具
5. paging.rs：4 级页表操作

2.2.2 中断处理

文件清单：
- kernel/src/arch/x86_64/idt.rs
- kernel/src/arch/x86_64/interrupt/mod.rs
- kernel/src/arch/x86_64/interrupt/entry.asm
- kernel/src/arch/x86_64/interrupt/handler.rs
- kernel/src/arch/x86_64/interrupt/apic.rs
- kernel/src/arch/x86_64/interrupt/ioapic.rs
- kernel/src/arch/x86_64/interrupt/msi.rs
- kernel/src/arch/x86_64/interrupt/timer.rs

实现顺序：
1. idt.rs：IDT 表结构、256 个中断门
2. entry.asm：中断入口存根（保存/恢复寄存器）
3. handler.rs：异常处理器（#PF、#GP、#DF 等）
4. apic.rs：Local APIC 初始化、IPI
5. ioapic.rs：I/O APIC 配置、IRQ 路由
6. msi.rs：MSI/MSI-X 配置
7. timer.rs：定时器中断（PIT/APIC Timer）

2.2.3 上下文切换

文件清单：
- kernel/src/arch/x86_64/context/mod.rs
- kernel/src/arch/x86_64/context/frame.rs
- kernel/src/arch/x86_64/context/switch.asm

2.2.4 VDSO

文件清单：
- kernel/src/arch/x86_64/vdso.rs
- kernel/src/arch/x86_64/vdso.asm（VDSO 符号实现）

2.3 同步原语

文件清单：
- kernel/src/sync/mod.rs
- kernel/src/sync/spinlock.rs
- kernel/src/sync/mutex.rs
- kernel/src/sync/rwlock.rs
- kernel/src/sync/semaphore.rs
- kernel/src/sync/condvar.rs
- kernel/src/sync/barrier.rs
- kernel/src/sync/event.rs
- kernel/src/sync/wait_queue.rs
- kernel/src/sync/atomic.rs

实现顺序：
1. spinlock.rs（基础，其他同步原语依赖它）
2. atomic.rs（原子操作封装）
3. wait_queue.rs（阻塞队列）
4. mutex.rs（基于 futex）
5. rwlock.rs、semaphore.rs、condvar.rs、barrier.rs、event.rs

2.4 库函数

文件清单：
- kernel/src/lib/mod.rs
- kernel/src/lib/print.rs
- kernel/src/lib/string.rs
- kernel/src/lib/hash.rs
- kernel/src/lib/crc.rs
- kernel/src/lib/bit.rs
- kernel/src/lib/result.rs
- kernel/src/lib/time.rs
- kernel/src/lib/debug.rs
- kernel/src/lib/collections/mod.rs
- kernel/src/lib/collections/ring_buffer.rs
- kernel/src/lib/collections/btree.rs
- kernel/src/lib/collections/radix_tree.rs
- kernel/src/lib/collections/bitmap.rs
- kernel/src/lib/collections/lru.rs
- kernel/src/lib/collections/lockfree/mod.rs
- kernel/src/lib/collections/lockfree/stack.rs
- kernel/src/lib/collections/lockfree/queue.rs
- kernel/src/lib/collections/lockfree/hashmap.rs

阶段目标：
内核能够初始化硬件、处理中断和异常

验收标准：
- [ ] 内核成功接收引导信息并打印启动 banner
- [ ] 串口输出正常工作（panic 信息可见）
- [ ] GDT 和 IDT 正确设置并加载
- [ ] CPU 特性检测完成（SSE、AVX、SMEP/SMAP 等）
- [ ] 能够处理所有 CPU 异常（#PF、#GP、#DF 等）
- [ ] Local APIC 和 I/O APIC 初始化成功
- [ ] 定时器中断正常触发（能够打印 tick 计数）
- [ ] 上下文切换汇编代码编写完成
- [ ] 所有基础同步原语（Spinlock、Mutex、WaitQueue）可用
- [ ] 基础库函数（打印、字符串、调试工具）可用

输出物：
- 能够响应中断和异常的内核
- 同步原语单元测试通过
- 中断处理测试通过

<br>



## 阶段 3: 内存管理

3.1 物理内存管理

文件清单：
- kernel/src/mm/mod.rs
- kernel/src/mm/pmm.rs
- kernel/src/mm/pmm/buddy.rs
- kernel/src/mm/pmm/zone.rs
- kernel/src/mm/pmm/frame.rs
- kernel/src/mm/pmm/numa.rs
- kernel/src/mm/pmm/watermark.rs
- kernel/src/mm/addr.rs

实现顺序：
1. frame.rs：页帧结构体
2. buddy.rs：伙伴系统算法
3. zone.rs：内存区域管理（DMA、Normal）
4. watermark.rs：页面水位标记
5. numa.rs：NUMA 支持（可选）
6. pmm.rs：物理内存分配器接口

3.2 虚拟内存管理

文件清单：
- kernel/src/mm/vmm.rs
- kernel/src/mm/vmm/page_table.rs（通用接口）
- kernel/src/mm/vmm/mapping.rs
- kernel/src/mm/vmm/layout.rs
- kernel/src/mm/vmm/vma.rs
- kernel/src/mm/vmm/cow.rs
- kernel/src/mm/vmm/swap.rs

实现顺序：
1. page_table.rs：页表操作通用接口（调用 arch/x86_64/memory.rs）
2. layout.rs：虚拟地址空间布局
3. vma.rs：VMA 描述符和管理
4. mapping.rs：地址映射、权限管理
5. cow.rs：写时复制
6. swap.rs：交换机制

3.3 堆分配器

文件清单：
- kernel/src/mm/heap.rs
- kernel/src/mm/heap/slub.rs
- kernel/src/mm/heap/kmalloc.rs
- kernel/src/mm/heap/slab_cache.rs

实现顺序：
1. slub.rs：SLUB 分配器核心
2. slab_cache.rs：Slab 缓存管理
3. kmalloc.rs：内核内存分配接口
4. heap.rs：实现 Rust GlobalAlloc trait

阶段目标：
内核能够管理物理和虚拟内存，支持动态分配

验收标准：
- [ ] 伙伴系统分配器正常工作（分配、释放、合并）
- [ ] 能够正确识别和管理内存区域（DMA、Normal）
- [ ] 4 级页表操作正确（映射、解映射、权限修改）
- [ ] VMA 管理正常（创建、查找、分割、合并）
- [ ] 能够处理页错误（按需分页）
- [ ] 写时复制（COW）机制工作正常
- [ ] SLUB 分配器正常工作
- [ ] 内核堆分配（kmalloc/kfree）可用
- [ ] Rust 的 Box、Vec 等标准容器可用
- [ ] 内存统计和监控接口可用

输出物：
- 内存管理单元测试全部通过
- 内存压力测试通过（大量分配/释放）
- 无内存泄漏（长时间运行验证）

<br>



## 阶段 4: 进程和线程管理

4.1 进程控制块和基础操作

文件清单：
- kernel/src/task/mod.rs
- kernel/src/task/pcb.rs
- kernel/src/task/thread.rs
- kernel/src/task/process.rs
- kernel/src/task/signal.rs
- kernel/src/task/resource.rs

实现顺序：
1. pcb.rs：PCB 结构体定义
2. thread.rs：线程数据结构
3. process.rs：fork、exec、exit、wait
4. signal.rs：信号处理机制
5. resource.rs：资源限制（rlimit）

4.2 调度器

文件清单：
- kernel/src/task/scheduler/mod.rs
- kernel/src/task/scheduler/cfs.rs
- kernel/src/task/scheduler/load_balance.rs
- kernel/src/task/scheduler/cpu_affinity.rs
- kernel/src/task/scheduler/preemption.rs

实现顺序：
1. cfs.rs：CFS 调度器核心（红黑树、vruntime）
2. preemption.rs：抢占调度
3. cpu_affinity.rs：CPU 亲和性
4. load_balance.rs：多核负载均衡

4.3 命名空间和 cgroup

文件清单：
- kernel/src/task/namespace.rs
- kernel/src/task/cgroup.rs

实现顺序：
1. namespace.rs：PID、Network、Mount、UTS、IPC、User 命名空间
2. cgroup.rs：cgroup v2 资源控制

阶段目标：
能够创建、调度和管理多个进程

验收标准：
- [ ] PCB 结构完整且正确初始化
- [ ] fork 系统调用正常工作（子进程正确创建）
- [ ] exec 系统调用能够加载和执行简单的静态链接 ELF
- [ ] exit 和 wait 系统调用正常工作
- [ ] CFS 调度器正常运行（多个进程公平分配 CPU）
- [ ] 上下文切换正确（寄存器状态正确保存/恢复）
- [ ] 进程优先级和 nice 值生效
- [ ] 信号发送和处理正常（SIGKILL、SIGTERM、SIGCHLD 等）
- [ ] 至少能够运行 3 个并发进程并正确切换
- [ ] 进程树正确维护（父子关系、孤儿进程回收）
- [ ] 资源限制（rlimit）生效

输出物：
- 能够运行简单用户态测试程序（打印 "Hello World"）
- 多进程并发测试通过
- fork/exec/exit 压力测试通过

<br>



## 阶段 5: 文件系统

5.1 VFS 抽象层

文件清单：
- kernel/src/fs/mod.rs
- kernel/src/fs/vfs.rs
- kernel/src/fs/file.rs
- kernel/src/fs/inode.rs
- kernel/src/fs/dentry.rs
- kernel/src/fs/path.rs
- kernel/src/fs/mount.rs
- kernel/src/fs/dcache.rs
- kernel/src/fs/pagecache.rs

实现顺序：
1. vfs.rs：FileSystem、Inode、File trait 定义
2. inode.rs：Inode 元数据和操作
3. dentry.rs：目录项结构
4. path.rs：路径解析
5. mount.rs：挂载点管理
6. dcache.rs：目录项缓存
7. pagecache.rs：页缓存

5.2 具体文件系统

文件清单：
- kernel/src/fs/filesystems/mod.rs
- kernel/src/fs/filesystems/tmpfs.rs
- kernel/src/fs/filesystems/devfs.rs
- kernel/src/fs/filesystems/procfs.rs
- kernel/src/fs/filesystems/sysfs.rs
- kernel/src/fs/filesystems/exfat.rs

实现顺序：
1. tmpfs.rs：内存文件系统（最简单，用于测试）
2. devfs.rs：设备文件系统
3. procfs.rs：/proc 文件系统
4. sysfs.rs：/sys 文件系统
5. exfat.rs：exFAT 文件系统（真实磁盘文件系统）

阶段目标：
能够挂载文件系统并进行文件操作

验收标准：
- [ ] VFS 抽象层正常工作
- [ ] tmpfs 挂载并可用（创建、读写、删除文件）
- [ ] devfs 挂载并可用（访问 /dev/null、/dev/zero 等）
- [ ] procfs 挂载并可用（读取 /proc/self/maps、/proc/cpuinfo 等）
- [ ] sysfs 挂载并可用
- [ ] exFAT 文件系统能够挂载真实磁盘分区
- [ ] 能够在 exFAT 上创建、读写、删除文件和目录
- [ ] 路径解析正确（绝对路径、相对路径、符号链接）
- [ ] 目录项缓存（dcache）正常工作
- [ ] 页缓存（page cache）正常工作
- [ ] 文件权限检查正确
- [ ] open、read、write、close、stat 系统调用正常

输出物：
- 文件系统测试套件通过
- 能够从 exFAT 分区读取和写入文件

<br>



## 阶段 6: 设备驱动

6.1 PCI 和 ACPI

文件清单：
- kernel/src/drivers/mod.rs
- kernel/src/drivers/pci/mod.rs
- kernel/src/drivers/pci/config.rs
- kernel/src/drivers/pci/device.rs
- kernel/src/drivers/pci/ecam.rs
- kernel/src/drivers/pci/dma.rs
- kernel/src/drivers/pci/iommu.rs
- kernel/src/drivers/acpi/mod.rs
- kernel/src/drivers/acpi/parse.rs
- kernel/src/drivers/acpi/tables.rs

6.2 基础设备驱动

文件清单：
- kernel/src/drivers/serial/mod.rs
- kernel/src/drivers/serial/uart16550.rs
- kernel/src/drivers/display/mod.rs
- kernel/src/drivers/display/fb.rs
- kernel/src/drivers/display/gop.rs
- kernel/src/drivers/display/vesafb.rs
- kernel/src/drivers/input/mod.rs
- kernel/src/drivers/input/hid.rs
- kernel/src/drivers/input/keyboard.rs
- kernel/src/drivers/input/mouse.rs

6.3 块设备驱动

文件清单：
- kernel/src/drivers/block/mod.rs
- kernel/src/drivers/block/nvme.rs
- kernel/src/drivers/block/ahci.rs
- kernel/src/drivers/block/virtio.rs
- kernel/src/drivers/block/io_scheduler.rs
- kernel/src/drivers/block/blk_queue.rs

6.4 网络设备驱动

文件清单：
- kernel/src/drivers/nic/mod.rs
- kernel/src/drivers/nic/driver.rs
- kernel/src/drivers/nic/e1000.rs
- kernel/src/drivers/nic/igc.rs
- kernel/src/drivers/nic/virtio.rs

阶段目标：
能够访问硬件设备（磁盘、网络、显示）

验收标准：
- [ ] PCI 设备枚举成功（列出所有 PCI 设备）
- [ ] ACPI 表解析正确（MADT、MCFG、FADT）
- [ ] 串口驱动正常工作（UART 16550）
- [ ] 帧缓冲显示驱动工作（能够绘制像素）
- [ ] 键盘驱动正常（能够接收键盘输入）
- [ ] 鼠标驱动正常（能够接收鼠标移动和点击）
- [ ] NVMe 驱动正常（能够读写 NVMe SSD）
- [ ] AHCI 驱动正常（能够读写 SATA 硬盘）
- [ ] VirtIO 块设备驱动正常
- [ ] e1000 网卡驱动初始化成功
- [ ] VirtIO 网卡驱动初始化成功
- [ ] 能够发送和接收原始以太网帧
- [ ] 块设备 I/O 调度器正常工作

输出物：
- 设备驱动测试通过
- 能够从物理磁盘读写数据
- 能够发送和接收网络数据包

<br>



## 阶段 7: 网络协议栈

7.1 链路层

文件清单：
- kernel/src/net/mod.rs
- kernel/src/net/link/mod.rs
- kernel/src/net/link/ethernet.rs
- kernel/src/net/link/arp.rs

7.2 IP 层

文件清单：
- kernel/src/net/ip/mod.rs
- kernel/src/net/ip/ipv4.rs
- kernel/src/net/ip/ipv6.rs
- kernel/src/net/ip/routing.rs
- kernel/src/net/ip/icmp.rs
- kernel/src/net/ip/icmpv6.rs
- kernel/src/net/ip/fragment.rs

7.3 传输层

文件清单：
- kernel/src/net/transport/mod.rs
- kernel/src/net/transport/socket.rs
- kernel/src/net/transport/udp.rs
- kernel/src/net/transport/tcp.rs

7.4 io_uring 异步 I/O

文件清单：
- kernel/src/net/io_uring.rs
- kernel/src/net/config.rs

阶段目标：
能够进行网络通信（IPv4/IPv6、TCP/UDP）

验收标准：
- [ ] 以太网帧正确封装和解析
- [ ] ARP 协议正常工作（地址解析）
- [ ] IPv4 协议正常工作（发送、接收、路由）
- [ ] IPv6 协议正常工作（发送、接收、自动配置）
- [ ] ICMPv4/v6 协议正常（能够 ping 通）
- [ ] 路由表正确维护和查询
- [ ] UDP 协议正常工作（能够发送和接收 UDP 数据包）
- [ ] TCP 协议正常工作（三次握手、数据传输、四次挥手）
- [ ] Socket 接口可用（socket、bind、listen、accept、connect、send、recv）
- [ ] 能够建立 TCP 连接并传输数据
- [ ] 能够作为服务器监听连接
- [ ] 网络配置接口可用（IP 地址、子网掩码、网关）

输出物：
- 网络协议栈单元测试通过
- 能够 ping 外部主机
- 能够建立 TCP 连接到外部服务器
- 能够运行简单的 HTTP 客户端/服务器

<br>



## 阶段 8: 系统调用

8.1 系统调用框架

文件清单：
- kernel/src/syscall/mod.rs
- kernel/src/syscall/dispatch.rs

8.2 各类系统调用

文件清单：
- kernel/src/syscall/fs.rs
- kernel/src/syscall/process.rs
- kernel/src/syscall/memory.rs
- kernel/src/syscall/signal.rs
- kernel/src/syscall/ipc.rs
- kernel/src/syscall/net.rs
- kernel/src/syscall/time.rs
- kernel/src/syscall/io_uring.rs
- kernel/src/syscall/cgroup.rs
- kernel/src/syscall/namespace.rs
- kernel/src/syscall/perf.rs
- kernel/src/syscall/misc.rs

阶段目标：
提供完整的系统调用接口，能够运行用户态程序

验收标准：
- [ ] 系统调用分发器正常工作
- [ ] 所有文件系统相关系统调用实现（open、read、write、close、stat、lseek、dup、pipe 等）
- [ ] 所有进程相关系统调用实现（fork、execve、exit、wait、getpid、kill 等）
- [ ] 所有内存相关系统调用实现（mmap、munmap、mprotect、brk 等）
- [ ] 所有信号相关系统调用实现（sigaction、sigprocmask、kill、sigreturn 等）
- [ ] 所有 IPC 相关系统调用实现（pipe、msgget、shmget、semget 等）
- [ ] 所有网络相关系统调用实现（socket、bind、listen、accept、connect、send、recv 等）
- [ ] 所有时间相关系统调用实现（gettimeofday、clock_gettime、nanosleep 等）
- [ ] io_uring 系统调用实现（setup、enter、register）
- [ ] cgroup 和 namespace 系统调用实现（unshare、setns、clone 等）
- [ ] 能够运行静态链接的自制 busybox
- [ ] 能够运行静态链接的自制 shell（zsh）

输出物：
- 系统调用测试套件全部通过
- 能够在自制 shell 中执行基本命令（ls、cat、echo 等）

<br>



## 阶段 9: Linux 兼容层

9.1 ELF 加载和动态链接

文件清单：
- kernel/src/compat/mod.rs
- kernel/src/compat/loader.rs
- kernel/src/compat/interpreter.rs
- kernel/src/compat/auxv.rs

9.2 兼容适配

文件清单：
- kernel/src/compat/syscall_translator.rs
- kernel/src/compat/signal_adapter.rs
- kernel/src/compat/procfs_emulation.rs（待定）
- kernel/src/compat/personality.rs

…… 不尽枚举，视情况自主决定

9.3 musl 兼容

把 musl 库移植到 AXIS 上，克隆其源码并改造、编译

阶段目标：
能够运行静态链接和动态链接 musl 的 Linux 二进制程序

验收标准：
- [ ] ELF 加载器支持 PT_INTERP（动态链接器）
- [ ] 能够加载 musl 动态链接器（/lib/ld-musl-<arch>.so.1）（注意，这里的 <arch> 视平台而定）
- [ ] 辅助向量（auxv）正确构建（AT_PHDR、AT_BASE、AT_ENTRY、AT_RANDOM、AT_SYSINFO_EHDR 等）
- [ ] VDSO 正确映射到用户空间
- [ ] VDSO 快速系统调用工作（gettimeofday、clock_gettime、getcpu）
- [ ] 用户栈正确初始化（argc、argv、envp、auxv）
- [ ] 能够加载和运行动态链接 musl 的 busybox
- [ ] 能够加载和运行动态链接 musl 的 bash/dash/zsh
- [ ] 能够运行需要共享库的程序（需要 libc.so）
- [ ] 系统调用号转换正确（如果需要）
- [ ] 信号处理适配正确（Linux 信号语义）
- [ ] Personality 系统调用正常工作
- [ ] 能够运行简单的 GNU coreutils 工具

输出物：
- 能够运行动态链接 musl 的 busybox 并执行所有内置命令
- 能够运行编译好的 Linux 应用程序（如 curl、wget）
- Linux 兼容性测试套件通过

<br>



## 阶段 10: 测试和优化

10.1 单元测试

- 为关键模块编写单元测试
- 内存管理测试
- 调度器测试
- 文件系统测试

10.2 集成测试

- 运行简单的用户态程序（静态链接）
- 运行动态链接程序（busybox、dash）
- 文件 I/O 测试
- 网络测试

10.3 性能优化

- 性能分析（perf）
- 热点优化
- 无锁数据结构优化

10.4 稳定性测试

- 压力测试
- 并发测试
- 内存泄漏检测
- Fuzzing 测试

阶段目标：
测试和优化完成，系统稳定

验收标准：
- [ ] 所有单元测试通过（覆盖率 > 80%）
- [ ] 所有集成测试通过
- [ ] 能够通过 LTP（Linux Test Project）核心测试集（> 90%）
- [ ] 内存泄漏检测通过（无泄漏）
- [ ] 死锁检测通过（无死锁）
- [ ] 并发压力测试通过（1000+ 进程同时运行）
- [ ] 文件系统压力测试通过（大量并发 I/O）
- [ ] 网络压力测试通过（高并发连接）
- [ ] 性能基准测试完成并达标：
  - [ ] 系统调用延迟 < 500ns
  - [ ] 上下文切换延迟 < 2μs
  - [ ] 进程创建延迟 < 100μs
  - [ ] TCP 吞吐量 > 1Gbps
  - [ ] 磁盘 I/O 吞吐量接近硬件上限
- [ ] 无锁数据结构优化完成
- [ ] 热点代码优化完成（基于 perf 分析）
- [ ] 文档完整（架构文档、开发文档、用户手册）
- [ ] 代码审查完成

输出物：
- 稳定版本发布镜像
- 完整文档集
- 示例应用程序和使用指南

<br>



## 阶段 11：内核可加载模块化改造

参考 [可加载模块化改造文档](loadable.md)
