// ============================================================
// cgroup v2 资源控制
// ============================================================
// 统一层级（unified hierarchy）的进程分组与限额检查（纯逻辑层）。
//
// 为什么选择 v2：
// - v2 单层级模型比 v1 多层级挂载更简洁，是 Linux 现行标准
//
// 为什么设计为纯逻辑模块：
// - 限额参数、记账计数、超限判定不依赖硬件；真实的
//   强制手段（超限时 throttle/杀死进程）由调度器与
//   内存管理在各自检查点调用本模块的判定函数实现
//
// 为什么子组用定长子数组（const 泛型）：
// - 层级是树形结构，节点数在系统启动时确定（root + 若干
//   子系统组），定长数组 + 索引与 task 子系统的"定长
//   任务表"风格一致，零堆开销

/// cgroup 标识（表内索引）
pub type CgroupId = u32;

/// 无效 cgroup（未分组任务的缺省归属标志）
pub const INVALID_CGROUP: CgroupId = u32::MAX;

/// cgroup v2 节点
#[derive(Debug, Clone, Copy)]
pub struct CgroupV2 {
    /// 组名（内核内不保留字符串，只保留标识；名称映射
    /// 由文件系统层（cgroupfs 的 procfs 化）在将来维护）
    pub id: CgroupId,
    /// 父组（根组为 None）
    pub parent: Option<CgroupId>,
    /// 子组索引表（定长）
    pub children: [Option<CgroupId>; MAX_CHILDREN],
    /// 子组数
    pub child_count: usize,
    /// CPU 权重（cgroup v2 的 cpu.weight，默认 100；
    /// 与 nice 的权重正交：组内再按 nice 分配）
    pub cpu_weight: u32,
    /// CPU 限额（cpu.max）：(配额 us, 周期 us)；None = 不限制
    pub cpu_max: Option<(u64, u64)>,
    /// 内存高水位（memory.high，字节）：超过则开始回收
    pub memory_high: Option<u64>,
    /// 内存硬上限（memory.max，字节）：超过则分配失败/OOM
    pub memory_max: Option<u64>,
    /// 进程数上限（pids.max）；None = 不限制
    pub pids_max: Option<u32>,
    // ---- 记账（由各子系统在分配/调度点更新）----
    /// 本组当前进程数
    pub pids_current: u32,
    /// 本组内存使用（字节）
    pub memory_current: u64,
    /// 本组 CPU 使用（微秒）
    pub cpu_usage_us: u64,
}

/// 子组容量（编译期常量，供 const 泛型使用）
pub const MAX_CHILDREN: usize = 16;

impl CgroupV2 {
    /// 创建根组
    pub const fn new_root(id: CgroupId) -> Self {
        Self {
            id,
            parent: None,
            children: [None; MAX_CHILDREN],
            child_count: 0,
            cpu_weight: 100,
            cpu_max: None,
            memory_high: None,
            memory_max: None,
            pids_max: None,
            pids_current: 0,
            memory_current: 0,
            cpu_usage_us: 0,
        }
    }

    /// 限额判定接口集合 --------------------------------------------------

    /// CPU 是否超限
    ///
    /// 语义：quota/period 表示"每 period 微秒最多运行
    /// quota 微秒"；周期内用量超过 quota 即超限。
    /// 为什么按"周期内用量"判定而不是累计：
    /// - cpu.max 是带宽限制（如 50ms/100ms = 半个核），
    ///   调度器在每个周期开始时重置记账并据此节流
    pub fn cpu_exceeded(&self) -> bool {
        match self.cpu_max {
            None => false, // 无配额 = 不限制
            // 语义：周期内累计用量不得超过配额
            Some((quota, _period)) => self.cpu_usage_us > quota,
        }
    }

    /// 内存是否触及高水位（软限：回收压力信号）
    pub fn memory_pressure(&self) -> bool {
        match self.memory_high {
            None => false,
            Some(high) => self.memory_current > high,
        }
    }

    /// 内存是否超过硬上限（分配应失败或触发 OOM）
    pub fn memory_exceeded(&self) -> bool {
        match self.memory_max {
            None => false,
            Some(max) => self.memory_current > max,
        }
    }

    /// 进程数是否超限
    pub fn pids_exceeded(&self) -> bool {
        match self.pids_max {
            None => false,
            Some(max) => self.pids_current >= max,
        }
    }
}

/// cgroup 层级表（根组 + 定长子组，树形索引）
pub struct CgroupTable<const MAX_CGROUPS: usize> {
    /// 组节点数组（按 id 索引）
    groups: [Option<CgroupV2>; MAX_CGROUPS],
    /// 根组 id
    root: CgroupId,
    /// 组总数
    len: usize,
}

impl<const MAX_CGROUPS: usize> CgroupTable<MAX_CGROUPS> {
    /// 创建只含根组的层级
    pub const fn new() -> Self {
        Self {
            groups: [None; MAX_CGROUPS],
            root: 0,
            len: 0,
        }
    }

    /// 初始化：创建根组（0 号）
    ///
    /// 为什么根组在 init 而非 new 中创建：
    /// - 保持 new 为 const fn（支持静态初始化），
    ///   与 btree/radix 的惰性初始化约定一致
    pub fn init(&mut self) {
        self.groups[0] = Some(CgroupV2::new_root(0));
        self.len = 1;
    }

    /// 根组 id
    pub const fn root(&self) -> CgroupId {
        self.root
    }

    /// 组总数
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 读取组
    pub fn get(&self, id: CgroupId) -> Option<&CgroupV2> {
        self.groups.get(id as usize).and_then(|g| g.as_ref())
    }

    /// 可变读取组
    pub fn get_mut(&mut self, id: CgroupId) -> Option<&mut CgroupV2> {
        self.groups.get_mut(id as usize).and_then(|g| g.as_mut())
    }

    /// 创建子组（挂到 parent 之下）
    ///
    /// 返回 Err 当表满或父组子槽满。
    /// 为什么子组继承父组的限额配置：
    /// - v2 语义：子组是父组的细分，配置继承是默认行为
    ///   （"资源是分层的"）
    pub fn create_child(&mut self, parent: CgroupId) -> Result<CgroupId, &'static str> {
        if self.len >= MAX_CGROUPS {
            return Err("cgroup 表已满");
        }
        let id = self.len as CgroupId;

        // 父组必须存在
        let parent_group = self
            .groups
            .get(parent as usize)
            .and_then(|g| g.as_ref())
            .ok_or("父组不存在")?;
        if parent_group.child_count >= MAX_CHILDREN {
            return Err("父组子槽已满");
        }

        // 子组继承父组限额
        let mut child = CgroupV2::new_root(id);
        child.parent = Some(parent);
        child.cpu_weight = parent_group.cpu_weight;
        child.cpu_max = parent_group.cpu_max;
        child.memory_high = parent_group.memory_high;
        child.memory_max = parent_group.memory_max;

        self.groups[id as usize] = Some(child);
        self.len += 1;

        // 挂到父组子表
        let parent_group = self.groups[parent as usize].as_mut().unwrap();
        parent_group.children[parent_group.child_count] = Some(id);
        parent_group.child_count += 1;

        Ok(id)
    }

    /// 设置 CPU 限额（cpu.max）
    pub fn set_cpu_max(&mut self, id: CgroupId, quota_us: u64, period_us: u64) -> Result<(), &'static str> {
        let group = self.get_mut(id).ok_or("cgroup 不存在")?;
        if period_us == 0 {
            return Err("周期必须大于 0");
        }
        group.cpu_max = Some((quota_us, period_us));
        Ok(())
    }

    /// 设置内存上限
    pub fn set_memory_max(&mut self, id: CgroupId, bytes: u64) -> Result<(), &'static str> {
        let group = self.get_mut(id).ok_or("cgroup 不存在")?;
        group.memory_max = Some(bytes);
        Ok(())
    }

    /// 设置进程数上限
    pub fn set_pids_max(&mut self, id: CgroupId, max: u32) -> Result<(), &'static str> {
        let group = self.get_mut(id).ok_or("cgroup 不存在")?;
        group.pids_max = Some(max);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_root_group_defaults() {
        let mut table: CgroupTable<8> = CgroupTable::new();
        table.init();
        let root = table.get(0).unwrap();
        assert_eq!(root.parent, None);
        assert_eq!(root.cpu_weight, 100);
        assert!(!root.cpu_exceeded());
        assert!(!root.memory_exceeded());
        assert!(!root.pids_exceeded());
    }

    #[test]
    fn test_hierarchy_and_inheritance() {
        let mut table: CgroupTable<8> = CgroupTable::new();
        table.init();
        // 根组设限额
        table.set_cpu_max(0, 50_000, 100_000).unwrap();
        table.set_memory_max(0, 1 << 20).unwrap();

        // 子组继承限额
        let child = table.create_child(0).unwrap();
        let c = table.get(child).unwrap();
        assert_eq!(c.parent, Some(0));
        assert_eq!(c.cpu_max, Some((50_000, 100_000)));
        assert_eq!(c.memory_max, Some(1 << 20));
        // 子组挂在根组之下
        assert_eq!(table.get(0).unwrap().child_count, 1);
    }

    #[test]
    fn test_cpu_quota_judgement() {
        let mut table: CgroupTable<8> = CgroupTable::new();
        table.init();
        table.set_cpu_max(0, 50_000, 100_000).unwrap();
        let root = table.get_mut(0).unwrap();

        // 周期内用一半 → 未超
        root.cpu_usage_us = 50_000;
        assert!(!root.cpu_exceeded());
        // 超配额 → 超限
        root.cpu_usage_us = 50_001;
        assert!(root.cpu_exceeded());
    }

    #[test]
    fn test_memory_and_pids_judgement() {
        let mut table: CgroupTable<8> = CgroupTable::new();
        table.init();
        table.set_memory_max(0, 100).unwrap();
        table.set_pids_max(0, 3).unwrap();
        let root = table.get_mut(0).unwrap();

        root.memory_current = 99;
        assert!(!root.memory_exceeded());
        root.memory_current = 101;
        assert!(root.memory_exceeded());

        // pids：>= max 视为超限（达到上限即拒绝新进程）
        root.pids_current = 2;
        assert!(!root.pids_exceeded());
        root.pids_current = 3;
        assert!(root.pids_exceeded());
    }

    #[test]
    fn test_table_limits() {
        let mut table: CgroupTable<4> = CgroupTable::new();
        table.init();
        assert!(table.create_child(0).is_ok());
        assert!(table.create_child(0).is_ok());
        assert!(table.create_child(0).is_ok()); // 第 4 个组
        assert!(table.create_child(0).is_err()); // 表满
        assert!(table.set_cpu_max(9, 1, 1).is_err()); // 不存在
    }
}
