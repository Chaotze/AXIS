# AXIS 架构设计文档

AXIS/
├── bootloader/                                 # 引导加载程序 (固件和内核之间的桥梁)
│   ├── bios/                                   # BIOS 传统引导模式
│   │   ├── Cargo.toml
│   │   ├── stage1.asm                          # 第一阶段引导 (MBR, 16 位实模式, 512 字节)
│   │   ├── stage2.asm                          # 第二阶段引导 (32 位保护模式启用)
│   │   ├── stage2.ld                           # Stage2 链接脚本及内存布局定义
│   │   └── src/
│   │       └── lib.rs                          # Stage2 Rust 实现 (ELF 解析、内核加载)
│   │
│   └── uefi/                                   # UEFI 现代引导模式 (UEFI 固件下运行)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                         # UEFI 主入口
│           ├── graphics.rs                     # UEFI 图形初始化
│           └── memory.rs                       # UEFI 内存映射获取
│
├── kernel/                                     # 内核主程序
│   ├── Cargo.toml
│   ├── kernel.ld                               # 内核链接脚本 (内存布局：低端引导桩 + 高半核主体)
│   ├── build.rs                                # 构建脚本 (NASM 汇编、链接参数注入)
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
│       │       ├── boot.asm                    # 内核启动代码 (建立高半核映射、绝对跳转高半区、取消恒等映射)
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
│       │   ┌——— mm 子系统实现说明（与设计的一致性修订）———┐
│       │   │ 文件结构与原设计一致（pmm.rs 为主模块文件 + pmm/ 子目录，
│       │   │ vmm.rs / heap.rs 同理）。实现中按现实约束做了如下落地：
│       │   │
│       │   │ 1. 分层：把「纯算法」与「内核胶水」拆开。buddy / zone /
│       │   │    watermark / frame / numa / vma / slub / slab_cache /
│       │   │    kmalloc / swap 均为纯逻辑模块（不引用 arch 与全局锁、
│       │   │    自身不依赖堆），可被宿主单元测试（unitest crate）直接
│       │   │    编译运行；pmm.rs / vmm.rs / heap.rs / page_table.rs /
│       │   │    mapping.rs / cow.rs 为装配层，承担全局状态、锁与
│       │   │    arch 对接。
│       │   │ 2. 伙伴系统元数据（段树空闲计数 + 双向空闲链）从被管理
│       │   │    物理内存顶部划分（Linux bootmem 式），使 PMM 在堆就绪
│       │   │    前自举：分配器不依赖分配。元数据访问必须经物理内存
│       │   │    映射区（PHYSICAL_MEMORY_OFFSET）——boot.asm 已删除
│       │   │    低端恒等映射，物理地址不可直接解引用。
│       │   │ 3. 堆对象驻留物理直接映射区：kmalloc 返回 phys + OFFSET
│       │   │    的虚拟地址，无需为堆另建映射；KERNEL_HEAP_START 保留
│       │   │    为将来独立堆映射的 VA 规划。SLUB 的 slab 页头与对象
│       │   │    空闲链共置页内（分配器自举），kfree 以页头魔数+
│       │   │    cache_id 定位缓存；kmalloc 为 9 级 2 的幂尺寸桶 + 大
│       │   │    对象直接连续页。
│       │   │ 4. COW 采用页表软件位（PageTableFlags::COW = bit9）；
│       │   │    为让「只读共享页」在核心态写同样触发缺页，cpu.rs 已
│       │   │    启用 CR0.WP。
│       │   │ 5. swap 先以 MemorySwapStore（内存页模拟磁盘）走通全流程，
│       │   │    接口面向 SwapStore trait，块设备就绪后替换实现即可。
│       │   │ 6. layout.rs 以 config.rs 为地址常量的唯一事实来源（重新
│       │   │    导出 + 补充用户空间布局），避免常量两处定义漂移。
│       │   │ 7. NUMA 当前为单节点 UMA 回退（结构就位，待 ACPI SRAT
│       │   │    接入后填充节点与亲和关系）。
│       │   │ 8. 测试：unitest（工作区成员，编译内核源文件在宿主环境跑
│       │   │    单元/压力测试）+ 内核启动自测 mm::selftest（QEMU 内
│       │   │    验证按需分页、COW 拆解、交换换入等真实硬件路径）。
│       │   └—— 锁序约定：VMM → PMM → HEAP ——————————┘
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
│       │   ┌——— task 子系统实现说明（与设计的一致性修订）———┐
│       │   │ 文件结构与原设计一致（pcb/thread/process/signal/
│       │   │ resource + scheduler/{cfs,preemption,cpu_affinity,
│       │   │ load_balance} + namespace/cgroup）。按现实约束
│       │   │ 落地修订如下：
│       │   │
│       │   │ 1. 分层（同 mm）：纯算法模块（除 task/mod.rs 外全部）
│       │   │    不引用 arch/全局锁，经 unitest 宿主测试；mod.rs
│       │   │    为装配层（全局任务表 + 调度器 + 启动自测）。
│       │   │ 2. 就绪队列复用 lib 的 BTreeMap（vruntime, tid 二元组
│       │   │    键保证唯一），替代原设计的红黑树：职责等价且
│       │   │    已过完整测试，避免重复造轮；树高同阶，接口
│       │   │    （first 取最小键）已补齐。若 profile 显示必要，
│       │   │    仅替换内部容器即可。
│       │   │ 3. 定长任务表：pid = 槽位下标，空闲位图复用
│       │   │    lib::Bitmap；Rust 稳定版不支持 const 泛型算术，
│       │   │    槽位/位图字数/节点池以独立 const 参数传入，
│       │   │    编译期断言约束（如 K_MAX 奇数、C_MAX=K_MAX+1、
│       │   │    MAX_NODES ≥ 2×MAX_TASKS+8）。
│       │   │ 4. 全局任务态用 Spinlock<Option<Box<TaskState>>>：
│       │   │    TaskState 约 150KB，Box 堆分配避免引导栈大块
│       │   │    拷贝；引导栈由 64KB 增至 256KB（mm 自测深层
│       │   │    调用链 + 大静态布局曾溢出 64KB）。
│       │   │ 5. 真实上下文切换（本轮新接线）：定时器中断内
│       │   │    换栈 + iretq 式抢占切换（见 interrupt/entry.asm
│       │   │    与 task::tick_hook）；任务首次运行帧由
│       │   │    context/frame.rs 的 SwitchFrame 构造（布局与
│       │   │    中断存根保存序列逐字节镜像）；switch.asm 的
│       │   │    协作式切换保留供主动让出路径。init 与 3 个
│       │   │    演示内核线程（nice 5/0/-5）在 QEMU 实测并发
│       │   │    运行、输出比例与权重一致。
│       │   │ 6. 中断纪律（自旋锁 + 抢占共存的 irqsave）：
│       │   │    任务态访问 TASK/堆/PMM 锁与打印（WRITER）均
│       │   │    屏蔽中断；否则任务持锁时被抢占、中断路径等
│       │   │    同一把锁即死锁（初始化持锁期被首个 tick 打断
│       │   │    是实测首坑）。vruntime 记账用 1024 定标，
│       │   │    避免高权重任务每 tick 增量为 0 而饿死其余。
│       │   │ 7. 测试：unitest 宿主测试（task 组 11 个纯算法文件
│       │   │    + context/frame）+ 内核启动自测 task::selftest
│       │   │    （9 项）在 QEMU 验证；多任务并发与公平性由
│       │   │    演示线程的屏幕输出实证。
│       │   └—— 锁序约定：TASK → PMM → KHEAP 单向，均 irqsave —┘
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
│       │   ├── vga.rs                          # VGA 文本模式底层支持 (共享写入器，供 print/panic 复用)
│       │   ├── print.rs                        # 打印和日志函数 (基于 vga.rs 加锁输出)
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
├── .gitignore                                  # Git 忽略文件列表
├── axis.ps1                                    # PowerShell 构建脚本
├── axis.sh                                     # Shell 构建脚本
├── Cargo.lock                                  # 依赖版本锁定文件
├── Cargo.toml                                  # 项目清单文件 (依赖、元数据)
├── LICENCE                                     # 项目项目许可证文件
├── Makefile                                    # 构建系统主入口 (编译、运行、清理)
├── dev/                                        # 开发者文件夹
│   ├── arch.md                                 # 架构设计文档
│   ├── dev.md                                  # 开发者文档
│   ├── loadable.md                             # 可加载模块化改造文档
│   └── roadmap.md                              # 路线落地计划文档
├── README.md                                   # 项目概览 (英文)
├── README.zh-CN.md                             # 项目概览 (中文)
└── x86_64-unknown-axis.json                    # 自定义 target 定义 (x86-64 编译目标)
