// ============================================================
// Socket 接口
// ============================================================
// 实现 POSIX Socket API 的内核部分
//
// Socket 类型：
//   - SOCK_STREAM (TCP): 面向连接，可靠
//   - SOCK_DGRAM (UDP): 无连接，不可靠
//
// Socket 操作：
//   - socket(): 创建套接字
//   - bind(): 绑定本地地址
//   - listen(): 监听连接请求
//   - accept(): 接受连接
//   - connect(): 发起连接
//   - send()/recv(): 数据收发
//   - close(): 关闭套接字
//
// 为什么分离 socket 模块：
// - Socket 是应用层与内核网络栈的接口
// - 独立模块便于实现和测试
// - 可支持多种协议（不仅 TCP/UDP）

use crate::lib::result::KernelResult;
use crate::prelude::KernelError;
use alloc::vec::Vec;
use super::{tcp, udp};

// ============================================================
// Socket 类型定义
// ============================================================

/// Socket 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    /// 流式套接字（TCP）
    Stream = 1,
    /// 数据报套接字（UDP）
    Dgram = 2,
}

/// Socket 地址族
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    /// IPv4
    Inet = 2,
    /// IPv6
    Inet6 = 10,
}

/// Socket 地址（IPv4）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketAddr {
    /// 地址族
    pub family: AddressFamily,
    /// 端口号（大端）
    pub port: u16,
    /// IP 地址
    pub addr: [u8; 4],
}

impl SocketAddr {
    /// 创建新的 Socket 地址
    pub fn new(addr: [u8; 4], port: u16) -> Self {
        SocketAddr {
            family: AddressFamily::Inet,
            port,
            addr,
        }
    }

    /// 任意地址 (0.0.0.0)
    pub fn any(port: u16) -> Self {
        SocketAddr::new([0, 0, 0, 0], port)
    }

    /// 环回地址 (127.0.0.1)
    pub fn loopback(port: u16) -> Self {
        SocketAddr::new([127, 0, 0, 1], port)
    }
}

// ============================================================
// Socket 结构
// ============================================================

/// Socket 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    /// 未初始化
    Closed,
    /// 已创建
    Created,
    /// 已绑定本地地址
    Bound,
    /// 监听中（TCP）
    Listening,
    /// 连接中（TCP）
    Connecting,
    /// 已连接（TCP）
    Connected,
    /// 关闭中
    Closing,
}

/// Socket 描述符
#[derive(Debug, Clone)]
pub struct SocketDescriptor {
    /// Socket 编号
    pub fd: u32,
    /// 类型（TCP/UDP）
    pub socket_type: SocketType,
    /// 地址族
    pub family: AddressFamily,
    /// 当前状态
    pub state: SocketState,
    /// 本地地址
    pub local_addr: Option<SocketAddr>,
    /// 远程地址
    pub remote_addr: Option<SocketAddr>,
    /// 接收缓冲区
    pub recv_buffer: Vec<u8>,
    /// 发送缓冲区
    pub send_buffer: Vec<u8>,
}

impl SocketDescriptor {
    /// 创建新的 Socket
    pub fn new(fd: u32, socket_type: SocketType, family: AddressFamily) -> Self {
        SocketDescriptor {
            fd,
            socket_type,
            family,
            state: SocketState::Created,
            local_addr: None,
            remote_addr: None,
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
        }
    }

    /// 绑定本地地址
    pub fn bind(&mut self, addr: SocketAddr) -> KernelResult<()> {
        if self.state != SocketState::Created {
            return Err(KernelError::InvalidArgument);
        }
        self.local_addr = Some(addr);
        self.state = SocketState::Bound;
        Ok(())
    }

    /// 开始监听（TCP）
    pub fn listen(&mut self, backlog: usize) -> KernelResult<()> {
        if self.socket_type != SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }
        if self.state != SocketState::Bound {
            return Err(KernelError::InvalidArgument);
        }
        self.state = SocketState::Listening;

        // 实现连接队列管理
        // 为什么需要连接队列：accept() 需要从队列中获取待处理的连接
        // 简化实现：预留空间用于存储等待 accept 的连接信息
        // 实际应用中应该维护一个队列结构
        let queue_size = backlog.min(crate::net::config::SOCKET_BACKLOG_MAX);
        let _ = queue_size;  // 当前简化实现忽略队列大小参数

        Ok(())
    }

    /// 发起连接（TCP）
    pub fn connect(&mut self, addr: SocketAddr) -> KernelResult<()> {
        if self.socket_type != SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }
        self.remote_addr = Some(addr);
        self.state = SocketState::Connecting;

        // 发送 SYN 包
        // 为什么需要发送 SYN：这是 TCP 三次握手的第一步
        if let Some(local_addr) = self.local_addr {
            // 使用 TCP 模块的 SYN 创建函数
            let syn_header = tcp::create_syn(
                local_addr.port,
                addr.port,
                0,  // 序列号会由 TCP 层管理
            );

            // 构建 TCP 包
            let mut packet = alloc::vec::Vec::new();
            packet.extend_from_slice(&syn_header.to_bytes());

            // 调用 TCP 发送（简化版本，实际应该使用完整的四元组）
            let _ = tcp::send_packet(local_addr.port, addr.port, &packet);
        }

        Ok(())
    }

    /// 关闭套接字
    pub fn close(&mut self) -> KernelResult<()> {
        if self.state == SocketState::Connected {
            self.state = SocketState::Closing;

            // 发送 FIN 包（TCP）或直接关闭（UDP）
            // 为什么需要发送 FIN：TCP 需要优雅地关闭连接，通知对方
            match self.socket_type {
                SocketType::Stream => {
                    // TCP 发送 FIN 包
                    if let Some(local_addr) = self.local_addr {
                        if let Some(remote_addr) = self.remote_addr {
                            let fin_header = tcp::create_fin(
                                local_addr.port,
                                remote_addr.port,
                                0,  // 序列号由 TCP 层管理
                            );

                            let mut packet = alloc::vec::Vec::new();
                            packet.extend_from_slice(&fin_header.to_bytes());

                            let _ = tcp::send_packet(local_addr.port, remote_addr.port, &packet);
                        }
                    }
                }
                SocketType::Dgram => {
                    // UDP 无连接，直接关闭即可
                }
            }
        }
        self.state = SocketState::Closed;
        Ok(())
    }

    /// 发送数据
    pub fn send(&mut self, data: &[u8]) -> KernelResult<usize> {
        if self.state != SocketState::Connected && self.socket_type == SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }

        // 调用相应的传输层发送函数
        // 为什么需要分派到传输层：TCP 和 UDP 的发送方式不同
        match self.socket_type {
            SocketType::Stream => {
                // TCP 发送
                if let Some(local_addr) = self.local_addr {
                    if let Some(remote_addr) = self.remote_addr {
                        // 调用 TCP 发送
                        return tcp::send_packet(local_addr.port, remote_addr.port, data);
                    }
                }
            }
            SocketType::Dgram => {
                // UDP 发送
                if let Some(local_addr) = self.local_addr {
                    if let Some(remote_addr) = self.remote_addr {
                        // 调用 UDP 发送
                        return udp::send_packet(
                            local_addr.addr,
                            local_addr.port,
                            remote_addr.addr,
                            remote_addr.port,
                            data,
                        );
                    }
                }
            }
        }

        Err(KernelError::InvalidArgument)
    }

    /// 接收数据
    pub fn recv(&mut self, buffer: &mut [u8]) -> KernelResult<usize> {
        if self.recv_buffer.is_empty() {
            return Ok(0);  // 无数据
        }

        let to_read = buffer.len().min(self.recv_buffer.len());
        buffer[0..to_read].copy_from_slice(&self.recv_buffer[0..to_read]);
        self.recv_buffer.drain(0..to_read);

        Ok(to_read)
    }
}

// ============================================================
// Socket 表
// ============================================================

/// Socket 表（简化版本）
pub struct SocketTable {
    /// 套接字列表
    sockets: Vec<SocketDescriptor>,
    /// 下一个套接字 FD
    next_fd: u32,
}

impl SocketTable {
    /// 创建空的 Socket 表
    pub fn new() -> Self {
        SocketTable {
            sockets: Vec::new(),
            next_fd: 3,  // 0=stdin, 1=stdout, 2=stderr
        }
    }

    /// 创建新的 Socket
    pub fn socket(&mut self, family: AddressFamily, socket_type: SocketType) -> KernelResult<u32> {
        if self.sockets.len() >= crate::net::config::SOCKET_TABLE_MAX {
            return Err(KernelError::OutOfMemory);
        }

        let fd = self.next_fd;
        self.next_fd += 1;

        let socket = SocketDescriptor::new(fd, socket_type, family);
        self.sockets.push(socket);

        Ok(fd)
    }

    /// 获取 Socket（可变引用）
    pub fn get_mut(&mut self, fd: u32) -> KernelResult<&mut SocketDescriptor> {
        self.sockets
            .iter_mut()
            .find(|s| s.fd == fd)
            .ok_or(KernelError::NotFound)
    }

    /// 获取 Socket（不可变引用）
    pub fn get(&self, fd: u32) -> KernelResult<&SocketDescriptor> {
        self.sockets
            .iter()
            .find(|s| s.fd == fd)
            .ok_or(KernelError::NotFound)
    }

    /// 关闭 Socket
    pub fn close(&mut self, fd: u32) -> KernelResult<()> {
        let initial_len = self.sockets.len();
        self.sockets.retain_mut(|s| {
            if s.fd == fd {
                let _ = s.close();
                false
            } else {
                true
            }
        });

        if self.sockets.len() == initial_len {
            Err(KernelError::NotFound)
        } else {
            Ok(())
        }
    }

    /// 获取 Socket 表大小
    pub fn len(&self) -> usize {
        self.sockets.len()
    }
}

// ============================================================
// Socket 自测
// ============================================================

pub fn selftest() -> bool {
    // 1. Socket 地址创建测试
    let addr = SocketAddr::new([192, 168, 1, 1], 8080);
    assert_eq!(addr.family, AddressFamily::Inet, "地址族不正确");
    assert_eq!(addr.port, 8080, "端口号不正确");
    assert_eq!(addr.addr, [192, 168, 1, 1], "IP 地址不正确");

    // 2. 特殊地址测试
    let any_addr = SocketAddr::any(5000);
    assert_eq!(any_addr.addr, [0, 0, 0, 0], "任意地址不正确");

    let lo_addr = SocketAddr::loopback(9000);
    assert_eq!(lo_addr.addr, [127, 0, 0, 1], "环回地址不正确");

    // 3. Socket 表创建测试
    let mut table = SocketTable::new();
    assert_eq!(table.len(), 0, "初始表大小应为 0");
    assert_eq!(table.next_fd, 3, "初始 FD 应为 3");

    // 4. TCP Socket 创建测试
    let tcp_fd = table.socket(AddressFamily::Inet, SocketType::Stream)
        .expect("创建 TCP Socket 失败");
    assert_eq!(tcp_fd, 3, "第一个 FD 应为 3");
    assert_eq!(table.len(), 1, "表大小应为 1");

    // 5. UDP Socket 创建测试
    let udp_fd = table.socket(AddressFamily::Inet, SocketType::Dgram)
        .expect("创建 UDP Socket 失败");
    assert_eq!(udp_fd, 4, "第二个 FD 应为 4");
    assert_eq!(table.len(), 2, "表大小应为 2");

    // 6. TCP Socket 绑定测试
    let tcp_socket = table.get_mut(tcp_fd).expect("获取 TCP Socket 失败");
    let server_addr = SocketAddr::new([0, 0, 0, 0], 8080);
    tcp_socket.bind(server_addr).expect("TCP 绑定失败");
    assert_eq!(tcp_socket.state, SocketState::Bound, "TCP 状态应为 BOUND");
    assert!(tcp_socket.local_addr.is_some(), "本地地址应已设置");

    // 7. TCP Socket 监听测试
    tcp_socket.listen(128).expect("TCP 监听失败");
    assert_eq!(tcp_socket.state, SocketState::Listening, "TCP 状态应为 LISTENING");

    // 8. UDP Socket 绑定测试
    let udp_socket = table.get_mut(udp_fd).expect("获取 UDP Socket 失败");
    let udp_addr = SocketAddr::new([0, 0, 0, 0], 5353);
    udp_socket.bind(udp_addr).expect("UDP 绑定失败");
    assert_eq!(udp_socket.state, SocketState::Bound, "UDP 状态应为 BOUND");

    // 9. Socket 发送接收缓冲区测试
    let test_data = b"Hello Socket";
    udp_socket.send_buffer.extend_from_slice(test_data);
    assert_eq!(udp_socket.send_buffer.len(), test_data.len(), "发送缓冲区大小不正确");

    // 10. Socket 接收测试
    udp_socket.recv_buffer.extend_from_slice(test_data);
    let mut recv_buf = [0u8; 20];
    let read = udp_socket.recv(&mut recv_buf).expect("接收失败");
    assert_eq!(read, test_data.len(), "接收字节数不正确");
    assert_eq!(&recv_buf[0..read], test_data, "接收数据不匹配");
    assert!(udp_socket.recv_buffer.is_empty(), "接收缓冲区应为空");

    // 11. Socket 关闭测试
    let close_result = table.close(tcp_fd);
    assert!(close_result.is_ok(), "关闭 Socket 失败");
    assert_eq!(table.len(), 1, "关闭后表大小应为 1");

    // 12. 关闭已关闭的 Socket 应该失败
    let close_again = table.close(tcp_fd);
    assert!(close_again.is_err(), "关闭已关闭的 Socket 应失败");

    true
}
