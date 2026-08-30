// ============================================================
// 资源限制（rlimit）
// ============================================================
// 进程级资源上限的纯数据结构与检查逻辑。
//
// 为什么需要 rlimit：
// - 防止单个进程耗尽系统资源（CPU 时间、文件句柄、内存、
//   进程数等），是 Unix 语义的组成部分，也是验收标准
//   "资源限制（rlimit）生效"的落点
// - fork/exec 时子进程继承父进程的限制；setrlimit 时
//   硬限制只能调小（特权进程例外），保证限制的单调性
//
// 为什么设计为纯逻辑模块：
// - 限制的存取、检查不依赖 arch 与全局状态，可被宿主
//   单元测试（unitest）直接编译验证

/// Linux 标准 rlimit 类别（与本模块的 RLIMITS 数组下标一致）
pub mod rlimit_type {
    /// CPU 时间（秒）
    pub const CPU: usize = 0;
    /// 文件大小上限（字节）
    pub const FSIZE: usize = 1;
    /// 数据段大小（字节）
    pub const DATA: usize = 2;
    /// 栈大小（字节）
    pub const STACK: usize = 3;
    /// core dump 文件大小（字节）
    pub const CORE: usize = 4;
    /// 常驻集大小（字节）
    pub const RSS: usize = 5;
    /// 可创建进程数
    pub const NPROC: usize = 6;
    /// 打开文件描述符数
    pub const NOFILE: usize = 7;
    /// 可锁定内存字节数
    pub const MEMLOCK: usize = 8;
    /// 地址空间大小（字节）
    pub const AS: usize = 9;
    /// 类别总数
    pub const COUNT: usize = 10;
}

/// 无限限制的哨兵值
pub const RLIM_INFINITY: u64 = u64::MAX;

/// 单项资源限制
///
/// 为什么 cur/max 分离（Linux 语义）：
/// - cur 是实际生效的软限制，进程可自行在 [0, max] 内调整
/// - max 是硬上限，普通进程只能单调下调（防止绕过限制），
///   只有特权进程可以上调
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RLimit {
    /// 软限制（当前生效值）
    pub cur: u64,
    /// 硬限制（软限制的上界）
    pub max: u64,
}

impl RLimit {
    /// 无限（默认值）
    pub const fn infinity() -> Self {
        Self {
            cur: RLIM_INFINITY,
            max: RLIM_INFINITY,
        }
    }

    /// 有限值（cur == max）
    pub const fn fixed(value: u64) -> Self {
        Self {
            cur: value,
            max: value,
        }
    }
}

/// 资源限制集合：类别号（见 rlimit_type）→ 限制值
#[derive(Debug, Clone, Copy)]
pub struct RLimits {
    limits: [RLimit; rlimit_type::COUNT],
}

impl Default for RLimits {
    /// 默认限制：全部无限
    ///
    /// 为什么默认无限：
    /// - 与 Linux 一致（无显式限制时资源由内核整体容量兜底）；
    ///   init 进程可在此基础上收紧后，子进程经 fork 继承
    fn default() -> Self {
        Self::new_infinity()
    }
}

impl RLimits {
    /// 全部无限
    pub const fn new_infinity() -> Self {
        Self {
            limits: [RLimit::infinity(); rlimit_type::COUNT],
        }
    }

    /// 读取某类别的限制
    pub fn get(&self, resource: usize) -> RLimit {
        self.limits[resource]
    }

    /// 设置某类别限制（非特权进程语义：软限制不得超过硬限制，
    /// 硬限制只能下降）
    ///
    /// # 参数
    /// - `resource`: 类别号（rlimit_type::*）
    /// - `limit`: 新限制值
    /// - `privileged`: 是否特权进程（允许上调硬限制）
    ///
    /// 返回 Ok 表示生效；Err 表示被拒绝（保持原值不变）。
    pub fn set(&mut self, resource: usize, limit: RLimit, privileged: bool) -> Result<(), &'static str> {
        let old = self.limits[resource];

        // 软限制永远不得超过硬限制（无论新旧）
        if limit.cur > limit.max {
            return Err("软限制不能超过硬限制");
        }

        // 非特权进程：硬限制只许下降（防绕过）
        if !privileged && limit.max > old.max {
            return Err("非特权进程只能下调硬限制");
        }

        self.limits[resource] = limit;
        Ok(())
    }

    /// 检查某用量是否违反软限制
    ///
    /// 返回 Ok 表示在限制内；Err 携带类别号供上层报错（如
    /// 信号 SIGXCPU / SIGKILL 的决策依据）。
    pub fn check(&self, resource: usize, usage: u64) -> Result<(), usize> {
        let limit = self.limits[resource];
        if usage > limit.cur {
            Err(resource)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_default_all_infinity() {
        let limits = RLimits::default();
        for i in 0..rlimit_type::COUNT {
            assert_eq!(limits.get(i).cur, RLIM_INFINITY);
            assert_eq!(limits.get(i).max, RLIM_INFINITY);
        }
    }

    #[test]
    fn test_set_and_check() {
        let mut limits = RLimits::default();
        // 限制文件描述符为 64
        assert!(limits.set(rlimit_type::NOFILE, RLimit::fixed(64), false).is_ok());
        assert_eq!(limits.get(rlimit_type::NOFILE).cur, 64);

        // 用量 63 在限制内，65 超限
        assert_eq!(limits.check(rlimit_type::NOFILE, 63), Ok(()));
        assert_eq!(limits.check(rlimit_type::NOFILE, 65), Err(rlimit_type::NOFILE));

        // 无限限制永不超限
        assert_eq!(limits.check(rlimit_type::CPU, RLIM_INFINITY - 1), Ok(()));
    }

    #[test]
    fn test_soft_cannot_exceed_hard() {
        let mut limits = RLimits::default();
        let bad = RLimit { cur: 100, max: 50 };
        assert!(limits.set(rlimit_type::DATA, bad, true).is_err());
        // 被拒绝后保持原值
        assert_eq!(limits.get(rlimit_type::DATA).cur, RLIM_INFINITY);
    }

    #[test]
    fn test_hard_limit_monotonic_for_unprivileged() {
        let mut limits = RLimits::default();
        // 非特权：先压到 1000
        assert!(limits.set(rlimit_type::AS, RLimit::fixed(1000), false).is_ok());
        // 试图上调硬限制 → 拒绝
        let raise = RLimit { cur: 2000, max: 2000 };
        assert!(limits.set(rlimit_type::AS, raise, false).is_err());
        // 下调 → 允许
        let lower = RLimit { cur: 500, max: 500 };
        assert!(limits.set(rlimit_type::AS, lower, false).is_ok());
        // 特权进程可以上调
        assert!(limits.set(rlimit_type::AS, raise, true).is_ok());
    }
}
