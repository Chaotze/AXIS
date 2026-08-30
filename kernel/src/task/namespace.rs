// ============================================================
// 命名空间（namespace）
// ============================================================
// 六类 Linux 命名空间的表示与 unshare/clone 语义（纯逻辑层）。
//
// 为什么需要命名空间：
// - 隔离全局资源视图（PID、网络、挂载、UTS、IPC、用户），
//   roadmap 4.3 的交付物；为将来的容器化与兼容层提供基础
// - 与 Linux 一致：命名空间按"实例"编号，进程持有一组
//   （每类各一个）实例引用；unshare 时对指定类别创建新实例
//
// 为什么设计为纯逻辑模块：
// - 实例引用表、unshare 掩码语义不依赖硬件；真正的隔离
//   效果（PID 表分片、网络栈分片）由各子系统在查表时
//   以 ns 实例号为键实现——那是胶水层的事，本模块只
//   定义语义正确、可测的数据结构

/// 命名空间类别（6 类，与 Linux CLONE_NEW* 一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceType {
    /// PID 命名空间（进程号视图）
    Pid,
    /// 网络命名空间
    Net,
    /// 挂载命名空间
    Mnt,
    /// UTS 命名空间（主机名/域名）
    Uts,
    /// IPC 命名空间（System V IPC / POSIX mq）
    Ipc,
    /// 用户命名空间（UID/GID 映射）
    User,
}

/// 类别总数
pub const NAMESPACE_TYPES: usize = 6;

impl NamespaceType {
    /// 类别序号（数组下标）
    pub const fn index(self) -> usize {
        match self {
            NamespaceType::Pid => 0,
            NamespaceType::Net => 1,
            NamespaceType::Mnt => 2,
            NamespaceType::Uts => 3,
            NamespaceType::Ipc => 4,
            NamespaceType::User => 5,
        }
    }

    /// 全部类别（按数组顺序）
    pub const ALL: [NamespaceType; NAMESPACE_TYPES] = [
        NamespaceType::Pid,
        NamespaceType::Net,
        NamespaceType::Mnt,
        NamespaceType::Uts,
        NamespaceType::Ipc,
        NamespaceType::User,
    ];
}

/// 命名空间类别位集（6 位，供 unshare/clone 标志打包）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NamespaceSet(pub u8);

impl NamespaceSet {
    /// 空集
    pub const fn empty() -> Self {
        Self(0)
    }

    /// 是否包含某类别
    pub const fn contains(&self, ty: NamespaceType) -> bool {
        self.0 & (1 << ty.index()) != 0
    }

    /// 加入某类别
    pub fn add(&mut self, ty: NamespaceType) {
        self.0 |= 1 << ty.index();
    }

    /// 是否为空
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// 命名空间实例号
pub type NamespaceId = u32;

/// 无效实例号（尚未分配/不存在）
pub const INVALID_NAMESPACE: NamespaceId = u32::MAX;

/// 进程的命名空间视图：每类别一个实例引用
///
/// 为什么用定长数组而非动态表：
/// - 类别数固定（6），数组内嵌在 PCB 中零堆开销；
///   与 mm/task 子系统的"定长 + 索引"风格一致
#[derive(Debug, Clone, Copy)]
pub struct NamespaceView {
    /// 各类别的实例号（INVALID_NAMESPACE 表示缺失）
    pub entries: [NamespaceId; NAMESPACE_TYPES],
}

impl Default for NamespaceView {
    /// 默认视图：全部无效（由 init 任务在初始化时统一分配）
    fn default() -> Self {
        Self {
            entries: [INVALID_NAMESPACE; NAMESPACE_TYPES],
        }
    }
}

impl NamespaceView {
    /// 读取某类别的实例号
    pub fn get(&self, ty: NamespaceType) -> NamespaceId {
        self.entries[ty.index()]
    }

    /// 写入某类别的实例号
    pub fn set(&mut self, ty: NamespaceType, id: NamespaceId) {
        self.entries[ty.index()] = id;
    }

    /// 与另一个视图逐类别比较，返回所有"实例不同"的类别
    ///
    /// 用途：跨进程共享资源前判断是否处于同一命名空间
    pub fn diff(&self, other: &Self) -> NamespaceSet {
        let mut diff = NamespaceSet::empty();
        for ty in NamespaceType::ALL {
            if self.get(ty) != other.get(ty) {
                diff.add(ty);
            }
        }
        diff
    }
}

/// 命名空间实例分配器（纯逻辑：每个类别独立计数）
///
/// 为什么把实例计数独立成结构：
/// - 真实系统中各命名空间是独立对象（PID 表、网络栈……），
///   这里以"自增实例号"模拟其标识；将来子系统接入时
///   以实例号为键即可，不需要改本模块
#[derive(Debug, Clone, Copy, Default)]
pub struct NamespaceRegistry {
    /// 各类别已分配的最大实例号（自增分配）
    next_ids: [NamespaceId; NAMESPACE_TYPES],
}

impl NamespaceRegistry {
    /// 为某类别创建新实例，返回新实例号
    pub fn create(&mut self, ty: NamespaceType) -> NamespaceId {
        let i = ty.index();
        let id = self.next_ids[i];
        self.next_ids[i] = id.wrapping_add(1);
        id
    }

    /// 对给定类别集执行 unshare：为其中每类创建新实例，
    /// 更新视图并返回发生变化的类别
    ///
    /// # 为什么返回变化集而不是新视图：
    /// - 调用方（clone/unshare 系统调用的内核侧）需要
    ///   知道哪些资源视图发生了变化，以便重建对应子系统
    ///   的视图（如新 PID 表的初始化）
    pub fn unshare(&mut self, view: &mut NamespaceView, set: NamespaceSet) -> NamespaceSet {
        let mut changed = NamespaceSet::empty();
        for ty in NamespaceType::ALL {
            if set.contains(ty) {
                let new_id = self.create(ty);
                view.set(ty, new_id);
                changed.add(ty);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_set_ops() {
        let mut set = NamespaceSet::empty();
        assert!(set.is_empty());
        set.add(NamespaceType::Pid);
        set.add(NamespaceType::User);
        assert!(set.contains(NamespaceType::Pid));
        assert!(!set.contains(NamespaceType::Net));
    }

    #[test]
    fn test_registry_allocates_distinct_ids() {
        let mut reg = NamespaceRegistry::default();
        let a = reg.create(NamespaceType::Pid);
        let b = reg.create(NamespaceType::Pid);
        assert_ne!(a, b);
        // 不同类别独立计数：Net 从 0 开始
        assert_eq!(reg.create(NamespaceType::Net), 0);
    }

    #[test]
    fn test_unshare_semantics() {
        let mut reg = NamespaceRegistry::default();
        // 初始视图：全类共享同一实例 0
        let mut view = NamespaceView::default();
        for ty in NamespaceType::ALL {
            view.set(ty, reg.create(ty));
        }

        // 只 unshare PID：PID 实例变化，其余共享
        let mut set = NamespaceSet::empty();
        set.add(NamespaceType::Pid);
        let changed = reg.unshare(&mut view, set);

        assert!(changed.contains(NamespaceType::Pid));
        assert!(!changed.contains(NamespaceType::Net));
        assert_eq!(view.get(NamespaceType::Pid), 1);
        assert_eq!(view.get(NamespaceType::Net), 0);

        // 父子进程 diff 只应报告 PID
        let mut parent = NamespaceView::default();
        for ty in NamespaceType::ALL {
            parent.set(ty, 0);
        }
        let mut diff = parent.diff(&view);
        let mut expected = NamespaceSet::empty();
        expected.add(NamespaceType::Pid);
        // diff 与顺序无关：比较位集
        assert_eq!(diff.0, expected.0);
        diff.add(NamespaceType::Mnt);
        assert!(diff.contains(NamespaceType::Mnt));
    }
}
