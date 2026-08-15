# AXIS 架构设计文档

AXIS/
├── bootloader/                                 # 引导加载程序 (固件和内核之间的桥梁)
│   ├── bios/                                   # BIOS 传统引导模式
│   │   ├── Cargo.toml
│   │   ├── stage1.asm                          # 第一阶段引导 (MBR, 16 位实模式, 512 字节)
│   │   ├── stage2.asm                          # 第二阶段引导 (32 位保护模式启用)
│   │   ├── stage2.ld                           # Stage2 链接脚本
│   │   └── src/
│   │       └── lib.rs                          # Stage2 Rust 实现 (ELF 解析、内核加载)
│   │
│   ├── uefi/                                   # UEFI 现代引导模式 (UEFI 固件下运行)
│   │   ├── Cargo.toml
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
