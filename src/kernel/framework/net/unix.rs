//! Unix Domain Socket (AF_UNIX) 子系统 — framework TCB
//!
//! ## 职责
//!
//! - 维护全局 `UDS_STATE`: socket 表 + 路径绑定表
//! - 暴露 12 个 `uds_*` 原始函数供 services 安全层包装
//! - 所有 unsafe 集中在本文件 (`static mut` 初始化) 与 FFI 边界
//!
//! ## 关键不变量
//!
//! - FD 空间: 由 `fd_alloc::FdPlan::UDS` 集中规划 (基址 1000, 容量 16)
//! - I-51: 与 smoltcp (`[0, MAX_SM_FD=256)`) 不重叠 (历史 100 范围与 smoltcp 重叠, 已挪出)
//! - 路径长度 ≤ `UNIX_PATH_MAX` = 108 (POSIX `sun_path` 上限)
//! - 每个 STREAM 连接由两端 socket 共享: 写方向 `src.stream_buf → dst.stream_buf`
//! - 每个 DGRAM socket 独立持有一个 `dgram_buf` (单消息排队)
//! - 路径与 socket **不可重名** (`bind` 时 `EADDRINUSE`)
//! - listener 的 `accept()` 行为非阻塞: 有 pending 则立即返回新 FD, 否则 `EAGAIN`
//!
//! ## 安全契约
//!
//! - 全局状态由 `IrqSpinLock` 守护, 持锁期间屏蔽中断
//! - 所有 public 函数接受 `&[u8]` / `&mut [u8]` 切片, 不接触用户空间裸指针
//! - services 层负责 copy-in / copy-out 与路径 UTF-8 校验
//!
//! ## 评估日期
//!
//! 2026-06-08, 关联 DECISION-006/007/008

#![allow(dead_code)]

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

// ============================================================================
// 常量
// ============================================================================

/// FD 起点, 位于 smoltcp FD 空间 (`[0, 256)`) 之后, 与之不重叠.
/// TD-02: 基址来源已迁移至 `framework::proc::fd_alloc::FdPlan::UDS` 单一来源, 不再硬编码.
pub const UDS_FD_BASE: i32 = crate::kernel::framework::proc::fd_alloc::FdPlan::UDS.base;

/// 最大 UDS socket 数量
pub const MAX_UDS_FD: usize = 16;

/// POSIX `sun_path` 最大长度
pub const UNIX_PATH_MAX: usize = 108;

/// 路径绑定表容量 (略大于 socket 表, 留空给未绑定的临时路径)
pub const UNIX_MAX_BINDINGS: usize = 32;

/// SOCK_STREAM 单端接收缓冲大小
pub const UNIX_STREAM_BUF: usize = 8192;

/// SOCK_DGRAM 单消息最大长度
pub const UNIX_DGRAM_MAX: usize = 8192;

/// listen() 固定 backlog (与 Linux 早期默认值一致)
pub const UNIX_LISTEN_BACKLOG: usize = 5;

// ============================================================================
// 错误码 (POSIX errno, 与 framework::syscall::types::Errno 对齐)
// ============================================================================

/// UDS TCB 内部错误 (services 层映射为 `UnixError` / `Errno`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum UdsError {
    /// 文件描述符无效
    BadFd = 9,
    /// 操作会阻塞 (非阻塞 accept 无 pending)
    Again = 11,
    /// 内存不足
    NoMem = 12,
    /// 地址族不支持 (调用方传入非 AF_UNIX)
    AddrFamily = 97,
    /// 地址已被使用 (路径已 bind)
    AddrInUse = 98,
    /// 目标地址无监听/无绑定
    ConnRefused = 111,
    /// 状态非法 (如 `connect` 已 `Connected` 的 socket)
    Invalid = 22,
    /// 路径不存在
    NotFound = 2,
    /// 子特性未启用
    NoSys = 38,
}

impl UdsError {
    pub fn as_ret(self) -> i32 {
        -(self as i32)
    }
}

// ============================================================================
// 类型定义
// ============================================================================

/// Socket 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnixSockType {
    /// 流式 (字节流, 无消息边界)
    Stream = 1,
    /// 数据报 (消息边界保留)
    Dgram = 2,
}

impl UnixSockType {
    pub fn from_i32(t: i32) -> Option<Self> {
        match t {
            1 => Some(Self::Stream),
            2 => Some(Self::Dgram),
            _ => None,
        }
    }
}

/// Socket 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnixSockState {
    /// 新建, 未绑定
    Unbound = 0,
    /// 已 bind, 未 listen (DGRAM 等待 connect, STREAM 等待 listen)
    Bound = 1,
    /// STREAM 监听中, 等待 accept
    Listening = 2,
    /// 已连接, 拥有 peer
    Connected = 3,
    /// 已关闭 (仅作语义标记, 资源立即释放)
    Closed = 4,
}

/// UDS socket (固定大小, 复制语义)
#[derive(Debug, Clone)]
pub struct UnixSocket {
    /// 唯一 ID (0 = 槽位空闲)
    pub id: u32,
    /// 类型
    pub sock_type: UnixSockType,
    /// 当前状态
    pub state: UnixSockState,
    /// 绑定路径 (None = 未绑定)
    pub bound_path: [u8; UNIX_PATH_MAX],
    pub bound_path_len: u16,

    /// 监听者: 等待 accept 的对端 socket ID 队列
    pub listen_pending: [u32; UNIX_LISTEN_BACKLOG],
    pub listen_head: u8,
    pub listen_tail: u8,
    pub listen_count: u8,

    /// 已连接 socket: 对端 ID (None = 无对端)
    pub peer: Option<u32>,

    /// STREAM 接收缓冲 (peer 的 send 写入此处)
    pub stream_buf: [u8; UNIX_STREAM_BUF],
    pub stream_len: u32,
    /// STREAM 对端已关闭标志 (read 完剩余数据后下次 recv 返回 0)
    pub peer_closed: bool,

    /// DGRAM 单消息缓冲
    pub dgram_buf: [u8; UNIX_DGRAM_MAX],
    pub dgram_len: u32,
    pub dgram_pending: bool,
}

impl UnixSocket {
    /// const 构造: 全零
    pub const fn empty() -> Self {
        Self {
            id: 0,
            sock_type: UnixSockType::Stream,
            state: UnixSockState::Unbound,
            bound_path: [0u8; UNIX_PATH_MAX],
            bound_path_len: 0,
            listen_pending: [0u32; UNIX_LISTEN_BACKLOG],
            listen_head: 0,
            listen_tail: 0,
            listen_count: 0,
            peer: None,
            stream_buf: [0u8; UNIX_STREAM_BUF],
            stream_len: 0,
            peer_closed: false,
            dgram_buf: [0u8; UNIX_DGRAM_MAX],
            dgram_len: 0,
            dgram_pending: false,
        }
    }
}

/// 路径绑定 (path → socket ID)
#[derive(Debug, Clone, Copy)]
pub struct UnixPathBinding {
    pub path: [u8; UNIX_PATH_MAX],
    pub path_len: u16,
    /// 指向 sockets[idx]
    pub sock_idx: u8,
    /// 0 = 槽位空闲
    pub used: bool,
}

impl UnixPathBinding {
    pub const fn empty() -> Self {
        Self {
            path: [0u8; UNIX_PATH_MAX],
            path_len: 0,
            sock_idx: 0,
            used: false,
        }
    }
}

/// 全局 UDS 状态容器
#[derive(Debug)]
pub struct UdsState {
    /// socket 表 (index = fd - UDS_FD_BASE)
    pub sockets: [UnixSocket; MAX_UDS_FD],
    /// 路径绑定表
    pub paths: [UnixPathBinding; UNIX_MAX_BINDINGS],
}

impl UdsState {
    /// const 构造: 全空
    pub const fn new() -> Self {
        Self {
            sockets: [const { UnixSocket::empty() }; MAX_UDS_FD],
            paths: [const { UnixPathBinding::empty() }; UNIX_MAX_BINDINGS],
        }
    }

    /// 找空闲 socket 槽位
    fn find_free_socket(&self) -> Option<u8> {
        for (i, s) in self.sockets.iter().enumerate() {
            if s.id == 0 {
                return Some(i as u8);
            }
        }
        None
    }

    /// 找空闲路径槽位
    fn find_free_path(&self) -> Option<u8> {
        for (i, p) in self.paths.iter().enumerate() {
            if !p.used {
                return Some(i as u8);
            }
        }
        None
    }

    /// 按路径查找绑定
    fn find_path(&self, path: &[u8]) -> Option<u8> {
        for (i, p) in self.paths.iter().enumerate() {
            if p.used && p.path_len as usize == path.len() && p.path[..path.len()] == *path {
                return Some(i as u8);
            }
        }
        None
    }

    /// 按 ID 找 socket 槽位索引
    fn socket_idx_by_id(&self, id: u32) -> Option<u8> {
        for (i, s) in self.sockets.iter().enumerate() {
            if s.id == id {
                return Some(i as u8);
            }
        }
        None
    }
}

// ============================================================================
// 全局状态 (IrqSpinLock 守护)
// ============================================================================

/// 全局 UDS 状态
pub static UDS_STATE: IrqSpinLock<UdsState> = IrqSpinLock::new(UdsState::new());

/// 下一个 socket ID 分配器
static NEXT_SOCK_ID: IrqSpinLock<u32> = IrqSpinLock::new(1);

// ============================================================================
// 内部辅助
// ============================================================================

/// 分配下一个 socket ID
fn alloc_socket_id() -> u32 {
    let mut guard = NEXT_SOCK_ID.lock();
    let id = *guard;
    // 跳过 0, 从 1 开始; 1..=u32::MAX 循环回卷到 1 (避免与 0=空闲冲突)
    *guard = if id == u32::MAX { 1 } else { id + 1 };
    id
}

/// FD ↔ 槽位索引转换
///
/// TD-02 V4: 改走 `fd_alloc::idx_of` 集中反查, 本地不再持有 UDS_FD_BASE 字面量 +
/// 减法边界检查. 错误返回 UdsError::BadFd.
#[inline]
fn fd_to_idx(fd: i32) -> Result<u8, UdsError> {
    match crate::kernel::framework::proc::fd_alloc::idx_of(fd) {
        Some((crate::kernel::framework::proc::fd_alloc::FdSubsystem::Uds, slot)) => {
            // MAX_UDS_FD = 16, 永远 u8 范围
            Ok(slot as u8)
        }
        _ => Err(UdsError::BadFd),
    }
}

#[inline]
fn idx_to_fd(idx: u8) -> i32 {
    // TD-02 V3: 通过 fd_alloc 集中计算 FD 编号
    crate::kernel::framework::proc::fd_alloc::fd_at(
        crate::kernel::framework::proc::fd_alloc::FdSubsystem::Uds,
        idx as usize,
    )
}

// ============================================================================
// 公开 TCB API
// ============================================================================

/// 全局初始化 (启动期调用一次)
///
/// 静态状态已通过 `UdsState::new()` const 初始化为零值, 此函数作为显式 init 钩子
/// 留作未来扩展 (例如统计计数器、IRQs 注册等)。
/// 当前仅重置 socket ID 分配器, 防止重启后 ID 错位。
pub fn uds_init() {
    NEXT_SOCK_ID.with_mut(|id| *id = 1);
}

/// 创建新 socket, 返回 FD
pub fn uds_create(sock_type: UnixSockType) -> Result<i32, UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = state.find_free_socket().ok_or(UdsError::NoMem)?;
        let id = alloc_socket_id();
        let s = &mut state.sockets[idx as usize];
        s.id = id;
        s.sock_type = sock_type;
        s.state = UnixSockState::Unbound;
        s.bound_path_len = 0;
        s.listen_count = 0;
        s.listen_head = 0;
        s.listen_tail = 0;
        s.peer = None;
        s.stream_len = 0;
        s.peer_closed = false;
        s.dgram_len = 0;
        s.dgram_pending = false;
        Ok(idx_to_fd(idx))
    })
}

/// bind(path) — 绑定路径
pub fn uds_bind(fd: i32, path: &[u8]) -> Result<(), UdsError> {
    if path.is_empty() || path.len() > UNIX_PATH_MAX {
        return Err(UdsError::Invalid);
    }
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &state.sockets[idx];
        if s.id == 0 {
            return Err(UdsError::BadFd);
        }
        if s.state != UnixSockState::Unbound {
            return Err(UdsError::Invalid);
        }
        if state.find_path(path).is_some() {
            return Err(UdsError::AddrInUse);
        }
        let pidx = state.find_free_path().ok_or(UdsError::NoMem)?;
        let binding = &mut state.paths[pidx as usize];
        binding.path[..path.len()].copy_from_slice(path);
        binding.path_len = path.len() as u16;
        binding.sock_idx = idx as u8;
        binding.used = true;
        let s = &mut state.sockets[idx];
        s.bound_path[..path.len()].copy_from_slice(path);
        s.bound_path_len = path.len() as u16;
        s.state = UnixSockState::Bound;
        Ok(())
    })
}

/// listen() — STREAM 标记为监听
pub fn uds_listen(fd: i32) -> Result<(), UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &state.sockets[idx];
        if s.id == 0 {
            return Err(UdsError::BadFd);
        }
        if s.sock_type != UnixSockType::Stream {
            return Err(UdsError::Invalid);
        }
        if s.state != UnixSockState::Bound {
            return Err(UdsError::Invalid);
        }
        state.sockets[idx].state = UnixSockState::Listening;
        Ok(())
    })
}

/// accept() — 弹出 pending, 创建 server-side socket, 返回新 FD
///
/// 行为: 非阻塞, 有 pending 则立即返回, 否则 `EAGAIN`
pub fn uds_accept(fd: i32) -> Result<i32, UdsError> {
    UDS_STATE.with_mut(|state| {
        let listen_idx = fd_to_idx(fd)? as usize;
        let listen = &state.sockets[listen_idx];
        if listen.id == 0 {
            return Err(UdsError::BadFd);
        }
        if listen.sock_type != UnixSockType::Stream {
            return Err(UdsError::Invalid);
        }
        if listen.state != UnixSockState::Listening {
            return Err(UdsError::Invalid);
        }
        if listen.listen_count == 0 {
            return Err(UdsError::Again);
        }
        // 取出 client_id
        let client_id = listen.listen_pending[listen.listen_head as usize];
        let client_idx = state.socket_idx_by_id(client_id).ok_or(UdsError::NoMem)? as usize;
        // 创建 server-side 新 socket (与 client 配对)
        let new_idx = state.find_free_socket().ok_or(UdsError::NoMem)? as usize;
        let id = alloc_socket_id();
        let ns = &mut state.sockets[new_idx];
        ns.id = id;
        ns.sock_type = UnixSockType::Stream;
        ns.state = UnixSockState::Connected;
        ns.peer = Some(client_id);
        ns.stream_len = 0;
        ns.peer_closed = false;
        // 弹出 listener
        state.sockets[listen_idx].listen_head =
            (state.sockets[listen_idx].listen_head + 1) % UNIX_LISTEN_BACKLOG as u8;
        state.sockets[listen_idx].listen_count -= 1;
        // 设置 client.peer = new socket
        state.sockets[client_idx].peer = Some(id);
        // client 状态保持 Connected (connect() 已设)
        Ok(idx_to_fd(new_idx as u8))
    })
}

/// connect(path) — 客户端连到监听/绑定 socket
pub fn uds_connect(fd: i32, path: &[u8]) -> Result<(), UdsError> {
    if path.is_empty() || path.len() > UNIX_PATH_MAX {
        return Err(UdsError::Invalid);
    }
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        if state.sockets[idx].id == 0 {
            return Err(UdsError::BadFd);
        }
        let s = &state.sockets[idx];
        if s.state != UnixSockState::Unbound && s.state != UnixSockState::Bound {
            return Err(UdsError::Invalid);
        }
        // 查找目标路径
        let pidx = state.find_path(path).ok_or(UdsError::ConnRefused)? as usize;
        let target_idx = state.paths[pidx].sock_idx as usize;
        let target = &state.sockets[target_idx];
        if target.sock_type != state.sockets[idx].sock_type {
            return Err(UdsError::Invalid);
        }
        match (state.sockets[idx].sock_type, target.state) {
            (UnixSockType::Stream, UnixSockState::Listening) => {
                // STREAM: 在 listener 的 pending 队列里加一个 client socket
                if target.listen_count as usize >= UNIX_LISTEN_BACKLOG {
                    return Err(UdsError::Again);
                }
                // 把当前 fd 标记为 client Connected, peer 待 accept 时分配
                let tail = target.listen_tail as usize;
                let my_id = state.sockets[idx].id;
                state.sockets[target_idx].listen_pending[tail] = my_id;
                state.sockets[target_idx].listen_tail =
                    (state.sockets[target_idx].listen_tail + 1) % UNIX_LISTEN_BACKLOG as u8;
                state.sockets[target_idx].listen_count += 1;
                // client 端先标记为 Connected (peer 在 accept 时填)
                state.sockets[idx].state = UnixSockState::Connected;
                state.sockets[idx].bound_path_len = 0; // connect 后不再保留原始路径
                Ok(())
            }
            (UnixSockType::Dgram, _) => {
                // DGRAM: 双向配对, 双方都设 peer
                let my_id = state.sockets[idx].id;
                let peer_id = target.id;
                state.sockets[idx].peer = Some(peer_id);
                state.sockets[target_idx].peer = Some(my_id);
                state.sockets[idx].state = UnixSockState::Connected;
                state.sockets[idx].bound_path_len = 0;
                Ok(())
            }
            _ => Err(UdsError::ConnRefused),
        }
    })
}

/// STREAM send — 写入 peer 的接收缓冲
pub fn uds_send(fd: i32, data: &[u8]) -> Result<usize, UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Stream {
            return Err(UdsError::Invalid);
        }
        if s.state != UnixSockState::Connected {
            return Err(UdsError::NotFound);
        }
        let peer_id = s.peer.ok_or(UdsError::NotFound)?;
        let peer_idx = state.socket_idx_by_id(peer_id).ok_or(UdsError::NotFound)? as usize;
        let peer = &mut state.sockets[peer_idx];
        if peer.peer_closed {
            return Err(UdsError::NotFound);
        }
        let space = UNIX_STREAM_BUF - peer.stream_len as usize;
        let n = data.len().min(space);
        if n == 0 {
            return Err(UdsError::Again);
        }
        peer.stream_buf[peer.stream_len as usize..peer.stream_len as usize + n]
            .copy_from_slice(&data[..n]);
        peer.stream_len += n as u32;
        Ok(n)
    })
}

/// STREAM recv — 从自己的接收缓冲读出
pub fn uds_recv(fd: i32, out: &mut [u8]) -> Result<usize, UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &mut state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Stream {
            return Err(UdsError::Invalid);
        }
        if s.state != UnixSockState::Connected {
            return Err(UdsError::Invalid);
        }
        if s.stream_len == 0 {
            if s.peer_closed {
                return Ok(0);
            }
            return Err(UdsError::Again);
        }
        let n = (s.stream_len as usize).min(out.len());
        out[..n].copy_from_slice(&s.stream_buf[..n]);
        // 移动剩余字节到缓冲头部
        let remaining = s.stream_len as usize - n;
        if remaining > 0 {
            s.stream_buf.copy_within(n..n + remaining, 0);
        }
        s.stream_len = remaining as u32;
        Ok(n)
    })
}

/// DGRAM sendto — 写入目标 socket 的 datagram 槽
pub fn uds_sendto(fd: i32, data: &[u8], dest_path: &[u8]) -> Result<usize, UdsError> {
    if dest_path.is_empty() || dest_path.len() > UNIX_PATH_MAX {
        return Err(UdsError::Invalid);
    }
    if data.len() > UNIX_DGRAM_MAX {
        return Err(UdsError::Invalid);
    }
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Dgram {
            return Err(UdsError::Invalid);
        }
        if s.state != UnixSockState::Bound && s.state != UnixSockState::Connected {
            return Err(UdsError::Invalid);
        }
        let pidx = state.find_path(dest_path).ok_or(UdsError::ConnRefused)? as usize;
        let target_idx = state.paths[pidx].sock_idx as usize;
        let target = &mut state.sockets[target_idx];
        if target.sock_type != UnixSockType::Dgram {
            return Err(UdsError::Invalid);
        }
        // 覆盖式写入 (单消息槽)
        target.dgram_buf[..data.len()].copy_from_slice(data);
        target.dgram_len = data.len() as u32;
        target.dgram_pending = true;
        Ok(data.len())
    })
}

/// DGRAM recvfrom — 从自己的 datagram 槽读出
pub fn uds_recvfrom(fd: i32, out: &mut [u8]) -> Result<usize, UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &mut state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Dgram {
            return Err(UdsError::Invalid);
        }
        if !s.dgram_pending {
            return Err(UdsError::Again);
        }
        let n = (s.dgram_len as usize).min(out.len());
        out[..n].copy_from_slice(&s.dgram_buf[..n]);
        s.dgram_pending = false;
        s.dgram_len = 0;
        Ok(n)
    })
}

/// close — 关闭 socket, 释放路径与配对
pub fn uds_close(fd: i32) -> Result<(), UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        if state.sockets[idx].id == 0 {
            return Err(UdsError::BadFd);
        }
        let s = &state.sockets[idx];
        let sock_type = s.sock_type;
        let state_val = s.state;
        let bound_path = s.bound_path;
        let bound_path_len = s.bound_path_len;
        let peer_id = s.peer;

        // 若是 STREAM Connected, 通知对端
        if sock_type == UnixSockType::Stream
            && state_val == UnixSockState::Connected
        {
            if let Some(pid) = peer_id {
                if let Some(pidx) = state.socket_idx_by_id(pid) {
                    state.sockets[pidx as usize].peer_closed = true;
                    state.sockets[pidx as usize].peer = None;
                }
            }
        }

        // 若是 listener, 关闭所有 pending client
        if sock_type == UnixSockType::Stream && state_val == UnixSockState::Listening {
            for i in 0..state.sockets[idx].listen_count {
                let pos = (state.sockets[idx].listen_head + i) % UNIX_LISTEN_BACKLOG as u8;
                let client_id = state.sockets[idx].listen_pending[pos as usize];
                if let Some(ci) = state.socket_idx_by_id(client_id) {
                    // 直接清空 client 槽位 (id=0 标记为空闲)
                    state.sockets[ci as usize] = UnixSocket::empty();
                }
            }
        }

        // 解除路径绑定
        if bound_path_len > 0 {
            for i in 0..state.paths.len() {
                if state.paths[i].used
                    && state.paths[i].path_len == bound_path_len
                    && state.paths[i].path[..bound_path_len as usize] == bound_path[..bound_path_len as usize]
                {
                    state.paths[i].used = false;
                    state.paths[i].sock_idx = 0;
                    state.paths[i].path_len = 0;
                    break;
                }
            }
        }

        // 清空 socket 槽位
        state.sockets[idx] = UnixSocket::empty();
        Ok(())
    })
}

/// unlink — 显式解除路径绑定 (close 时会自动调用, 此函数给管理者手动清理用)
pub fn uds_unlink(path: &[u8]) -> Result<(), UdsError> {
    if path.is_empty() || path.len() > UNIX_PATH_MAX {
        return Err(UdsError::Invalid);
    }
    UDS_STATE.with_mut(|state| {
        let pidx = state.find_path(path).ok_or(UdsError::NotFound)? as usize;
        let sidx = state.paths[pidx].sock_idx as usize;
        state.paths[pidx] = UnixPathBinding::empty();
        // 标记对应 socket 为未绑定 (但不释放 socket 本身, 需 close)
        state.sockets[sidx].bound_path_len = 0;
        state.sockets[sidx].state = UnixSockState::Unbound;
        Ok(())
    })
}

/// 重置全部状态 (故障恢复 / 单元测试用)
pub fn uds_reset_for_test() {
    UDS_STATE.with_mut(|state| {
        for s in state.sockets.iter_mut() {
            *s = UnixSocket::empty();
        }
        for p in state.paths.iter_mut() {
            *p = UnixPathBinding::empty();
        }
    });
    NEXT_SOCK_ID.with_mut(|id| *id = 1);
}

// ============================================================================
// 单元测试 (host 端)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 完整生命周期: bind → listen → connect → accept → send/recv → close
    #[test]
    fn stream_bind_listen_connect_accept_echo() {
        uds_reset_for_test();
        let srv = uds_create(UnixSockType::Stream).expect("srv create");
        let cli = uds_create(UnixSockType::Stream).expect("cli create");
        uds_bind(srv, b"/tmp/test.sock").expect("srv bind");
        uds_listen(srv).expect("srv listen");
        // connect 在 client FD 上调用, 但实际建立 server-side 在 listener 的 pending 队列
        uds_connect(cli, b"/tmp/test.sock").expect("cli connect");
        // accept 从 pending 弹出一个 client, 创建 server-side
        let accepted = uds_accept(srv).expect("accept");
        assert_ne!(accepted, srv);
        // client 发送
        let n = uds_send(cli, b"hello").expect("send");
        assert_eq!(n, 5);
        // server 接收
        let mut buf = [0u8; 16];
        let m = uds_recv(accepted, &mut buf).expect("recv");
        assert_eq!(m, 5);
        assert_eq!(&buf[..5], b"hello");
        // 反向
        let k = uds_send(accepted, b"world").expect("send back");
        assert_eq!(k, 5);
        let mut buf2 = [0u8; 16];
        let p = uds_recv(cli, &mut buf2).expect("recv back");
        assert_eq!(p, 5);
        assert_eq!(&buf2[..5], b"world");
        uds_close(cli).expect("cli close");
        uds_close(accepted).expect("acc close");
        uds_close(srv).expect("srv close");
    }

    /// DGRAM: bind → connect → sendto/recvfrom 流程
    #[test]
    fn dgram_bind_connect_echo() {
        uds_reset_for_test();
        let rx = uds_create(UnixSockType::Dgram).expect("rx create");
        let tx = uds_create(UnixSockType::Dgram).expect("tx create");
        uds_bind(rx, b"/tmp/rx.sock").expect("rx bind");
        uds_connect(tx, b"/tmp/rx.sock").expect("tx connect");
        let n = uds_sendto(tx, b"datagram-payload", b"/tmp/rx.sock").expect("sendto");
        assert_eq!(n, 16);
        let mut buf = [0u8; 32];
        let m = uds_recvfrom(rx, &mut buf).expect("recvfrom");
        assert_eq!(m, 16);
        assert_eq!(&buf[..16], b"datagram-payload");
        uds_close(tx).expect("close tx");
        uds_close(rx).expect("close rx");
    }

    /// 路径冲突
    #[test]
    fn eaddrinuse_on_duplicate_bind() {
        uds_reset_for_test();
        let a = uds_create(UnixSockType::Stream).expect("a");
        let b = uds_create(UnixSockType::Stream).expect("b");
        uds_bind(a, b"/tmp/dup.sock").expect("a bind");
        let r = uds_bind(b, b"/tmp/dup.sock");
        assert_eq!(r, Err(UdsError::AddrInUse));
        uds_close(a).expect("close a");
        uds_close(b).expect("close b");
    }

    /// accept 无 pending → EAGAIN
    #[test]
    fn eagain_on_empty_accept() {
        uds_reset_for_test();
        let s = uds_create(UnixSockType::Stream).expect("s");
        uds_bind(s, b"/tmp/empty.sock").expect("bind");
        uds_listen(s).expect("listen");
        let r = uds_accept(s);
        assert_eq!(r, Err(UdsError::Again));
        uds_close(s).expect("close");
    }

    /// close listener → 所有 pending client 同步关闭
    #[test]
    fn close_listener_cancels_pending_clients() {
        uds_reset_for_test();
        let srv = uds_create(UnixSockType::Stream).expect("srv");
        let cli1 = uds_create(UnixSockType::Stream).expect("c1");
        let cli2 = uds_create(UnixSockType::Stream).expect("c2");
        uds_bind(srv, b"/tmp/cancel.sock").expect("bind");
        uds_listen(srv).expect("listen");
        uds_connect(cli1, b"/tmp/cancel.sock").expect("c1 connect");
        uds_connect(cli2, b"/tmp/cancel.sock").expect("c2 connect");
        // 关闭 listener
        uds_close(srv).expect("close srv");
        // client 应该都已无效 (id=0)
        // 通过尝试 close 来验证 (close 已释放的 FD 应返回 BadFd)
        let r1 = uds_close(cli1);
        let r2 = uds_close(cli2);
        // client 槽位在 listener 关闭时已被清空, 二次 close 应返回 BadFd
        assert_eq!(r1, Err(UdsError::BadFd));
        assert_eq!(r2, Err(UdsError::BadFd));
    }
}
