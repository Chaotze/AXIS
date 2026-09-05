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
    pub fn listen(&mut self, _backlog: usize) -> KernelResult<()> {
        if self.socket_type != SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }
        if self.state != SocketState::Bound {
            return Err(KernelError::InvalidArgument);
        }
        self.state = SocketState::Listening;
        // TODO: 实现连接队列管理
        Ok(())
    }

    /// 发起连接（TCP）
    pub fn connect(&mut self, addr: SocketAddr) -> KernelResult<()> {
        if self.socket_type != SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }
        self.remote_addr = Some(addr);
        self.state = SocketState::Connecting;
        // TODO: 发送 SYN 包
        Ok(())
    }

    /// 关闭套接字
    pub fn close(&mut self) -> KernelResult<()> {
        if self.state == SocketState::Connected {
            self.state = SocketState::Closing;
            // TODO: 发送 FIN 包（TCP）或直接关闭（UDP）
        }
        self.state = SocketState::Closed;
        Ok(())
    }

    /// 发送数据
    pub fn send(&mut self, data: &[u8]) -> KernelResult<usize> {
        if self.state != SocketState::Connected && self.socket_type == SocketType::Stream {
            return Err(KernelError::InvalidArgument);
        }
        // TODO: 调用相应的传输层发送函数
        Ok(data.len())
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
    // Socket 创建测试
    let mut table = SocketTable::new();
    let fd = table.socket(AddressFamily::Inet, SocketType::Stream)
        .expect("创建 Socket 失败");

    // 绑定测试
    let socket = table.get_mut(fd).expect("获取 Socket 失败");
    let addr = SocketAddr::new([127, 0, 0, 1], 8080);
    socket.bind(addr).expect("绑定失败");
    assert_eq!(socket.state, SocketState::Bound, "状态不正确");

    // 监听测试
    socket.listen(5).expect("监听失败");
    assert_eq!(socket.state, SocketState::Listening, "监听状态不正确");

    // Socket 表管理测试
    assert_eq!(table.len(), 1, "Socket 表大小不正确");

    true
}
