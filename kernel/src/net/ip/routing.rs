// ============================================================
// 路由表管理（Routing）
// ============================================================
// 实现路由表的维护和查询功能
//
// 路由表项格式：
//   {destination_prefix, prefix_length, gateway, metric, interface}
//
// 为什么分离路由模块：
// - 路由查询是 IP 层的核心功能，独立模块便于优化
// - 不同协议栈版本可有不同的路由算法（最长前缀匹配 LPM）
// - 便于添加高级功能（策略路由、多路径等）

use crate::lib::result::KernelResult;
use super::super::types::Ipv4Address;

// ============================================================
// 路由表项
// ============================================================

/// IPv4 路由表项
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// 目标前缀（网络地址）
    pub destination: Ipv4Address,
    /// 前缀长度（子网掩码位数）
    pub prefix_len: u8,
    /// 下一跳网关
    pub gateway: Ipv4Address,
    /// 跳数（路由优先级）
    pub metric: u32,
    /// 出接口编号
    pub interface: u32,
}

impl RouteEntry {
    /// 创建新的路由表项
    pub fn new(
        destination: Ipv4Address,
        prefix_len: u8,
        gateway: Ipv4Address,
        metric: u32,
        interface: u32,
    ) -> Self {
        RouteEntry {
            destination,
            prefix_len,
            gateway,
            metric,
            interface,
        }
    }

    /// 检查给定 IP 是否匹配此路由项
    pub fn matches(&self, ip: Ipv4Address) -> bool {
        if self.prefix_len == 0 {
            return true;  // 默认路由
        }

        let dest_u32 = self.destination.as_u32();
        let ip_u32 = ip.as_u32();

        // 创建子网掩码
        let mask = if self.prefix_len == 32 {
            0xFFFFFFFF
        } else {
            (0xFFFFFFFFu32 << (32 - self.prefix_len)) & 0xFFFFFFFF
        };

        (dest_u32 & mask) == (ip_u32 & mask)
    }
}

// ============================================================
// 路由表
// ============================================================

/// 路由表（简化版本，使用线性搜索）
/// TODO: 后续使用 Trie 或其他更高效的数据结构
pub struct RoutingTable {
    /// 路由表项列表
    entries: alloc::vec::Vec<RouteEntry>,
}

impl RoutingTable {
    /// 创建空的路由表
    pub fn new() -> Self {
        RoutingTable {
            entries: alloc::vec::Vec::new(),
        }
    }

    /// 添加路由表项
    pub fn add_route(&mut self, entry: RouteEntry) -> KernelResult<()> {
        if self.entries.len() >= crate::net::config::ROUTING_TABLE_MAX_ENTRIES {
            return Err(crate::prelude::KernelError::OutOfMemory);
        }
        self.entries.push(entry);
        // 按照前缀长度排序（最长前缀优先）
        self.entries.sort_by(|a, b| b.prefix_len.cmp(&a.prefix_len));
        Ok(())
    }

    /// 查询路由（最长前缀匹配）
    pub fn lookup(&self, dest_ip: Ipv4Address) -> Option<&RouteEntry> {
        // 最长前缀匹配：遍历已排序的列表，返回第一个匹配项
        for entry in &self.entries {
            if entry.matches(dest_ip) {
                return Some(entry);
            }
        }
        None
    }

    /// 删除路由表项
    pub fn remove_route(&mut self, destination: Ipv4Address, prefix_len: u8) -> KernelResult<()> {
        let initial_len = self.entries.len();
        self.entries.retain(|e| !(e.destination == destination && e.prefix_len == prefix_len));

        if self.entries.len() == initial_len {
            Err(crate::prelude::KernelError::NotFound)
        } else {
            Ok(())
        }
    }

    /// 获取路由表大小
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 检查路由表是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空路由表
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================
// 路由表自测
// ============================================================

pub fn selftest() -> bool {
    let mut table = RoutingTable::new();

    // 添加默认路由
    let default_route = RouteEntry::new(
        Ipv4Address::from_parts(0, 0, 0, 0),
        0,
        Ipv4Address::from_parts(192, 168, 1, 1),
        100,
        0,
    );
    table.add_route(default_route).unwrap_or_else(|_| {
        panic!("添加默认路由失败");
    });

    // 添加特定网络路由
    let network_route = RouteEntry::new(
        Ipv4Address::from_parts(10, 0, 0, 0),
        8,
        Ipv4Address::from_parts(192, 168, 1, 2),
        10,
        1,
    );
    table.add_route(network_route).unwrap_or_else(|_| {
        panic!("添加网络路由失败");
    });

    // 测试查询
    let test_ip = Ipv4Address::from_parts(10, 1, 2, 3);
    let route = table.lookup(test_ip).expect("路由查询失败");
    assert_eq!(route.prefix_len, 8, "应该匹配 /8 路由");

    // 测试默认路由
    let other_ip = Ipv4Address::from_parts(8, 8, 8, 8);
    let route = table.lookup(other_ip).expect("默认路由查询失败");
    assert_eq!(route.prefix_len, 0, "应该匹配默认路由");

    assert_eq!(table.len(), 2, "路由表项数不正确");

    true
}
