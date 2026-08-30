// ============================================================
// 多核负载均衡（load_balance）
// ============================================================
// 各 CPU 就绪队列之间的负载统计与迁移决策（纯逻辑层）。
//
// 为什么需要负载均衡：
// - 每个 CPU 有自己的 CFS 就绪队列；新任务若总是落在
//   CPU 0，多核优势荡然无存。周期性地把任务从"最忙"
//   的核迁移到"最闲"的核，让各核负载趋于一致
//
// 触发时机（Linux 语义，本模块提供决策纯函数）：
// - 周期均衡：每固定 tick 数检查一次（如每 10ms）
// - 空闲均衡：CPU 进入 idle 前主动"偷"任务
//
// 为什么不均衡要有阈值：
// - 迁移任务要付出缓存失效代价；负载差 1 个任务时
//   迁移得不偿失——阈值 = "不均衡度百分比 + 最小任务数"
//   的双重门槛，与 Linux imbalance_pct 思想一致
//
// 为什么设计为纯逻辑模块：
// - 迁移决策只是数组上的比较与算术；真实的迁移动作
//   （跨核出队/入队 + IPI 通知）由胶水层调用本模块
//   的结果执行

/// 不均衡阈值：最忙核负载必须超过最闲核 25% 才迁移
pub const IMBALANCE_PCT: usize = 125; // 125% = 超出 25%

/// 最小迁移量：负载差至少 2 个任务才值得动手
pub const MIN_MIGRATE: usize = 2;

/// 多核负载视图与迁移决策器
pub struct LoadBalancer<const MAX_CPUS: usize> {
    /// 各 CPU 就绪队列长度（负载的度量）
    loads: [usize; MAX_CPUS],
    /// 已接入的 CPU 数（≤ MAX_CPUS）
    active_cpus: usize,
}

impl<const MAX_CPUS: usize> LoadBalancer<MAX_CPUS> {
    /// 创建负载视图
    pub const fn new() -> Self {
        Self {
            loads: [0; MAX_CPUS],
            active_cpus: 0,
        }
    }

    /// 设置接入的 CPU 数（初始化时调用）
    pub fn set_active_cpus(&mut self, count: usize) {
        assert!(count <= MAX_CPUS, "CPU 数超过视图容量");
        self.active_cpus = count;
    }

    /// 接入的 CPU 数
    pub const fn active_cpus(&self) -> usize {
        self.active_cpus
    }

    /// 设置某 CPU 的负载
    pub fn set_load(&mut self, cpu: usize, load: usize) {
        self.loads[cpu] = load;
    }

    /// 读取某 CPU 的负载
    pub fn load(&self, cpu: usize) -> usize {
        self.loads[cpu]
    }

    /// 总负载
    pub fn total_load(&self) -> usize {
        self.loads[..self.active_cpus].iter().sum()
    }

    /// 找负载最大的 CPU
    pub fn find_busiest(&self) -> Option<(usize, usize)> {
        (0..self.active_cpus)
            .map(|cpu| (cpu, self.loads[cpu]))
            .max_by_key(|&(_, load)| load)
    }

    /// 找负载最小的 CPU
    pub fn find_idlest(&self) -> Option<(usize, usize)> {
        (0..self.active_cpus)
            .map(|cpu| (cpu, self.loads[cpu]))
            .min_by_key(|&(_, load)| load)
    }

    /// 判定是否需要从 from 向 to 迁移
    ///
    /// 双重门槛（百分比 + 最小差）：
    /// - from 负载 / to 负载 > 125%（负载差超过 25%）
    /// - 且负载差 ≥ 2 个任务（避免小打小闹的乒乓迁移）
    pub fn should_migrate(&self, from: usize, to: usize) -> bool {
        let (busy, idle) = (self.loads[from], self.loads[to]);
        if busy < idle {
            return false; // 方向反了
        }
        let diff = busy - idle;
        // 百分比门槛：busy 比 idle 多出 25% 以上
        // （等价 busy*100 > idle*125）
        busy * 100 > idle * IMBALANCE_PCT && diff >= MIN_MIGRATE
    }

    /// 计算建议迁移量：取"差距的一半"（迁后双方趋于相等），
    /// 但至少 1 个、至多全部差距
    pub fn migrate_amount(&self, from: usize, to: usize) -> usize {
        let (busy, idle) = (self.loads[from], self.loads[to]);
        if busy <= idle {
            return 0;
        }
        ((busy - idle) / 2).max(1)
    }

    /// 执行一次迁移记账：把 count 个任务从 from 挪到 to，
    /// 返回实际迁移数（受 from 现有负载约束）
    pub fn migrate(&mut self, from: usize, to: usize, count: usize) -> usize {
        let actual = count.min(self.loads[from]);
        self.loads[from] -= actual;
        self.loads[to] += actual;
        actual
    }

    /// 一轮均衡：若最忙/最闲核不均衡，迁移一次并返回
    /// (from, to, 迁移数)；已均衡返回 None
    pub fn balance_once(&mut self) -> Option<(usize, usize, usize)> {
        let (busiest, _) = self.find_busiest()?;
        let (idlest, _) = self.find_idlest()?;
        if busiest == idlest {
            return None; // 单核或全相等
        }
        if !self.should_migrate(busiest, idlest) {
            return None;
        }
        let amount = self.migrate_amount(busiest, idlest);
        let actual = self.migrate(busiest, idlest, amount);
        Some((busiest, idlest, actual))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_find_busiest_idlest() {
        let mut lb: LoadBalancer<8> = LoadBalancer::new();
        lb.set_active_cpus(4);
        lb.set_load(0, 10);
        lb.set_load(1, 3);
        lb.set_load(2, 7);
        lb.set_load(3, 5);

        assert_eq!(lb.find_busiest(), Some((0, 10)));
        assert_eq!(lb.find_idlest(), Some((1, 3)));
        assert_eq!(lb.total_load(), 25);
    }

    #[test]
    fn test_should_migrate_thresholds() {
        let mut lb: LoadBalancer<4> = LoadBalancer::new();
        lb.set_active_cpus(2);

        // 差 1：不足最小迁移量 → 不迁移
        lb.set_load(0, 5);
        lb.set_load(1, 4);
        assert!(!lb.should_migrate(0, 1));

        // 差 2 但比例不足 25%（8 vs 7）→ 不迁移
        lb.set_load(0, 8);
        lb.set_load(1, 7);
        assert!(!lb.should_migrate(0, 1));

        // 差 2 且比例达标（5 vs 3 = 166%）→ 迁移
        lb.set_load(0, 5);
        lb.set_load(1, 3);
        assert!(lb.should_migrate(0, 1));

        // 方向反了 → 拒绝
        assert!(!lb.should_migrate(1, 0));
    }

    #[test]
    fn test_balance_round() {
        let mut lb: LoadBalancer<4> = LoadBalancer::new();
        lb.set_active_cpus(2);
        lb.set_load(0, 12);
        lb.set_load(1, 2);

        let (from, to, moved) = lb.balance_once().unwrap();
        assert_eq!((from, to), (0, 1));
        assert_eq!(moved, 5); // (12-2)/2
        assert_eq!(lb.load(0), 7);
        assert_eq!(lb.load(1), 7);

        // 已均衡 → 不再迁移
        assert_eq!(lb.balance_once(), None);
    }

    #[test]
    fn test_balance_converges_over_rounds() {
        // 连续多轮均衡应使负载趋于一致
        let mut lb: LoadBalancer<4> = LoadBalancer::new();
        lb.set_active_cpus(4);
        lb.set_load(0, 40);
        lb.set_load(1, 4);
        lb.set_load(2, 8);
        lb.set_load(3, 12);

        for _ in 0..50 {
            if lb.balance_once().is_none() {
                break;
            }
        }

        let loads: std::vec::Vec<usize> = (0..4).map(|c| lb.load(c)).collect();
        let max = *loads.iter().max().unwrap();
        let min = *loads.iter().min().unwrap();
        assert!(max - min <= 2, "负载未收敛：{:?}", loads);
        // 总量守恒
        assert_eq!(lb.total_load(), 64);
    }
}
