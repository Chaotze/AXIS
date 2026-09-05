// ============================================================
// TCP 协议（Transmission Control Protocol）
// ============================================================
// 实现 RFC 793 TCP 协议
//
// TCP 包头结构（20 字节最小）：
//   [SrcPort(16) | DstPort(16) | SeqNum(32) | AckNum(32) |
//    DataOffset(4) | Reserved(3) | Flags(9) | Window(16) | Checksum(16) |
//    UrgPtr(16) | Options(0-40)]
//
// TCP 状态机：
//   CLOSED → LISTEN → (SYN_RCVD → ESTABLISHED)
//   CLOSED → SYN_SENT → ESTABLISHED
//   ESTABLISHED → FIN_WAIT_1 / CLOSE_WAIT → ... → CLOSED

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use crate::net::config::{tcp_flags, TcpState};

// ============================================================
// TCP 包头
// ============================================================

/// TCP 包头（最小 20 字节）
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TcpHeader {
    /// 源端口（大端）
    pub src_port: [u8; 2],
    /// 目标端口（大端）
    pub dst_port: [u8; 2],
    /// 序列号（大端）
    pub seq_num: [u8; 4],
    /// 确认号（大端）
    pub ack_num: [u8; 4],
    /// 数据偏移(4) + 保留(4)
    pub data_offset_reserved: u8,
    /// 控制标志
    pub flags: u8,
    /// 窗口大小（大端）
    pub window: [u8; 2],
    /// 校验和（大端）
    pub checksum: [u8; 2],
    /// 紧急指针（大端）
    pub urgent_ptr: [u8; 2],
}

impl TcpHeader {
    /// 创建新的 TCP 包头
    pub fn new(src_port: u16, dst_port: u16, seq_num: u32, flags: u8) -> Self {
        TcpHeader {
            src_port: src_port.to_be_bytes(),
            dst_port: dst_port.to_be_bytes(),
            seq_num: seq_num.to_be_bytes(),
            ack_num: [0, 0, 0, 0],
            data_offset_reserved: 0x50,  // 数据偏移 5 (20 字节)
            flags,
            window: [0xFF, 0xFF],  // 最大窗口
            checksum: [0, 0],
            urgent_ptr: [0, 0],
        }
    }

    /// 获取源端口
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }

    /// 获取目标端口
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }

    /// 获取序列号
    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(self.seq_num)
    }

    /// 获取确认号
    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }

    /// 获取数据偏移（32 位字为单位）
    pub fn data_offset(&self) -> u8 {
        (self.data_offset_reserved >> 4) & 0x0F
    }

    /// 获取包头长度（字节）
    pub fn header_length(&self) -> usize {
        (self.data_offset() as usize) * 4
    }

    /// 获取窗口大小
    pub fn window(&self) -> u16 {
        u16::from_be_bytes(self.window)
    }

    /// 检查 SYN 标志
    pub fn has_syn(&self) -> bool {
        (self.flags & tcp_flags::SYN) != 0
    }

    /// 检查 ACK 标志
    pub fn has_ack(&self) -> bool {
        (self.flags & tcp_flags::ACK) != 0
    }

    /// 检查 FIN 标志
    pub fn has_fin(&self) -> bool {
        (self.flags & tcp_flags::FIN) != 0
    }

    /// 检查 RST 标志
    pub fn has_rst(&self) -> bool {
        (self.flags & tcp_flags::RST) != 0
    }

    /// 从字节数组解析 TCP 包头
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 20 {
            return Err(KernelError::InvalidArgument);
        }

        let header = unsafe {
            *(data.as_ptr() as *const TcpHeader)
        };
        Ok(header)
    }

    /// 转换为字节数组（仅首部）
    pub fn to_bytes(&self) -> [u8; 20] {
        let mut bytes = [0u8; 20];
        bytes[0..2].copy_from_slice(&self.src_port);
        bytes[2..4].copy_from_slice(&self.dst_port);
        bytes[4..8].copy_from_slice(&self.seq_num);
        bytes[8..12].copy_from_slice(&self.ack_num);
        bytes[12] = self.data_offset_reserved;
        bytes[13] = self.flags;
        bytes[14..16].copy_from_slice(&self.window);
        bytes[16..18].copy_from_slice(&self.checksum);
        bytes[18..20].copy_from_slice(&self.urgent_ptr);
        bytes
    }
}

// ============================================================
// TCP 连接管理
// ============================================================

/// TCP 拥塞控制状态
/// 为什么需要拥塞控制：防止网络过载，提高吞吐量
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionControlAlgorithm {
    /// 慢启动（Slow Start）
    SlowStart,
    /// 拥塞避免（Congestion Avoidance）
    CongestionAvoidance,
    /// 快速恢复（Fast Recovery）
    FastRecovery,
}

/// 拥塞控制参数
#[derive(Debug, Clone, Copy)]
pub struct CongestionControl {
    /// 拥塞窗口（字节）
    pub cwnd: u32,
    /// 慢启动阈值（字节）
    pub ssthresh: u32,
    /// 当前算法
    pub algorithm: CongestionControlAlgorithm,
    /// 重复 ACK 计数
    pub dup_ack_count: u32,
}

impl CongestionControl {
    /// 创建新的拥塞控制器
    pub fn new() -> Self {
        // 初始 cwnd 为 10 个 MSS（最大段大小），假设 MSS 为 1460 字节
        let mss = 1460u32;
        CongestionControl {
            cwnd: 10 * mss,
            ssthresh: 65535,  // 初始 ssthresh 设为 64KB
            algorithm: CongestionControlAlgorithm::SlowStart,
            dup_ack_count: 0,
        }
    }

    /// 处理数据被确认（ACK）
    /// 为什么需要：收到 ACK 时增加拥塞窗口
    pub fn on_ack(&mut self, _bytes_acked: u32) {
        match self.algorithm {
            CongestionControlAlgorithm::SlowStart => {
                // 慢启动：每收到一个 ACK 就增加 1 个 MSS
                let mss = 1460u32;
                self.cwnd += mss;

                // 当 cwnd >= ssthresh 时进入拥塞避免
                if self.cwnd >= self.ssthresh {
                    self.algorithm = CongestionControlAlgorithm::CongestionAvoidance;
                }
            }
            CongestionControlAlgorithm::CongestionAvoidance => {
                // 拥塞避免：每收到 cwnd 字节的数据后才增加 1 个 MSS
                // 简化实现：每个 ACK 增加 MSS/cwnd
                let mss = 1460u32;
                self.cwnd += mss / self.cwnd.max(1);
            }
            CongestionControlAlgorithm::FastRecovery => {
                // 快速恢复：收到新数据的 ACK 后回到拥塞避免
                self.algorithm = CongestionControlAlgorithm::CongestionAvoidance;
                self.dup_ack_count = 0;
            }
        }
    }

    /// 处理丢包（超时或三个重复 ACK）
    /// 为什么需要：检测到丢包时减少窗口
    pub fn on_loss(&mut self) {
        let mss = 1460u32;

        // 设置新的 ssthresh
        self.ssthresh = (self.cwnd / 2).max(2 * mss);

        // 进入快速恢复
        self.cwnd = self.ssthresh + 3 * mss;
        self.algorithm = CongestionControlAlgorithm::FastRecovery;
        self.dup_ack_count = 0;
    }

    /// 处理重复 ACK
    /// 为什么需要：三个重复 ACK 表示丢包
    pub fn on_dup_ack(&mut self) {
        self.dup_ack_count += 1;

        if self.dup_ack_count == 3 {
            // 触发快速重传
            self.on_loss();
        }
    }

    /// 获取当前允许发送的最大数据量
    pub fn get_max_sendable(&self) -> u32 {
        self.cwnd
    }
}

/// TCP 重传状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetransmitState {
    /// 待重传数据的序列号
    pub seq_num: u32,
    /// 重传次数
    pub retransmit_count: usize,
    /// 最后重传时间（毫秒时间戳）
    pub last_retransmit_time: u64,
    /// RTO（重传超时时间，毫秒）
    pub rto_ms: u32,
}

// ============================================================
// TCP 连接管理
// ============================================================

/// TCP 连接
#[derive(Debug, Clone)]
pub struct TcpConnection {
    /// 源 IP
    pub src_ip: [u8; 4],
    /// 目标 IP
    pub dst_ip: [u8; 4],
    /// 源端口
    pub src_port: u16,
    /// 目标端口
    pub dst_port: u16,
    /// 连接状态
    pub state: TcpState,
    /// 发送序列号
    pub snd_seq: u32,
    /// 接收序列号
    pub rcv_seq: u32,
    /// 接收缓冲区
    pub recv_buffer: alloc::vec::Vec<u8>,
    /// 发送缓冲区
    pub send_buffer: alloc::vec::Vec<u8>,
    /// TCP 发送窗口大小
    pub snd_wnd: u32,
    /// TCP 接收窗口大小
    pub rcv_wnd: u32,
    /// 最后一个已确认的序列号
    pub last_acked_seq: u32,
    /// 最后一个已发送的序列号
    pub last_sent_seq: u32,
}

impl TcpConnection {
    /// 创建新的 TCP 连接
    /// 使用梅森旋转算法生成随机初始序列号（ISS）
    /// 为什么需要随机 ISS：RFC 793 要求防止旧连接的数据被新连接误认
    pub fn new(
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Self {
        // 使用全局随机数生成器获取随机初始序列号
        // 取 u64 的低 32 位作为 TCP 序列号
        let snd_seq = (crate::lib::random::random_u64() as u32).wrapping_add(1);

        TcpConnection {
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            state: TcpState::Closed,
            snd_seq,
            rcv_seq: 0,
            recv_buffer: alloc::vec::Vec::new(),
            send_buffer: alloc::vec::Vec::new(),
            snd_wnd: crate::net::config::TCP_RX_WINDOW_SIZE,  // 初始化为 65535
            rcv_wnd: crate::net::config::TCP_RX_WINDOW_SIZE,
            last_acked_seq: 0,
            last_sent_seq: 0,
        }
    }

    /// 获取连接标识（用于查找）
    /// 为什么用四元组：唯一标识一条TCP连接
    pub fn tuple(&self) -> ([u8; 4], u16, [u8; 4], u16) {
        (self.src_ip, self.src_port, self.dst_ip, self.dst_port)
    }

    /// 主动发起连接（客户端 SYN）
    /// 为什么需要：支持客户端主动发起TCP连接
    pub fn initiate_connection(&mut self) -> KernelResult<()> {
        if self.state != TcpState::Closed {
            return Err(KernelError::InvalidArgument);
        }

        // 转换到 SYN_SENT 状态，等待服务器的 SYN+ACK
        self.state = TcpState::SynSent;
        // snd_seq 已在创建时初始化
        Ok(())
    }

    /// 处理服务器的 SYN+ACK（客户端侧）
    /// 为什么需要：第二次握手的处理
    pub fn handle_synack(&mut self, ack_num: u32, recv_seq: u32) -> KernelResult<()> {
        if self.state != TcpState::SynSent {
            return Err(KernelError::InvalidArgument);
        }

        // 验证 ACK 号是否正确（应该是 snd_seq + 1）
        if ack_num != self.snd_seq + 1 {
            return Err(KernelError::InvalidArgument);
        }

        // 记录对方的初始序列号
        self.rcv_seq = recv_seq;
        // 准备发送 ACK
        self.snd_seq += 1;
        // 转换到 ESTABLISHED 状态
        self.state = TcpState::Established;
        Ok(())
    }

    /// 监听连接（服务器）
    /// 为什么需要：服务器端进入监听状态
    pub fn listen(&mut self) -> KernelResult<()> {
        if self.state != TcpState::Closed {
            return Err(KernelError::InvalidArgument);
        }

        self.state = TcpState::Listen;
        Ok(())
    }

    /// 处理客户端的 SYN（服务器侧）
    /// 为什么需要：第一次握手的处理
    pub fn handle_syn(&mut self, recv_seq: u32) -> KernelResult<()> {
        if self.state != TcpState::Listen {
            return Err(KernelError::InvalidArgument);
        }

        // 记录客户端的初始序列号
        self.rcv_seq = recv_seq;
        // 转换到 SYN_RCVD 状态，准备发送 SYN+ACK
        self.state = TcpState::SynRecvd;
        // snd_seq 已在创建时初始化
        Ok(())
    }

    /// 处理客户端的 ACK（服务器侧）
    /// 为什么需要：第三次握手的处理
    pub fn handle_ack(&mut self, ack_num: u32) -> KernelResult<()> {
        if self.state != TcpState::SynRecvd {
            return Err(KernelError::InvalidArgument);
        }

        // 验证 ACK 号是否正确（应该是 snd_seq + 1）
        if ack_num != self.snd_seq + 1 {
            return Err(KernelError::InvalidArgument);
        }

        // 更新发送序列号
        self.snd_seq += 1;
        // 连接建立
        self.state = TcpState::Established;
        Ok(())
    }

    /// 存储接收到的数据
    pub fn store_received_data(&mut self, data: &[u8]) {
        self.recv_buffer.extend_from_slice(data);
    }

    /// 获取接收缓冲区
    pub fn get_recv_buffer(&self) -> &[u8] {
        &self.recv_buffer
    }

    /// 清空接收缓冲区
    pub fn clear_recv_buffer(&mut self) {
        self.recv_buffer.clear();
    }

    /// 发起连接关闭（FIN）
    pub fn close_connection(&mut self) -> KernelResult<()> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                Ok(())
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                Ok(())
            }
            _ => Err(KernelError::InvalidArgument),
        }
    }

    /// 处理对方的 FIN
    pub fn handle_fin(&mut self) -> KernelResult<()> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::CloseWait;
                Ok(())
            }
            TcpState::FinWait1 | TcpState::FinWait2 => {
                self.state = TcpState::TimeWait;
                Ok(())
            }
            _ => Err(KernelError::InvalidArgument),
        }
    }

    /// 更新接收窗口大小
    /// 为什么需要：流量控制，告诉对方可以发送多少数据
    pub fn update_rcv_window(&mut self, available_space: u32) {
        self.rcv_wnd = available_space.min(crate::net::config::TCP_RX_WINDOW_SIZE);
    }

    /// 获取发送窗口大小
    /// 为什么需要：检查是否可以发送数据
    pub fn get_snd_wnd(&self) -> u32 {
        self.snd_wnd
    }

    /// 更新发送窗口（从对方的 ACK 中提取）
    /// 为什么需要：对方告诉我们可以发送多少数据
    pub fn update_snd_wnd(&mut self, window_size: u16) {
        self.snd_wnd = window_size as u32;
    }

    /// 检查是否可以发送数据
    /// 为什么需要：发送窗口不能为 0（接收方已满）
    pub fn can_send_data(&self) -> bool {
        self.snd_wnd > 0 && !self.send_buffer.is_empty()
    }

    /// 计算可发送的数据量
    /// 为什么需要：不能超过对方的接收窗口
    pub fn get_sendable_bytes(&self) -> usize {
        let unsent = self.send_buffer.len() as u32;
        let can_send = self.snd_wnd.saturating_sub(
            self.last_sent_seq.saturating_sub(self.last_acked_seq)
        );
        (unsent.min(can_send)) as usize
    }

    /// 处理 ACK，更新已确认的序列号
    /// 为什么需要：接收方确认了数据，可以删除发送缓冲区中的已确认数据
    pub fn handle_received_ack(&mut self, ack_num: u32) -> KernelResult<()> {
        // 检查 ACK 序列号是否有效
        if ack_num > self.snd_seq && ack_num <= self.last_sent_seq.wrapping_add(1) {
            self.last_acked_seq = ack_num;
            Ok(())
        } else {
            Err(KernelError::InvalidArgument)
        }
    }

    /// 标记需要重传
    /// 为什么需要：当没有收到 ACK 时，需要重传数据
    pub fn mark_for_retransmit(&mut self, current_time: u64) -> RetransmitState {
        RetransmitState {
            seq_num: self.last_acked_seq + 1,
            retransmit_count: 0,
            last_retransmit_time: current_time,
            rto_ms: crate::net::config::TCP_RETRANSMIT_TIMEOUT,
        }
    }

    /// 检查是否应该重传
    /// 为什么需要：根据 RTO 和重传次数决定是否重新发送
    pub fn should_retransmit(retransmit: &RetransmitState, current_time: u64) -> bool {
        let elapsed = current_time.saturating_sub(retransmit.last_retransmit_time);

        // 检查是否超过 RTO 且还未达到最大重传次数
        elapsed >= (retransmit.rto_ms as u64) &&
        retransmit.retransmit_count < crate::net::config::TCP_MAX_RETRANSMIT
    }

    /// 更新重传状态
    /// 为什么需要：每次重传时更新计数和时间
    pub fn update_retransmit(retransmit: &mut RetransmitState, current_time: u64) {
        retransmit.retransmit_count += 1;
        retransmit.last_retransmit_time = current_time;

        // 指数退避：每次重传时将 RTO 加倍（最大到 60 秒）
        retransmit.rto_ms = (retransmit.rto_ms * 2).min(60000);
    }
}

// ============================================================
// TCP 连接表全局管理
// ============================================================

use crate::sync::Spinlock;

/// 全局 TCP 连接表
static TCP_CONNECTIONS: Spinlock<TcpConnectionTable> = Spinlock::new(TcpConnectionTable::new());

/// TCP 连接表结构
pub struct TcpConnectionTable {
    /// 连接列表（四元组 → 连接状态）
    connections: alloc::vec::Vec<TcpConnection>,
}

impl TcpConnectionTable {
    /// 创建空的连接表
    pub const fn new() -> Self {
        TcpConnectionTable {
            connections: alloc::vec::Vec::new(),
        }
    }

    /// 添加新连接
    /// 为什么需要：管理所有活跃的 TCP 连接
    pub fn add(&mut self, conn: TcpConnection) -> KernelResult<()> {
        if self.connections.len() >= crate::net::config::TCP_CONNECTION_MAX {
            return Err(KernelError::OutOfMemory);
        }

        // 检查是否已存在相同的四元组
        if self.connections.iter().any(|c| c.tuple() == conn.tuple()) {
            return Err(KernelError::InvalidArgument);  // 连接已存在
        }

        self.connections.push(conn);
        Ok(())
    }

    /// 查找连接（按四元组）
    pub fn find(&self, tuple: ([u8; 4], u16, [u8; 4], u16)) -> Option<&TcpConnection> {
        self.connections.iter().find(|c| c.tuple() == tuple)
    }

    /// 查找连接（可变引用）
    pub fn find_mut(&mut self, tuple: ([u8; 4], u16, [u8; 4], u16)) -> Option<&mut TcpConnection> {
        self.connections.iter_mut().find(|c| c.tuple() == tuple)
    }

    /// 移除连接
    pub fn remove(&mut self, tuple: ([u8; 4], u16, [u8; 4], u16)) -> KernelResult<()> {
        let initial_len = self.connections.len();
        self.connections.retain(|c| c.tuple() != tuple);

        if self.connections.len() == initial_len {
            Err(KernelError::NotFound)
        } else {
            Ok(())
        }
    }

    /// 按端口对查找连接（遍历所有连接）
    pub fn find_by_ports(&self, src_port: u16, dst_port: u16) -> Option<TcpConnection> {
        self.connections
            .iter()
            .find(|c| c.src_port == src_port && c.dst_port == dst_port)
            .cloned()
    }

    /// 获取连接数
    pub fn len(&self) -> usize {
        self.connections.len()
    }
}

/// 全局连接表访问接口
pub fn add_connection(conn: TcpConnection) -> KernelResult<()> {
    let mut table = TCP_CONNECTIONS.lock();
    table.add(conn)
}

pub fn find_connection(tuple: ([u8; 4], u16, [u8; 4], u16)) -> Option<TcpConnection> {
    let table = TCP_CONNECTIONS.lock();
    table.find(tuple).cloned()
}

pub fn update_connection<F>(tuple: ([u8; 4], u16, [u8; 4], u16), f: F) -> KernelResult<()>
where
    F: FnOnce(&mut TcpConnection) -> KernelResult<()>,
{
    let mut table = TCP_CONNECTIONS.lock();
    if let Some(conn) = table.find_mut(tuple) {
        f(conn)
    } else {
        Err(KernelError::NotFound)
    }
}

pub fn remove_connection(tuple: ([u8; 4], u16, [u8; 4], u16)) -> KernelResult<()> {
    let mut table = TCP_CONNECTIONS.lock();
    table.remove(tuple)
}
/// 为什么分离发送：便于连接管理层调用
pub fn send_packet(
    src_port: u16,
    dst_port: u16,
    data: &[u8],
) -> KernelResult<usize> {
    // 从连接表查找真实的目标 IP
    // 为什么需要：send_packet 只有端口信息，需要从连接表获取完整四元组
    let found_conn = {
        let conns = TCP_CONNECTIONS.lock();
        conns.find_by_ports(src_port, dst_port)
    };

    if let Some(conn) = found_conn {
        // 创建 TCP 包头
        let mut header = TcpHeader::new(src_port, dst_port, conn.snd_seq, tcp_flags::ACK);
        header.ack_num = conn.rcv_seq.to_be_bytes();
        header.window = (conn.rcv_wnd as u16).to_be_bytes();

        // 构建完整 TCP 包
        let mut packet = alloc::vec::Vec::with_capacity(20 + data.len());
        packet.extend_from_slice(&header.to_bytes());
        packet.extend_from_slice(data);

        // 调用 IP 层发送（使用连接中的真实目标 IP）
        let _ = super::super::ip::send_packet(
            &conn.dst_ip,
            crate::net::config::ip_protocol::TCP,
            &packet,
        )?;

        Ok(data.len())
    } else {
        Err(KernelError::NotFound)
    }
}

/// 接收 TCP 数据包
/// 为什么需要此函数：IP 层识别协议号后分发给 TCP 处理
///
/// 注意：在完整实现中，此函数应该从 IP 层接收源和目标 IP 地址
/// 为什么：TCP 四元组 = (src_ip, src_port, dst_ip, dst_port)
pub fn recv_packet(data: &[u8]) -> KernelResult<()> {
    let header = TcpHeader::from_bytes(data)?;

    // 获取包头信息
    let src_port = header.src_port();
    let dst_port = header.dst_port();
    let seq_num = header.seq_num();
    let ack_num = header.ack_num();

    // TODO: 改进方案
    // 应该从 IP 层获取源和目标 IP 地址
    // 当前为简化实现，使用默认本机地址
    let src_ip = [0, 0, 0, 0];  // 应该从 IP 层获取
    let dst_ip = [0, 0, 0, 0];  // 应该从 IP 层获取

    let tuple = (src_ip, src_port, dst_ip, dst_port);

    // 查询连接表（使用四元组）
    // 为什么需要查询：找到对应的连接状态
    if let Some(_conn) = find_connection(tuple) {
        // 获取有效负载（跳过 TCP 头）
        let payload_start = header.header_length();
        let payload = if data.len() > payload_start {
            &data[payload_start..]
        } else {
            &[]
        };

        // 根据连接状态和标志位处理（状态机）
        // 为什么需要状态机：TCP 连接有多个状态，不同状态下处理包的方式不同
        update_connection(tuple, |conn| {
            match conn.state {
                crate::net::config::TcpState::Listen => {
                    // 监听状态：应该收到 SYN
                    if header.has_syn() {
                        // 处理客户端的 SYN
                        conn.handle_syn(seq_num)?;

                        // 转换到 SYN_RCVD 状态，准备发送 SYN+ACK
                    }
                    Ok(())
                }
                crate::net::config::TcpState::SynSent => {
                    // SYN_SENT 状态：应该收到 SYN+ACK
                    if header.has_syn() && header.has_ack() {
                        conn.handle_synack(ack_num, seq_num)?;
                        // 转换到 ESTABLISHED 状态
                    }
                    Ok(())
                }
                crate::net::config::TcpState::SynRecvd => {
                    // SYN_RCVD 状态：应该收到 ACK
                    if header.has_ack() {
                        conn.handle_ack(ack_num)?;
                        // 转换到 ESTABLISHED 状态
                    }
                    Ok(())
                }
                crate::net::config::TcpState::Established => {
                    // ESTABLISHED 状态：可以接收数据或 FIN
                    if payload.len() > 0 {
                        // 存储接收到的数据
                        // 为什么需要存储：应用层稍后会读取此数据
                        conn.store_received_data(payload);
                    }

                    if header.has_fin() {
                        // 处理对方的 FIN
                        conn.handle_fin()?;
                        // 转换到 CLOSE_WAIT 状态
                    }

                    // 如果收到数据，需要发送 ACK
                    if payload.len() > 0 || header.has_fin() {
                        conn.rcv_seq = conn.rcv_seq.wrapping_add(payload.len() as u32);
                        // 后续可以通知应用层有新数据
                    }
                    Ok(())
                }
                crate::net::config::TcpState::FinWait1 | crate::net::config::TcpState::FinWait2 => {
                    // 关闭相关状态：处理对方的 FIN 或 ACK
                    if header.has_fin() {
                        conn.handle_fin()?;
                    }
                    if header.has_ack() {
                        conn.last_acked_seq = ack_num;
                    }
                    Ok(())
                }
                _ => {
                    // 其他状态的处理
                    Ok(())
                }
            }
        })?;
    } else {
        // 没有找到对应的连接
        // 可能需要发送 RST（重置）包来告知对方
        // 当前简化实现：忽略此包
    }

    Ok(())
}

/// 创建 SYN 包（TCP 连接请求）
pub fn create_syn(src_port: u16, dst_port: u16, seq_num: u32) -> TcpHeader {
    TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::SYN)
}

/// 创建 SYN+ACK 包（TCP 连接应答）
pub fn create_synack(src_port: u16, dst_port: u16, seq_num: u32, ack_num: u32) -> TcpHeader {
    let mut header = TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::SYN | tcp_flags::ACK);
    header.ack_num = ack_num.to_be_bytes();
    header
}

/// 创建 ACK 包
pub fn create_ack(src_port: u16, dst_port: u16, seq_num: u32, ack_num: u32) -> TcpHeader {
    let mut header = TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::ACK);
    header.ack_num = ack_num.to_be_bytes();
    header
}

/// 创建 FIN 包（TCP 连接关闭）
pub fn create_fin(src_port: u16, dst_port: u16, seq_num: u32) -> TcpHeader {
    TcpHeader::new(src_port, dst_port, seq_num, tcp_flags::FIN | tcp_flags::ACK)
}

// ============================================================
// TCP 自测
// ============================================================

pub fn selftest() -> bool {
    // 1. TCP 包头创建
    let header = TcpHeader::new(12345, 80, 1000, tcp_flags::SYN);
    assert_eq!(header.src_port(), 12345, "Source port mismatch");
    assert_eq!(header.dst_port(), 80, "Destination port mismatch");
    assert_eq!(header.seq_num(), 1000, "Sequence number mismatch");
    assert!(header.has_syn(), "SYN flag not set");

    // 2. SYN+ACK 包创建
    let synack = create_synack(80, 12345, 2000, 1001);
    assert!(synack.has_syn(), "SYN flag not set");
    assert!(synack.has_ack(), "ACK flag not set");
    assert_eq!(synack.ack_num(), 1001, "Acknowledgment number mismatch");

    // 3. 包头序列化和解析
    let bytes = header.to_bytes();
    let parsed = TcpHeader::from_bytes(&bytes).unwrap_or_else(|_| {
        panic!("TCP header parsing failed");
    });
    assert_eq!(parsed.src_port(), header.src_port(), "Parsed source port mismatch");

    // 4. TCP 连接创建（注意：初始 seq 现在随机化）
    let mut conn = TcpConnection::new(
        [192, 168, 1, 10],
        [8, 8, 8, 8],
        12345,
        80,
    );
    assert_eq!(conn.state, TcpState::Closed, "Initial state should be CLOSED");

    // 保存客户端的初始序列号以供后续验证
    let client_initial_seq = conn.snd_seq;

    // 5. 客户端三次握手测试
    // Step 1: 客户端发起 SYN
    conn.initiate_connection().unwrap_or_else(|_| {
        panic!("initiate_connection failed");
    });
    assert_eq!(conn.state, TcpState::SynSent, "Should transition to SYN_SENT");

    // Step 2: 客户端收到服务器的 SYN+ACK（seq=2000, ack=client_seq+1）
    conn.handle_synack(client_initial_seq + 1, 2000).unwrap_or_else(|_| {
        panic!("handle_synack failed");
    });
    assert_eq!(conn.state, TcpState::Established, "Should transition to ESTABLISHED");
    assert_eq!(conn.rcv_seq, 2000, "rcv_seq should be 2000");

    // 6. 服务器三次握手测试
    let mut server_conn = TcpConnection::new(
        [8, 8, 8, 8],
        [192, 168, 1, 10],
        80,
        12345,
    );

    // 保存服务器的初始序列号
    let server_initial_seq = server_conn.snd_seq;

    // Step 1: 服务器监听
    server_conn.listen().unwrap_or_else(|_| {
        panic!("listen failed");
    });
    assert_eq!(server_conn.state, TcpState::Listen, "Should be LISTEN");

    // Step 2: 服务器收到客户端的 SYN（seq=1000）
    server_conn.handle_syn(1000).unwrap_or_else(|_| {
        panic!("handle_syn failed");
    });
    assert_eq!(server_conn.state, TcpState::SynRecvd, "Should transition to SYN_RCVD");
    assert_eq!(server_conn.rcv_seq, 1000, "rcv_seq should be 1000");

    // Step 3: 服务器收到客户端的 ACK（ack=server_seq+1）
    server_conn.handle_ack(server_initial_seq + 1).unwrap_or_else(|_| {
        panic!("handle_ack failed");
    });
    assert_eq!(server_conn.state, TcpState::Established, "Should transition to ESTABLISHED");

    // 7. 数据存储和接收测试
    let test_data = b"Hello TCP";
    server_conn.store_received_data(test_data);
    assert_eq!(server_conn.get_recv_buffer(), test_data, "Received data mismatch");

    // 8. 连接关闭测试
    server_conn.close_connection().unwrap_or_else(|_| {
        panic!("close_connection failed");
    });
    assert_eq!(server_conn.state, TcpState::FinWait1, "Should transition to FIN_WAIT_1");

    true
}
