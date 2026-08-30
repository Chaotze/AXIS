// ============================================================
// VFS 系统调用层
// ============================================================
// 实现文件系统相关的系统调用（open、read、write、close 等）
// 通过 TrapFrame 与用户空间交互

use crate::arch::x86_64::context::frame::TrapFrame;
use crate::lib::result::KernelError;

// ============================================================
// 系统调用号定义
// ============================================================

/// VFS 相关的系统调用号
pub mod syscall_numbers {
    pub const SYS_READ: u64 = 0;
    pub const SYS_WRITE: u64 = 1;
    pub const SYS_OPEN: u64 = 2;
    pub const SYS_CLOSE: u64 = 3;
    pub const SYS_STAT: u64 = 4;
    pub const SYS_MKDIR: u64 = 39;
    pub const SYS_RMDIR: u64 = 40;
}

// ============================================================
// 错误转换
// ============================================================

/// 将 KernelError 转换为 Linux errno
/// 为什么需要转换：系统调用通过负的 errno 返回错误
pub fn error_to_errno(err: KernelError) -> i64 {
    match err {
        KernelError::NotFound => -2,              // ENOENT
        KernelError::PermissionDenied => -13,     // EACCES
        KernelError::InvalidArgument => -22,      // EINVAL
        KernelError::OutOfMemory => -12,          // ENOMEM
        KernelError::IoError => -5,               // EIO
        KernelError::AlreadyExists => -17,        // EEXIST
        KernelError::Timeout => -110,             // ETIMEDOUT
        KernelError::Interrupted => -4,           // EINTR
        KernelError::Unsupported => -38,          // ENOSYS
        KernelError::DeviceError => -19,          // ENODEV
        KernelError::Other(_) => -1,              // 通用错误
    }
}

// ============================================================
// 系统调用处理器
// ============================================================

// ============================================================
// 系统调用处理器
// ============================================================

/// 总的系统调用分发入口
/// 为什么放在这里：所有 VFS 系统调用通过此入口分发
#[unsafe(no_mangle)]
pub extern "C" fn fs_syscall_handler(frame: &mut TrapFrame) {
    let syscall_num = frame.rax;

    let result = match syscall_num {
        syscall_numbers::SYS_READ => sys_read(frame),
        syscall_numbers::SYS_WRITE => sys_write(frame),
        syscall_numbers::SYS_OPEN => sys_open(frame),
        syscall_numbers::SYS_CLOSE => sys_close(frame),
        syscall_numbers::SYS_STAT => sys_stat(frame),
        syscall_numbers::SYS_MKDIR => sys_mkdir(frame),
        syscall_numbers::SYS_RMDIR => sys_rmdir(frame),
        _ => Err(KernelError::InvalidArgument),
    };

    // 设置返回值
    frame.rax = match result {
        Ok(val) => val as u64,
        Err(err) => error_to_errno(err) as u64,
    };
}

// ============================================================
// read 系统调用
// ============================================================

/// read(fd, buf, count) - 从文件读取数据
/// 参数：
///   rdi = fd（文件描述符）
///   rsi = buf_ptr（缓冲区地址）
///   rdx = count（读取字节数）
/// 返回值：
///   rax = 读取字节数，或错误码
fn sys_read(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _fd = frame.rdi as i32;
    let _buf_ptr = frame.rsi;
    let _count = frame.rdx as usize;

    // 为什么这样实现：框架代码，实际实现需要：
    // 1. 从当前进程的 fd 表获取 OpenFile
    // 2. 验证 buf_ptr 是否在用户空间
    // 3. 从文件系统读取数据
    // 4. 复制数据到用户缓冲区
    // 这些功能需要与 task 和 mm 模块集成

    // 目前返回 0（占位符）
    Ok(0)
}

// ============================================================
// write 系统调用
// ============================================================

/// write(fd, buf, count) - 写入数据到文件
/// 参数：
///   rdi = fd
///   rsi = buf_ptr
///   rdx = count
/// 返回值：
///   rax = 写入字节数，或错误码
fn sys_write(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _fd = frame.rdi as i32;
    let _buf_ptr = frame.rsi;
    let _count = frame.rdx as usize;

    // TODO: 完整实现
    Ok(0)
}

// ============================================================
// open 系统调用
// ============================================================

/// open(path, flags, mode) - 打开文件
/// 参数：
///   rdi = path_ptr（路径字符串地址）
///   rsi = flags（打开标志）
///   rdx = mode（文件权限）
/// 返回值：
///   rax = 文件描述符，或错误码
fn sys_open(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _path_ptr = frame.rdi;
    let _flags = frame.rsi as u32;
    let _mode = frame.rdx as u32;

    // TODO: 完整实现
    // 1. 从用户空间复制路径字符串
    // 2. 验证路径
    // 3. 查找或创建 inode
    // 4. 创建 OpenFile 对象
    // 5. 在 fd 表中分配 fd
    // 6. 返回 fd

    Ok(-1)  // 失败（占位符）
}

// ============================================================
// close 系统调用
// ============================================================

/// close(fd) - 关闭文件
/// 参数：
///   rdi = fd
/// 返回值：
///   rax = 0（成功）或错误码
fn sys_close(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _fd = frame.rdi as i32;

    // TODO: 完整实现
    // 1. 验证 fd 有效
    // 2. 从 fd 表中删除
    // 3. 释放相关资源

    Ok(0)
}

// ============================================================
// stat 系统调用
// ============================================================

/// stat(path, statbuf) - 获取文件状态
/// 参数：
///   rdi = path_ptr
///   rsi = statbuf_ptr（stat 结构体地址）
/// 返回值：
///   rax = 0（成功）或错误码
fn sys_stat(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _path_ptr = frame.rdi;
    let _statbuf_ptr = frame.rsi;

    // TODO: 完整实现
    // 1. 从用户空间复制路径
    // 2. 查找 inode
    // 3. 获取元数据
    // 4. 构造 stat 结构体
    // 5. 复制回用户空间

    Ok(0)
}

// ============================================================
// mkdir 系统调用
// ============================================================

/// mkdir(path, mode) - 创建目录
/// 参数：
///   rdi = path_ptr
///   rsi = mode
/// 返回值：
///   rax = 0（成功）或错误码
fn sys_mkdir(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _path_ptr = frame.rdi;
    let _mode = frame.rsi as u32;

    // TODO: 完整实现
    Ok(0)
}

// ============================================================
// rmdir 系统调用
// ============================================================

/// rmdir(path) - 删除目录
/// 参数：
///   rdi = path_ptr
/// 返回值：
///   rax = 0（成功）或错误码
fn sys_rmdir(frame: &mut TrapFrame) -> Result<i64, KernelError> {
    let _path_ptr = frame.rdi;

    // TODO: 完整实现
    Ok(0)
}

// ============================================================
// 用户指针验证
// ============================================================

/// 检查地址是否在用户空间
/// 为什么需要检查：防止用户程序访问内核内存
pub fn is_user_pointer(addr: u64) -> bool {
    // 用户空间范围：0x0 - 0x7FFF_FFFF_FFFF
    addr < 0x8000_0000_0000
}

/// 从用户空间安全地复制字符串
/// 为什么需要这个函数：防止用户程序传入无效指针导致内核崩溃
pub fn copy_string_from_user(ptr: u64, max_len: usize) -> Result<alloc::vec::Vec<u8>, KernelError> {
    use alloc::vec::Vec;

    if !is_user_pointer(ptr) {
        return Err(KernelError::InvalidArgument);
    }

    let mut result = Vec::new();

    // 为什么逐字节复制：可以检查每个字节是否有效
    for i in 0..max_len {
        let byte_ptr = ptr + i as u64;

        if !is_user_pointer(byte_ptr) {
            return Err(KernelError::InvalidArgument);
        }

        // 注意：实际实现需要安全的内存访问机制
        // 这里仅为概念展示
        let byte = unsafe { *(byte_ptr as *const u8) };

        if byte == 0 {
            // 找到 NUL 终止符
            break;
        }

        result.push(byte);
    }

    Ok(result)
}

/// 从用户空间安全地复制缓冲区
pub fn copy_from_user(src: u64, dst: &mut [u8]) -> Result<(), KernelError> {
    if !is_user_pointer(src) {
        return Err(KernelError::InvalidArgument);
    }

    // 注意：实际实现需要安全的内存访问机制
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), dst.len());
    }

    Ok(())
}

/// 复制数据到用户空间
pub fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), KernelError> {
    if !is_user_pointer(dst) {
        return Err(KernelError::InvalidArgument);
    }

    // 注意：实际实现需要安全的内存访问机制
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len());
    }

    Ok(())
}
