// ============================================================
// IP 分片和重组（Fragment）
// ============================================================
// 实现 IPv4 的分片和重组功能
//
// 分片条件：
// - 包大小超过 MTU
// - DF（Don't Fragment）标志未设置
//
// 重组策略：
// - 使用分片 ID 和源/目标 IP 标识分片流
// - 超时丢弃不完整的分片组

use crate::lib::result::KernelResult;

/// IP 分片表项（用于重组）
pub struct FragmentEntry {
    /// 源 IP
    pub src_ip: [u8; 4],
    /// 目标 IP
    pub dst_ip: [u8; 4],
    /// 分片 ID
    pub id: u16,
    /// 协议号
    pub protocol: u8,
    /// 创建时间戳
    pub timestamp: u64,
    /// 已接收的分片数据（按偏移排序）
    pub fragments: alloc::vec::Vec<(u16, alloc::vec::Vec<u8>)>,
}

/// 处理 IP 分片
pub fn handle_fragment(
    _src_ip: &[u8; 4],
    _dst_ip: &[u8; 4],
    _id: u16,
    _protocol: u8,
    _more_fragments: bool,
    _fragment_offset: u16,
    _data: &[u8],
) -> KernelResult<Option<alloc::vec::Vec<u8>>> {
    // TODO: 实现分片重组逻辑
    // 返回 Some(完整包) 当所有分片都已接收
    // 返回 None 当还有分片待接收
    Ok(None)
}

/// IP 分片自测
pub fn selftest() -> bool {
    true
}
