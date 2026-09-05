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
use crate::prelude::KernelError;
use crate::sync::Spinlock;

/// IP 分片表项（用于重组）
#[derive(Debug, Clone)]
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
    /// 已接收的分片数据（按偏移排序：offset → data）
    pub fragments: alloc::vec::Vec<(u16, alloc::vec::Vec<u8>)>,
    /// 期望的总长度（最后一个分片的 offset + length）
    pub expected_length: Option<u16>,
}

impl FragmentEntry {
    /// 创建新的分片条目
    pub fn new(src_ip: [u8; 4], dst_ip: [u8; 4], id: u16, protocol: u8, timestamp: u64) -> Self {
        FragmentEntry {
            src_ip,
            dst_ip,
            id,
            protocol,
            timestamp,
            fragments: alloc::vec::Vec::new(),
            expected_length: None,
        }
    }

    /// 检查是否过期
    pub fn is_expired(&self, current_time: u64) -> bool {
        (current_time - self.timestamp) > (crate::net::config::IP_FRAGMENT_TIMEOUT as u64)
    }

    /// 添加分片
    /// 返回 Some(完整包数据) 当所有分片都已接收
    pub fn add_fragment(
        &mut self,
        offset: u16,
        data: &[u8],
        more_fragments: bool,
    ) -> Option<alloc::vec::Vec<u8>> {
        // 如果没有更多分片，记录总长度
        if !more_fragments {
            self.expected_length = Some(offset + data.len() as u16);
        }

        // 添加分片数据
        self.fragments.push((offset, data.to_vec()));

        // 按 offset 排序（便于重组）
        self.fragments.sort_by_key(|f| f.0);

        // 检查是否收到了所有分片
        if let Some(total_len) = self.expected_length {
            if Self::is_complete(&self.fragments, total_len) {
                return Some(Self::reassemble(&self.fragments, total_len));
            }
        }

        None
    }

    /// 检查分片是否完整
    fn is_complete(fragments: &[(u16, alloc::vec::Vec<u8>)], expected_length: u16) -> bool {
        let mut current_offset = 0u16;

        for (offset, data) in fragments {
            // 检查是否有间隙
            if *offset != current_offset {
                return false;
            }
            current_offset += data.len() as u16;
        }

        current_offset == expected_length
    }

    /// 重组分片数据
    fn reassemble(fragments: &[(u16, alloc::vec::Vec<u8>)], _expected_length: u16) -> alloc::vec::Vec<u8> {
        let mut result = alloc::vec::Vec::new();

        for (_offset, data) in fragments {
            result.extend_from_slice(data);
        }

        result
    }
}

/// 全局分片重组表
static FRAGMENT_TABLE: Spinlock<FragmentReassemblyTable> = Spinlock::new(FragmentReassemblyTable::new());

/// 分片重组表
pub struct FragmentReassemblyTable {
    /// 分片条目列表
    entries: alloc::vec::Vec<FragmentEntry>,
}

impl FragmentReassemblyTable {
    /// 创建空的重组表
    pub const fn new() -> Self {
        FragmentReassemblyTable {
            entries: alloc::vec::Vec::new(),
        }
    }

    /// 处理分片
    pub fn add_fragment(
        &mut self,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        id: u16,
        protocol: u8,
        more_fragments: bool,
        fragment_offset: u16,
        data: &[u8],
        current_time: u64,
    ) -> KernelResult<Option<alloc::vec::Vec<u8>>> {
        // 查找或创建分片条目
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.src_ip == src_ip && e.dst_ip == dst_ip && e.id == id);

        if let Some(entry) = entry {
            // 检查是否过期
            if entry.is_expired(current_time) {
                // 过期了，删除并创建新的
                self.entries.retain(|e| !(e.src_ip == src_ip && e.dst_ip == dst_ip && e.id == id));
                return self.add_fragment(
                    src_ip,
                    dst_ip,
                    id,
                    protocol,
                    more_fragments,
                    fragment_offset,
                    data,
                    current_time,
                );
            }

            // 添加分片
            if let Some(complete_data) = entry.add_fragment(fragment_offset, data, more_fragments) {
                // 分片完成，删除条目
                self.entries.retain(|e| !(e.src_ip == src_ip && e.dst_ip == dst_ip && e.id == id));
                return Ok(Some(complete_data));
            }
        } else {
            // 创建新条目
            if self.entries.len() >= crate::net::config::IP_FRAGMENT_TABLE_MAX {
                return Err(KernelError::OutOfMemory);
            }

            let mut entry = FragmentEntry::new(src_ip, dst_ip, id, protocol, current_time);
            if let Some(complete_data) = entry.add_fragment(fragment_offset, data, more_fragments) {
                // 分片完成（只有一个分片）
                return Ok(Some(complete_data));
            }

            self.entries.push(entry);
        }

        Ok(None)
    }

    /// 清理过期分片
    pub fn cleanup_expired(&mut self, current_time: u64) {
        self.entries.retain(|e| !e.is_expired(current_time));
    }

    /// 获取表大小
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 处理 IP 分片（全局接口）
pub fn handle_fragment(
    src_ip: &[u8; 4],
    dst_ip: &[u8; 4],
    id: u16,
    protocol: u8,
    more_fragments: bool,
    fragment_offset: u16,
    data: &[u8],
    current_time: u64,
) -> KernelResult<Option<alloc::vec::Vec<u8>>> {
    let mut table = FRAGMENT_TABLE.lock();
    table.add_fragment(
        *src_ip,
        *dst_ip,
        id,
        protocol,
        more_fragments,
        fragment_offset,
        data,
        current_time,
    )
}

/// 清理过期分片
pub fn cleanup_expired(current_time: u64) {
    let mut table = FRAGMENT_TABLE.lock();
    table.cleanup_expired(current_time);
}

/// IP 分片自测
pub fn selftest() -> bool {
    // 测试单个分片（无需重组）
    let mut table = FragmentReassemblyTable::new();
    let result = table
        .add_fragment(
            [192, 168, 1, 1],
            [192, 168, 1, 2],
            12345,
            6,  // TCP
            false,  // 无更多分片
            0,
            b"Complete packet",
            0,
        )
        .expect("add_fragment failed");

    assert!(result.is_some(), "Single fragment should complete immediately");
    assert_eq!(result.unwrap(), b"Complete packet");

    // 测试多个分片
    let mut table = FragmentReassemblyTable::new();

    // 添加第一个分片
    let result1 = table
        .add_fragment(
            [192, 168, 1, 1],
            [192, 168, 1, 2],
            12346,
            6,
            true,  // 有更多分片
            0,
            b"First",
            0,
        )
        .expect("add_fragment failed");
    assert!(result1.is_none(), "First fragment should not complete");

    // 添加第二个分片
    let result2 = table
        .add_fragment(
            [192, 168, 1, 1],
            [192, 168, 1, 2],
            12346,
            6,
            false,  // 最后一个分片
            5,
            b"Second",
            0,
        )
        .expect("add_fragment failed");

    assert!(result2.is_some(), "All fragments should complete");
    let complete = result2.unwrap();
    assert_eq!(&complete[..], b"FirstSecond");

    true
}
