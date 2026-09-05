// ============================================================
// 网络协议栈（Network Protocol Stack）根模块
// ============================================================
// 聚合网络协议栈的所有层次实现，对外提供统一接口
//
// 分层架构（OSI 模型）：
//   链路层 (Link Layer) → IP 层 → 传输层 (Transport) → 应用层接口
//
// 模块结构：
//   - types.rs: 基础类型（MAC、IPv4、IPv6 地址等）
//   - link/: 链路层协议（以太网、ARP）
//   - ip/: IP 层协议（IPv4、IPv6、路由、ICMP、分片）
//   - transport/: 传输层协议（TCP、UDP、Socket）
//   - io_uring.rs: 异步 I/O 高性能接口
//   - config.rs: 网络栈配置参数
//
// 锁序约定：NET → PMM（物理内存分配）
// 中断纪律：网络包处理路径禁用中断 (irqsave)，避免中断路径与任务态访问同一资源
//
// 为什么分离出 types.rs：
// - MAC 地址、IP 地址等在多个协议中都需要使用
// - 集中管理避免重复定义和循环依赖

pub mod types;
pub mod config;
pub mod link;
pub mod ip;
pub mod transport;

// 异步 I/O 接口（高级功能，迭代开发）
pub mod io_uring;

// 重新导出常用类型
pub use types::{MacAddress, Ipv4Address, Ipv6Address};

use crate::prelude::KernelResult;
use crate::sync::Spinlock;

// ============================================================
// 网络协议栈全局状态
// ============================================================

/// 网络子系统状态
///
/// 为什么用 Box：NetworkState 包含多个路由表、连接表等
/// 动态数据结构，Box 让它在堆上落位，锁内只存指针
struct NetworkState {
    /// ARP 缓存表
    _arp_cache: alloc::vec::Vec<u8>,  // 占位符，后续实现
    /// IPv4 路由表
    _routing_table_v4: alloc::vec::Vec<u8>,  // 占位符
    /// IPv6 路由表
    _routing_table_v6: alloc::vec::Vec<u8>,  // 占位符
    /// TCP 连接表
    _tcp_connections: alloc::vec::Vec<u8>,  // 占位符
    /// UDP 套接字表
    _udp_sockets: alloc::vec::Vec<u8>,  // 占位符
}

/// 全局网络状态
static NET_STATE: Spinlock<Option<alloc::boxed::Box<NetworkState>>> = Spinlock::new(None);

// ============================================================
// 网络栈初始化
// ============================================================

/// 网络子系统初始化入口
pub fn init() {
    println!("[NET] Network subsystem initializing...");

    // 初始化全局网络状态
    let mut guard = NET_STATE.lock();
    let state = alloc::boxed::Box::new(NetworkState {
        _arp_cache: alloc::vec::Vec::new(),
        _routing_table_v4: alloc::vec::Vec::new(),
        _routing_table_v6: alloc::vec::Vec::new(),
        _tcp_connections: alloc::vec::Vec::new(),
        _udp_sockets: alloc::vec::Vec::new(),
    });
    *guard = Some(state);
    drop(guard);

    println!("[NET] Network subsystem ready");

    // 网络栈自测
    selftest();
}

// ============================================================
// 网络协议栈自测
// ============================================================

/// 运行全部网络栈自测
pub fn selftest() -> bool {
    println!("\n[NET-SELFTEST] Network Stack Selftest");
    let mut all = true;

    // 链路层测试
    all &= t("ethernet frame pack/unpack", link::selftest());
    all &= t("ARP protocol", link::arp::selftest());

    // IP 层测试
    all &= t("IPv4 packet handling", ip::ipv4::selftest());
    all &= t("IPv6 packet handling", ip::ipv6::selftest());
    all &= t("routing table management", ip::routing::selftest());
    all &= t("ICMP echo (ping)", ip::icmp::selftest());

    // 传输层测试
    all &= t("UDP protocol", transport::udp::selftest());
    all &= t("TCP protocol", transport::tcp::selftest());
    all &= t("Socket interface", transport::socket::selftest());

    println!("[NET-SELFTEST] Result: {}", if all { "ALL PASS" } else { "FAILED" });
    all
}

/// 单测结果记录和输出
fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}

// ============================================================
// 重新导出常用类型
// ============================================================

pub use link::{ethernet, arp};
pub use ip::{ipv4, ipv6, routing, icmp};
pub use transport::{socket, udp, tcp};
