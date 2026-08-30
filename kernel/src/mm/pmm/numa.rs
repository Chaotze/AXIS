// ============================================================
// NUMA（非一致内存架构）支持
// ============================================================
// 在内核尚未读取 ACPI SRAT 表之前，本模块提供：
// - 节点（NumaNode）与拓扑（NumaTopology）的数据结构
// - 节点↔内存区域的关联关系（每个节点拥有一组 Zone 索引）
// - 单节点回退：所有区域默认挂在节点 0（均匀内存，UMA）
//
// 设计要点（为什么这么做）：
// 1. 保持纯逻辑、可宿主单测：节点与区域的关系用一个「zone 索引
//    静态切片」表达，不持有 Zone 本体——Zone 由 pmm 胶水层统一
//    持有，避免双重所有权。
// 2. 预留扩展点：未来从 SRAT 表解析出内存亲和位图后，只需新增
//    `set_node_affinity(pfn, node)` 并在本模块补充邻近/交错策略，
//    上层 API（alloc 时显式指定首选节点）不需要变化。
// 3. 本里程碑不实现跨节点负载均衡与内存交错策略——多节点机器
//    尚未接入，先做到「结构就位 + 单节点可用」，避免空转复杂度。

/// 每个节点最多可关联的区域数
pub const MAX_NODES: usize = 8;
/// 每个节点最多可关联的 Zone 数
pub const MAX_ZONES_PER_NODE: usize = 4;

/// NUMA 节点
#[derive(Debug, Clone, Copy)]
pub struct NumaNode {
    /// 节点 ID（对应于 ACPI SRAT 的 proximity domain）
    pub id: u32,
    /// 该节点拥有的区域索引（指向全局 Zone 数组）
    pub zone_ids: &'static [usize],
}

impl NumaNode {
    /// 节点是否没有任何内存
    #[inline]
    pub fn is_memoryless(&self) -> bool {
        self.zone_ids.is_empty()
    }

    /// 节点上的首个区域索引（内存分配首选）
    #[inline]
    pub fn first_zone(&self) -> Option<usize> {
        self.zone_ids.first().copied()
    }

    /// 遍历节点上的区域索引
    #[inline]
    pub fn iter_zones(&self) -> core::slice::Iter<'_, usize> {
        self.zone_ids.iter()
    }
}

/// 节点间距离矩阵（最近邻亲疏度，SRAT 表就绪前全为 1）
///
/// 为什么用固定二维数组而非 Vec：节点数很少（<= 8），
/// 固定数组即可覆盖，避免分配器之间的相互依赖。
#[derive(Debug, Clone, Copy)]
pub struct DistanceMatrix {
    n: usize,
    dist: [[u8; MAX_NODES]; MAX_NODES],
}

impl DistanceMatrix {
    /// 构造全 1 距离矩阵（默认均匀拓扑）
    pub const fn uniform(n: usize) -> Self {
        Self {
            n,
            dist: [[1; MAX_NODES]; MAX_NODES],
        }
    }

    /// 查询节点 a 到 b 的相对距离（越小越近）
    #[inline]
    pub fn distance(&self, a: usize, b: usize) -> u8 {
        if a < self.n && b < self.n {
            self.dist[a][b]
        } else {
            u8::MAX
        }
    }
}

/// NUMA 拓扑
#[derive(Debug, Clone, Copy)]
pub struct NumaTopology {
    nodes: [NumaNode; MAX_NODES],
    count: usize,
    /// 节点间距离矩阵
    pub distances: DistanceMatrix,
}

impl NumaTopology {
    /// 构造「单节点、拥有全部区域」的均匀拓扑（UMA 回退）
    ///
    /// zone_ids 指向一个静态切片；调用方（胶水层）须保证该切片
    /// 生命周期为 'static（例如放在 static 数组中）。
    pub const fn single_node(zone_ids: &'static [usize]) -> Self {
        let mut nodes = [NumaNode {
            id: 0,
            zone_ids: &[],
        }; MAX_NODES];
        nodes[0] = NumaNode { id: 0, zone_ids };
        Self {
            nodes,
            count: 1,
            distances: DistanceMatrix::uniform(1),
        }
    }

    /// 节点数量
    #[inline]
    pub const fn node_count(&self) -> usize {
        self.count
    }

    /// 是否单节点（UMA）
    #[inline]
    pub fn is_uniform(&self) -> bool {
        self.count <= 1
    }

    /// 获取节点
    #[inline]
    pub fn node(&self, id: usize) -> Option<&NumaNode> {
        self.nodes.get(id)
    }

    /// 遍历所有节点
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, NumaNode> {
        self.nodes[..self.count].iter()
    }
}

/// 节点选择策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodePolicy {
    /// 默认：取当前 CPU 所在节点（本里程碑恒为节点 0）
    #[default]
    Local,
    /// 轮转：跨所有有内存的节点平均分配
    Interleave,
    /// 固定节点（例如设备 DMA 要求落在特定节点）
    Fixed(usize),
}

// ---------- 宿主单元测试（通过 unitest crate 以 #[path] 方式编译运行） ----------
#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::*;

    const ALL_ZONES: [usize; 2] = [0, 1];

    #[test]
    fn test_single_node_uniform() {
        let topo = NumaTopology::single_node(&ALL_ZONES);
        assert_eq!(topo.node_count(), 1);
        assert!(topo.is_uniform());
        let n = topo.node(0).expect("node0");
        assert_eq!(n.id, 0);
        assert!(!n.is_memoryless());
        assert_eq!(n.first_zone(), Some(0));
        let zones: Vec<usize> = n.iter_zones().copied().collect();
        assert_eq!(zones, vec![0, 1]);
        assert_eq!(topo.distances.distance(0, 0), 1);
    }

    #[test]
    fn test_multi_node_topology() {
        // 模拟双节点：节点 0 拥有 zone 0，节点 1 拥有 zone 1
        let z0: &'static [usize] = &[0];
        let z1: &'static [usize] = &[1];
        let n0 = NumaNode { id: 0, zone_ids: z0 };
        let n1 = NumaNode { id: 1, zone_ids: z1 };
        let mut nodes = [NumaNode { id: 0, zone_ids: &[] }; MAX_NODES];
        nodes[0] = n0;
        nodes[1] = n1;
        let topo = NumaTopology {
            nodes,
            count: 2,
            distances: DistanceMatrix::uniform(2),
        };
        assert!(!topo.is_uniform());
        assert_eq!(topo.node(1).unwrap().first_zone(), Some(1));
        assert_ne!(topo.distances.distance(0, 1), u8::MAX);
    }

    #[test]
    fn test_policies_exist() {
        let _ = NodePolicy::default();
        let _ = NodePolicy::Local;
        let _ = NodePolicy::Interleave;
        let _ = NodePolicy::Fixed(2);
    }
}