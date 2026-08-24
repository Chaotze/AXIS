// ============================================================
// 结果类型
// ============================================================
// 定义内核操作的结果类型和错误码

use core::fmt;

/// 内核错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    /// 内存不足
    OutOfMemory,
    /// 无效参数
    InvalidArgument,
    /// 权限不足
    PermissionDenied,
    /// 资源不存在
    NotFound,
    /// 资源已存在
    AlreadyExists,
    /// 操作超时
    Timeout,
    /// 操作被中断
    Interrupted,
    /// 不支持的操作
    Unsupported,
    /// 设备错误
    DeviceError,
    /// I/O 错误
    IoError,
    /// 其他错误
    Other(&'static str),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::OutOfMemory => write!(f, "Out of memory"),
            Self::InvalidArgument => write!(f, "Invalid argument"),
            Self::PermissionDenied => write!(f, "Permission denied"),
            Self::NotFound => write!(f, "Not found"),
            Self::AlreadyExists => write!(f, "Already exists"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Interrupted => write!(f, "Interrupted"),
            Self::Unsupported => write!(f, "Unsupported"),
            Self::DeviceError => write!(f, "Device error"),
            Self::IoError => write!(f, "I/O error"),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

/// 内核结果类型
pub type KernelResult<T> = Result<T, KernelError>;
