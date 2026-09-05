// ============================================================
// procfs - proc 文件系统
// ============================================================
// 提供对进程和系统信息的访问（/proc/cpuinfo、/proc/meminfo 等）
// 支持 PID 目录、/proc/self 等高级功能
//
// 架构设计：
// - 根目录：系统级文件（cpuinfo、meminfo、version 等）+ PID 目录
// - /proc/[pid]：进程级文件（cmdline、status、stat 等）
// - /proc/self：符号链接，指向当前进程 PID 目录
// - 所有文件都是动态生成的，无持久化存储
//
// 为什么分离虚拟文件生成：
// - 不同文件需要访问不同的内核子系统（mm、task 等）
// - 生成逻辑与 VFS 操作解耦，便于单元测试
// - 支持实时数据读取而非静态快照

#![allow(dead_code)]

use crate::fs::vfs::{
    FileSystem, DirectoryEntry, FileMode, FileType,
    InodeMetadata, InodeNumber
};
use crate::lib::result::KernelResult;
use crate::lib::time::current_timestamp_secs;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ============================================================
// 虚拟文件类型定义
// ============================================================

/// procfs 中的系统级虚拟文件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcSystemFileType {
    CpuInfo,    // CPU 信息
    MemInfo,    // 内存信息
    Uptime,     // 系统运行时间
    Version,    // 内核版本
    Cmdline,    // 内核命令行
    Filesystems,// 支持的文件系统
    Stat,       // 系统全局 CPU/进程统计
}

/// procfs 中的进程级虚拟文件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcProcessFileType {
    Cmdline,    // 进程命令行
    Status,     // 进程状态信息
    Stat,       // 进程 CPU/调度统计
    Exe,        // 指向进程可执行文件的符号链接
}

/// procfs 节点类型
#[derive(Debug, Clone)]
enum ProcNodeKind {
    /// 根目录
    Root,
    /// 系统级虚拟文件
    SystemFile(ProcSystemFileType),
    /// 进程目录（包含 PID）
    ProcessDir(u32),
    /// 进程级虚拟文件
    ProcessFile(u32, ProcProcessFileType),
    /// /proc/self 符号链接
    SelfLink,
}

/// procfs 文件节点
struct ProcfsNode {
    /// inode 号
    inode_number: InodeNumber,
    /// 节点类型（决定文件类型和内容）
    kind: ProcNodeKind,
    /// 子项（仅目录）
    children: Vec<(Vec<u8>, InodeNumber)>,
    /// 元数据
    metadata: InodeMetadata,
}

impl ProcfsNode {
    /// 创建根目录节点
    fn new_root(inode_number: InodeNumber) -> Self {
        let now = current_timestamp_secs();
        ProcfsNode {
            inode_number,
            kind: ProcNodeKind::Root,
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::Directory,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o555),
                uid: 0,
                gid: 0,
                nlink: 2,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 创建系统级虚拟文件节点
    fn new_system_file(inode_number: InodeNumber, file_type: ProcSystemFileType) -> Self {
        let now = current_timestamp_secs();
        ProcfsNode {
            inode_number,
            kind: ProcNodeKind::SystemFile(file_type),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::File,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o444),
                uid: 0,
                gid: 0,
                nlink: 1,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 创建进程目录节点
    fn new_process_dir(inode_number: InodeNumber, pid: u32) -> Self {
        let now = current_timestamp_secs();
        ProcfsNode {
            inode_number,
            kind: ProcNodeKind::ProcessDir(pid),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::Directory,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o555),
                uid: 0,
                gid: 0,
                nlink: 2,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 创建进程级虚拟文件节点
    fn new_process_file(inode_number: InodeNumber, pid: u32, file_type: ProcProcessFileType) -> Self {
        let now = current_timestamp_secs();
        ProcfsNode {
            inode_number,
            kind: ProcNodeKind::ProcessFile(pid, file_type),
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::File,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o444),
                uid: 0,
                gid: 0,
                nlink: 1,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    /// 创建 /proc/self 符号链接
    fn new_self_link(inode_number: InodeNumber) -> Self {
        let now = current_timestamp_secs();
        ProcfsNode {
            inode_number,
            kind: ProcNodeKind::SelfLink,
            children: Vec::new(),
            metadata: InodeMetadata {
                inode_number,
                file_type: FileType::Symlink,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o777),
                uid: 0,
                gid: 0,
                nlink: 1,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            },
        }
    }

    fn is_dir(&self) -> bool {
        self.metadata.file_type == FileType::Directory
    }
}

// ============================================================
// 虚拟文件内容生成器
// ============================================================

/// 为什么分离成独立模块：
/// - 文件内容生成逻辑与 VFS 操作解耦
/// - 不同系统级别可能需要访问不同的内核子系统
/// - 便于单元测试文件内容生成
struct ProcContentGenerator;

impl ProcContentGenerator {
    /// 生成 /proc/cpuinfo
    ///
    /// 从 arch 层读取真实 CPU 信息（品牌字符串、特性位）
    fn generate_cpuinfo() -> Vec<u8> {
        let mut content = String::new();

        // 获取 CPU 品牌字符串
        let brand = crate::arch::x86_64::cpu::get_brand_string();
        let brand_str = core::str::from_utf8(&brand)
            .unwrap_or("Unknown CPU")
            .trim_end_matches('\0')
            .trim();

        // 获取 CPU 特性和功能级别
        let features = crate::arch::x86_64::cpu::get_features();
        let cpuid_level = crate::arch::x86_64::cpu::get_cpuid_level();

        content.push_str("processor\t: 0\n");
        content.push_str("model name\t: ");
        content.push_str(brand_str);
        content.push_str("\n");
        content.push_str("cpuid level\t: ");
        content.push_str(&cpuid_level.to_string());
        content.push_str("\n");
        content.push_str("fpu\t\t: yes\n");
        content.push_str("fpu_exception\t: yes\n");

        content.push_str("flags\t\t:");

        const SSE: u32 = 1 << 0;
        const SSE2: u32 = 1 << 1;
        const SSE3: u32 = 1 << 2;
        const SSSE3: u32 = 1 << 3;
        const SSE4_1: u32 = 1 << 4;
        const SSE4_2: u32 = 1 << 5;
        const AVX: u32 = 1 << 6;
        const AVX2: u32 = 1 << 7;
        const SMEP: u32 = 1 << 8;
        const SMAP: u32 = 1 << 9;
        const XSAVE: u32 = 1 << 10;

        if features & SSE != 0 { content.push_str(" sse"); }
        if features & SSE2 != 0 { content.push_str(" sse2"); }
        if features & SSE3 != 0 { content.push_str(" pni"); }
        if features & SSSE3 != 0 { content.push_str(" ssse3"); }
        if features & SSE4_1 != 0 { content.push_str(" sse4_1"); }
        if features & SSE4_2 != 0 { content.push_str(" sse4_2"); }
        if features & AVX != 0 { content.push_str(" avx"); }
        if features & AVX2 != 0 { content.push_str(" avx2"); }
        if features & XSAVE != 0 { content.push_str(" xsave"); }
        if features & SMEP != 0 { content.push_str(" smep"); }
        if features & SMAP != 0 { content.push_str(" smap"); }

        // 添加常见的基础标志（基于 CPUID.01H）
        content.push_str(" fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 cflush");
        content.push_str(" dts acpi mmx fxsr syscall lahf_lm");
        content.push_str("\n");

        content.into_bytes()
    }

    /// 生成 /proc/meminfo
    ///
    /// 从 mm 系统读取真实内存统计信息
    fn generate_meminfo() -> Vec<u8> {
        let stats = crate::mm::stats();
        let page_size = crate::config::PAGE_SIZE as u64;

        // 转换为 kB
        let total_kb = (stats.total_pages as u64 * page_size) / 1024;
        let free_kb = (stats.free_pages as u64 * page_size) / 1024;
        let _used_kb = (stats.used_pages as u64 * page_size) / 1024;
        let heap_kb = (stats.heap_bytes as u64) / 1024;

        let mut content = String::new();
        content.push_str("MemTotal:\t");
        content.push_str(&total_kb.to_string());
        content.push_str(" kB\n");
        content.push_str("MemFree:\t");
        content.push_str(&free_kb.to_string());
        content.push_str(" kB\n");
        content.push_str("MemAvailable:\t");
        content.push_str(&free_kb.to_string());
        content.push_str(" kB\n");
        content.push_str("Buffers:\t0 kB\n");
        content.push_str("Cached:\t\t");
        content.push_str(&heap_kb.to_string());
        content.push_str(" kB\n");
        content.push_str("HeapObjects:\t");
        content.push_str(&stats.heap_objects.to_string());
        content.push_str("\n");
        content.push_str("PageFaults:\t");
        content.push_str(&stats.page_faults.to_string());
        content.push_str("\n");
        content.into_bytes()
    }

    /// 生成 /proc/version
    ///
    /// 从 config.rs 读取版本信息，而非硬编码
    fn generate_version() -> Vec<u8> {
        let mut content = String::new();
        content.push_str(crate::config::KERNEL_NAME);
        content.push_str(" version ");
        content.push_str(crate::config::KERNEL_VERSION);
        content.push_str(" (");
        content.push_str(crate::config::KERNEL_AUTHOR);
        content.push_str(") - ");
        content.push_str(crate::config::KERNEL_SLOGAN);
        content.push('\n');
        content.into_bytes()
    }

    /// 生成 /proc/uptime
    ///
    /// 返回系统运行时间和 CPU 空闲时间（单位秒，浮点格式）
    /// TODO: 空闲时间需从任务调度器获取真实 idle 时间统计
    fn generate_uptime() -> Vec<u8> {
        let uptime_secs = crate::lib::time::uptime_seconds();
        let uptime_ms = crate::lib::time::uptime_ms();
        let ms = uptime_ms % 1000;

        let mut content = String::new();
        content.push_str(&uptime_secs.to_string());
        content.push_str(".");
        content.push_str(&format!("{:03}", ms));
        content.push_str(" ");
        content.push_str(&uptime_secs.to_string());
        content.push_str(".");
        content.push_str(&format!("{:03}", ms));
        content.push_str("\n");
        content.into_bytes()
    }

    /// 生成 /proc/cmdline
    fn generate_cmdline() -> Vec<u8> {
        // TODO: 从内核启动参数获取
        b"axis kernel\0".to_vec()
    }

    /// 生成 /proc/filesystems
    ///
    /// 列出当前编译进内核的所有支持的文件系统
    fn generate_filesystems() -> Vec<u8> {
        // TODO: 从 VFS 获取已注册的文件系统列表
        let mut content = String::new();
        // 虚拟文件系统（无需块设备）
        content.push_str("nodev\tdevfs\n");
        content.push_str("nodev\tprocfs\n");
        content.push_str("nodev\tsysfs\n");
        content.push_str("nodev\ttmpfs\n");
        // 块设备文件系统
        content.push_str("\texfat\n");
        content.into_bytes()
    }

    /// 生成 /proc/stat
    ///
    /// 返回系统级CPU和进程统计
    /// 格式：cpu时间 cpu[n]时间 intr ctxt btime processes procs_running procs_blocked
    fn generate_stat() -> Vec<u8> {
        let stats = crate::mm::stats();
        let mut content = String::new();

        // CPU 统计（暂为演示值，等待 CFS 调度器提供真实统计）
        content.push_str("cpu  0 0 0 0 0 0 0 0 0 0\n");
        content.push_str("cpu0 0 0 0 0 0 0 0 0 0 0\n");

        // 中断计数（TODO：从中断处理器获取）
        content.push_str("intr 0\n");

        // 上下文切换计数（TODO：从任务调度器获取）
        content.push_str("ctxt 0\n");

        // 系统启动时间戳（从当前时间反算）
        content.push_str("btime ");
        content.push_str(&(crate::lib::time::current_timestamp_secs() - crate::lib::time::uptime_seconds() as i64).to_string());
        content.push_str("\n");

        // 进程统计
        content.push_str("processes 1\n");
        content.push_str("procs_running 1\n");
        content.push_str("procs_blocked 0\n");

        // 页面错误统计（从内存管理器获取真实数据）
        content.push_str("page_faults ");
        content.push_str(&stats.page_faults.to_string());
        content.push_str("\n");

        content.into_bytes()
    }

    /// 生成进程的 /proc/[pid]/cmdline
    fn generate_process_cmdline(pid: u32) -> Vec<u8> {
        // TODO: 从进程控制块获取
        let mut content = String::new();
        content.push_str("task");
        content.push_str(&pid.to_string());
        content.push('\0');
        content.into_bytes()
    }

    /// 生成进程的 /proc/[pid]/stat
    ///
    /// 返回进程的调度统计信息
    fn generate_process_stat(pid: u32) -> Vec<u8> {
        let mut content = String::new();

        // 从任务系统获取进程信息
        if let Some(info) = crate::task::get_process_info(pid) {
            // pid 和名称
            content.push_str(&pid.to_string());
            content.push_str(" (");
            if info.name_len > 0 {
                if let Ok(name) = core::str::from_utf8(&info.name[..info.name_len]) {
                    content.push_str(name);
                }
            }
            content.push_str(") ");

            // 进程状态：Running -> R, Blocked -> D, Zombie -> Z
            let state_char = match info.state {
                crate::task::pcb::ProcessState::Running => 'R',
                crate::task::pcb::ProcessState::Blocked => 'D',
                crate::task::pcb::ProcessState::Zombie => 'Z',
            };
            content.push(state_char);
            content.push_str(" ");

            // ppid 和其他基本字段
            content.push_str(&info.ppid.to_string());
            content.push_str(" 0 0 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 ");

            // priority（=20+nice）
            let priority = 20i32 + (info.nice as i32);
            content.push_str(&priority.to_string());
            content.push_str(" ");

            // nice 值
            content.push_str(&info.nice.to_string());
            content.push_str(" 1 0 ");

            // starttime
            content.push_str(&info.start_tick.to_string());
            content.push_str(" 0 0 0\n");
        } else {
            // 进程不存在：返回占位符
            content.push_str(&format!("{} (unknown) S 1 0 0 0 -1 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 0\n", pid));
        }

        content.into_bytes()
    }

    /// 生成进程的 /proc/[pid]/status
    ///
    /// 返回进程的详细状态信息
    fn generate_process_status(pid: u32) -> Vec<u8> {
        let mut content = String::new();

        if let Some(info) = crate::task::get_process_info(pid) {
            // 进程名称
            content.push_str("Name:\t");
            if info.name_len > 0 {
                if let Ok(name) = core::str::from_utf8(&info.name[..info.name_len]) {
                    content.push_str(name);
                }
            }
            content.push_str("\n");

            // 进程号
            content.push_str("Pid:\t");
            content.push_str(&pid.to_string());
            content.push_str("\n");

            // 父进程号
            content.push_str("PPid:\t");
            content.push_str(&info.ppid.to_string());
            content.push_str("\n");

            // 状态
            let state_str = match info.state {
                crate::task::pcb::ProcessState::Running => "R (running)",
                crate::task::pcb::ProcessState::Blocked => "S (sleeping)",
                crate::task::pcb::ProcessState::Zombie => "Z (zombie)",
            };
            content.push_str("State:\t");
            content.push_str(state_str);
            content.push_str("\n");

            // Nice 值
            content.push_str("Nice:\t");
            content.push_str(&info.nice.to_string());
            content.push_str("\n");

            // Vruntime（虚拟运行时间）
            content.push_str("VRuntime:\t");
            content.push_str(&info.vruntime.to_string());
            content.push_str("\n");

            // 虚拟内存（占位符）
            content.push_str("VmSize:\t1024 kB\n");
            content.push_str("VmRSS:\t512 kB\n");
        } else {
            // 进程不存在
            content.push_str("Name:\tunknown\n");
            content.push_str("Pid:\t");
            content.push_str(&pid.to_string());
            content.push_str("\n");
            content.push_str("State:\tZ (zombie)\n");
        }

        content.into_bytes()
    }
}

// ============================================================
// procfs 文件系统实现
// ============================================================

/// procfs 文件系统
pub struct Procfs {
    /// 所有节点
    nodes: Vec<Option<ProcfsNode>>,
    /// 下一个可用的 inode 号
    next_ino: InodeNumber,
}

impl Procfs {
    /// 创建新的 procfs 实例
    pub fn new() -> KernelResult<Self> {
        let mut procfs = Procfs {
            nodes: Vec::new(),
            next_ino: 2,
        };

        // 预留 inode 0/1
        procfs.nodes.push(None);
        procfs.nodes.push(None);

        // 创建根目录（inode 2）
        procfs.nodes.push(Some(ProcfsNode::new_root(2)));

        // 系统级文件
        let sys_files = vec![
            (b"cpuinfo".to_vec(), ProcSystemFileType::CpuInfo, 3),
            (b"meminfo".to_vec(), ProcSystemFileType::MemInfo, 4),
            (b"uptime".to_vec(), ProcSystemFileType::Uptime, 5),
            (b"version".to_vec(), ProcSystemFileType::Version, 6),
            (b"cmdline".to_vec(), ProcSystemFileType::Cmdline, 7),
            (b"filesystems".to_vec(), ProcSystemFileType::Filesystems, 8),
            (b"stat".to_vec(), ProcSystemFileType::Stat, 9),
        ];

        procfs.next_ino = 10;

        // 初始化系统级虚拟文件
        for (name, file_type, ino) in sys_files {
            procfs.nodes.push(Some(ProcfsNode::new_system_file(ino, file_type)));
            if let Some(root) = procfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
                root.children.push((name, ino));
            }
        }

        // /proc/self 符号链接
        let self_ino = procfs.next_ino;
        procfs.next_ino += 1;
        procfs.nodes.push(Some(ProcfsNode::new_self_link(self_ino)));
        if let Some(root) = procfs.nodes.get_mut(2).and_then(|n| n.as_mut()) {
            root.children.push((b"self".to_vec(), self_ino));
        }

        // 为演示创建 /proc/1 (init) 目录
        procfs.create_process_dir_internal(&mut 1)?;

        Ok(procfs)
    }

    /// 内部辅助：创建进程目录及其子文件
    fn create_process_dir_internal(&mut self, pid: &mut u32) -> KernelResult<InodeNumber> {
        let pid_val = *pid;
        let dir_ino = self.next_ino;
        self.next_ino += 1;

        // 创建进程目录
        self.nodes.push(Some(ProcfsNode::new_process_dir(dir_ino, pid_val)));

        // 创建进程内的虚拟文件
        let process_files = vec![
            (b"cmdline".to_vec(), ProcProcessFileType::Cmdline),
            (b"status".to_vec(), ProcProcessFileType::Status),
            (b"stat".to_vec(), ProcProcessFileType::Stat),
            (b"exe".to_vec(), ProcProcessFileType::Exe),
        ];

        for (name, file_type) in process_files {
            let file_ino = self.next_ino;
            self.next_ino += 1;
            self.nodes.push(Some(ProcfsNode::new_process_file(file_ino, pid_val, file_type)));

            if let Some(proc_dir) = self.nodes.get_mut(dir_ino as usize).and_then(|n| n.as_mut()) {
                proc_dir.children.push((name, file_ino));
            }
        }

        // 将进程目录添加到根目录
        if let Some(root) = self.nodes.get_mut(2).and_then(|n| n.as_mut()) {
            root.children.push((pid_val.to_string().into_bytes(), dir_ino));
        }

        Ok(dir_ino)
    }

    /// 获取节点
    fn get_node(&self, ino: InodeNumber) -> Option<&ProcfsNode> {
        self.nodes.get(ino as usize)?.as_ref()
    }

    /// 在目录中查找子项
    ///
    /// 支持：
    /// - 静态系统文件（cpuinfo、meminfo 等）
    /// - 动态 PID 目录（从进程名解析 PID 并检查进程是否存在）
    ///
    /// 虚拟 inode 编码（避免与真实 inode 冲突）：
    /// - 范围：[0x1_0000_0000, 0xFFFF_FFFF_FFFF_FFFF]
    /// - PID 目录：0x1_0000_0000 | pid（低 32 位存 pid）
    /// - PID 文件：0x2_0000_0000 | (pid << 8) | file_type_idx
    fn find_child(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
        let parent = self.get_node(parent_ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !parent.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 先检查静态子项
        for (child_name, child_ino) in &parent.children {
            if child_name == name {
                return Ok(*child_ino);
            }
        }

        // 如果在根目录中查找，检查是否为数字（PID 目录）
        if parent_ino == 2 {
            // 尝试将 name 解析为十进制数字
            if let Ok(name_str) = core::str::from_utf8(name) {
                if let Ok(pid) = name_str.parse::<u32>() {
                    // 检查该进程是否存在（从真实任务表查询）
                    if let Some(_info) = crate::task::get_process_info(pid) {
                        // 返回虚拟 inode：0x1_0000_0000 | pid
                        return Ok(0x1_0000_0000 | (pid as u64));
                    }
                }
            }
        } else if parent_ino >= 0x1_0000_0000 && parent_ino < 0x2_0000_0000 {
            // 在虚拟 PID 目录中查找进程文件
            let pid = (parent_ino & 0xFFFF_FFFF) as u32;
            if crate::task::get_process_info(pid).is_some() {
                // 检查是否为进程文件名（cmdline、stat、status、exe）
                let file_type_idx = match name {
                    b"cmdline" => 0u8,
                    b"status" => 1u8,
                    b"stat" => 2u8,
                    b"exe" => 3u8,
                    _ => return Err(crate::lib::result::KernelError::InvalidArgument),
                };
                // 返回虚拟文件 inode：0x2_0000_0000 | (pid << 8) | file_type_idx
                return Ok(0x2_0000_0000 | ((pid as u64) << 8) | (file_type_idx as u64));
            }
        }

        Err(crate::lib::result::KernelError::InvalidArgument)
    }
}

impl FileSystem for Procfs {
    fn name(&self) -> &'static str {
        "procfs"
    }

    fn mount(&self) -> KernelResult<InodeNumber> {
        Ok(2)
    }

    fn unmount(&self) -> KernelResult<()> {
        Ok(())
    }

    fn lookup(&self, parent_ino: InodeNumber, name: &[u8]) -> KernelResult<InodeNumber> {
        self.find_child(parent_ino, name)
    }

    fn root_inode(&self) -> InodeNumber {
        2
    }

    fn stat(&self, ino: InodeNumber) -> KernelResult<InodeMetadata> {
        // 检查是否为虚拟 inode
        if ino >= 0x1_0000_0000 && ino < 0x2_0000_0000 {
            // 虚拟 PID 目录
            let now = current_timestamp_secs();
            return Ok(InodeMetadata {
                inode_number: ino,
                file_type: FileType::Directory,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o555),
                uid: 0,
                gid: 0,
                nlink: 2,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            });
        }

        if ino >= 0x2_0000_0000 && ino < 0x3_0000_0000 {
            // 虚拟 PID 文件
            let now = current_timestamp_secs();
            return Ok(InodeMetadata {
                inode_number: ino,
                file_type: FileType::File,
                size: 0,
                blocks: 0,
                mode: FileMode::new(0o444),
                uid: 0,
                gid: 0,
                nlink: 1,
                atime: now,
                mtime: now,
                ctime: now,
                btime: Some(now),
            });
        }

        // 静态 inode
        self.get_node(ino)
            .map(|node| node.metadata)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)
    }

    fn read(&self, ino: InodeNumber, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
        // 处理虚拟 PID 目录
        if ino >= 0x1_0000_0000 && ino < 0x2_0000_0000 {
            // 虚拟 PID 目录不能读取
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 处理虚拟 PID 文件
        if ino >= 0x2_0000_0000 && ino < 0x3_0000_0000 {
            // 解码 inode：0x2_0000_0000 | (pid << 8) | file_type_idx
            let pid = ((ino >> 8) & 0xFF_FFFF) as u32;
            let file_type_idx = (ino & 0xFF) as u8;

            let content = match file_type_idx {
                0 => ProcContentGenerator::generate_process_cmdline(pid),
                1 => ProcContentGenerator::generate_process_status(pid),
                2 => ProcContentGenerator::generate_process_stat(pid),
                3 => b"/sbin/init\0".to_vec(),  // exe 符号链接目标
                _ => return Err(crate::lib::result::KernelError::InvalidArgument),
            };

            let offset = offset as usize;
            if offset >= content.len() {
                return Ok(0);
            }

            let to_read = core::cmp::min(buf.len(), content.len() - offset);
            buf[..to_read].copy_from_slice(&content[offset..offset + to_read]);
            return Ok(to_read);
        }

        // 静态 inode
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        // 动态生成文件内容
        let content = match &node.kind {
            ProcNodeKind::SystemFile(file_type) => match file_type {
                ProcSystemFileType::CpuInfo => ProcContentGenerator::generate_cpuinfo(),
                ProcSystemFileType::MemInfo => ProcContentGenerator::generate_meminfo(),
                ProcSystemFileType::Uptime => ProcContentGenerator::generate_uptime(),
                ProcSystemFileType::Version => ProcContentGenerator::generate_version(),
                ProcSystemFileType::Cmdline => ProcContentGenerator::generate_cmdline(),
                ProcSystemFileType::Filesystems => ProcContentGenerator::generate_filesystems(),
                ProcSystemFileType::Stat => ProcContentGenerator::generate_stat(),
            },
            ProcNodeKind::ProcessFile(pid, file_type) => match file_type {
                ProcProcessFileType::Cmdline => ProcContentGenerator::generate_process_cmdline(*pid),
                ProcProcessFileType::Status => ProcContentGenerator::generate_process_status(*pid),
                ProcProcessFileType::Stat => ProcContentGenerator::generate_process_stat(*pid),
                ProcProcessFileType::Exe => b"/sbin/init\0".to_vec(),  // 占位符
            },
            ProcNodeKind::SelfLink => {
                // TODO: /proc/self 的目标是 /proc/1（当前还是演示，应为实际 PID）
                b"1".to_vec()
            },
            _ => return Err(crate::lib::result::KernelError::InvalidArgument),
        };

        let offset = offset as usize;
        if offset >= content.len() {
            return Ok(0);
        }

        let to_read = core::cmp::min(buf.len(), content.len() - offset);
        buf[..to_read].copy_from_slice(&content[offset..offset + to_read]);
        Ok(to_read)
    }

    fn write(&self, _ino: InodeNumber, _offset: u64, _data: &[u8]) -> KernelResult<usize> {
        // procfs 文件只读
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readdir(&self, ino: InodeNumber) -> KernelResult<Vec<DirectoryEntry>> {
        let mut entries = Vec::new();

        entries.push(DirectoryEntry::new(b".", ino, FileType::Directory)?);
        entries.push(DirectoryEntry::new(b"..", ino, FileType::Directory)?);

        // 处理虚拟 PID 目录
        if ino >= 0x1_0000_0000 && ino < 0x2_0000_0000 {
            // 虚拟 PID 目录：列出该进程的文件
            let pid = (ino & 0xFFFF_FFFF) as u32;
            let file_names = [
                (b"cmdline" as &[u8], 0u8),
                (b"status", 1u8),
                (b"stat", 2u8),
                (b"exe", 3u8),
            ];

            for (name, idx) in file_names.iter() {
                // 生成虚拟文件 inode：0x2_0000_0000 | (pid << 8) | idx
                let file_ino = 0x2_0000_0000u64 | ((pid as u64) << 8) | (*idx as u64);
                entries.push(DirectoryEntry::new(name, file_ino, FileType::File)?);
            }
            return Ok(entries);
        }

        // 处理静态目录
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if !node.is_dir() {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // 如果是根目录，需要列出系统文件 + 活进程
        if ino == 2 {
            // 先添加静态子项（系统文件）
            for (name, child_ino) in &node.children {
                if let Some(child_node) = self.get_node(*child_ino) {
                    entries.push(DirectoryEntry::new(name, *child_ino, child_node.metadata.file_type)?);
                }
            }

            // 然后添加动态进程目录（从真实任务表获取）
            // 为什么从真实任务表读取：进程在运行时创建/销毁，不能预创建
            let pids = crate::task::list_all_pids();
            for pid in pids {
                let pid_name = alloc::format!("{}", pid).into_bytes();
                // 生成虚拟 inode：0x1_0000_0000 | pid
                let virtual_ino = 0x1_0000_0000u64 | (pid as u64);
                entries.push(DirectoryEntry::new(&pid_name, virtual_ino, FileType::Directory)?);
            }
        } else {
            // 其他静态目录：列出预存的子项
            for (name, child_ino) in &node.children {
                if let Some(child_node) = self.get_node(*child_ino) {
                    entries.push(DirectoryEntry::new(name, *child_ino, child_node.metadata.file_type)?);
                }
            }
        }

        Ok(entries)
    }

    fn create(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn mkdir(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _mode: FileMode,
    ) -> KernelResult<InodeNumber> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn unlink(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn rmdir(&self, _parent_ino: InodeNumber, _name: &[u8]) -> KernelResult<()> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn symlink(
        &self,
        _parent_ino: InodeNumber,
        _name: &[u8],
        _target: &[u8],
    ) -> KernelResult<InodeNumber> {
        Err(crate::lib::result::KernelError::InvalidArgument)
    }

    fn readlink(&self, ino: InodeNumber, buf: &mut [u8]) -> KernelResult<usize> {
        let node = self.get_node(ino)
            .ok_or(crate::lib::result::KernelError::InvalidArgument)?;

        if node.metadata.file_type != FileType::Symlink {
            return Err(crate::lib::result::KernelError::InvalidArgument);
        }

        // /proc/self 指向 "1"（init 进程）
        let target = b"1";
        let len = core::cmp::min(buf.len(), target.len());
        buf[..len].copy_from_slice(&target[..len]);
        Ok(len)
    }
}

// ============================================================
// 单元测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_procfs_creation() {
        let procfs = Procfs::new().unwrap();
        assert_eq!(procfs.root_inode(), 2);
    }

    #[test]
    fn test_procfs_readdir_root() {
        let procfs = Procfs::new().unwrap();
        let entries = procfs.readdir(2).unwrap();
        // 应该包含系统文件 + /proc/1 + /proc/self
        assert!(entries.len() >= 10);
    }

    #[test]
    fn test_procfs_version_from_config() {
        let procfs = Procfs::new().unwrap();
        let mut buf = [0u8; 256];
        // version 通常是 inode 6
        let n = procfs.read(6, 0, &mut buf).unwrap();
        assert!(n > 0);
        let content = core::str::from_utf8(&buf[..n]).unwrap();
        // 应该包含内核名称
        assert!(content.contains("AXIS"));
    }
}
