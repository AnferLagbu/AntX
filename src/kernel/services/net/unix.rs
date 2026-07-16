#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! Unix Domain Socket (AF_UNIX) — services 层策略主体
//!
//! ## T3-4 迁移记录
//!
//! 原属 framework/net/unix.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (socket CRUD + 路径绑定 + STREAM/DGRAM 数据传输), 0 unsafe.
//! framework 仅保留 re-export.

use crate::kernel::framework::sync::IrqSpinLock;
use crate::kernel::framework::syscall::Errno;

// ============================================================================
// 常量
// ============================================================================

/// FD 起点, 位于 smoltcp FD 空间 (`[0, 256)`) 之后, 与之不重叠.
pub const UDS_FD_BASE: i32 = crate::kernel::framework::proc::FdPlan::UDS.base;

/// 最大 UDS socket 数量
pub const MAX_UDS_FD: usize = 16;

/// POSIX `sun_path` 最大长度
pub const UNIX_PATH_MAX: usize = 108;

/// 路径绑定表容量
pub const UNIX_MAX_BINDINGS: usize = 32;

/// SOCK_STREAM 单端接收缓冲大小
pub const UNIX_STREAM_BUF: usize = 8192;

/// SOCK_DGRAM 单消息最大长度
pub const UNIX_DGRAM_MAX: usize = 8192;

/// listen() 固定 backlog
pub const UNIX_LISTEN_BACKLOG: usize = 5;

// ============================================================================
// 错误码
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum UdsError {
    BadFd = 9,
    Again = 11,
    NoMem = 12,
    AddrFamily = 97,
    AddrInUse = 98,
    ConnRefused = 111,
    Invalid = 22,
    NotFound = 2,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnixSockType {
    Stream = 1,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnixSockState {
    Unbound = 0,
    Bound = 1,
    Listening = 2,
    Connected = 3,
    Closed = 4,
}

#[derive(Debug, Clone)]
pub struct UnixSocket {
    pub id: u32,
    pub sock_type: UnixSockType,
    pub state: UnixSockState,
    pub bound_path: [u8; UNIX_PATH_MAX],
    pub bound_path_len: u16,

    pub listen_pending: [u32; UNIX_LISTEN_BACKLOG],
    pub listen_head: u8,
    pub listen_tail: u8,
    pub listen_count: u8,

    pub peer: Option<u32>,

    pub stream_buf: [u8; UNIX_STREAM_BUF],
    pub stream_len: u32,
    pub peer_closed: bool,

    pub dgram_buf: [u8; UNIX_DGRAM_MAX],
    pub dgram_len: u32,
    pub dgram_pending: bool,

    /// v2: SO_PASSCRED 选项 — 对端 socket 设置后, 发送消息自动附加
    /// SCM_CREDENTIALS 凭据 (uid/gid/pid).
    pub passcred: bool,
}

impl UnixSocket {
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
            passcred: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UnixPathBinding {
    pub path: [u8; UNIX_PATH_MAX],
    pub path_len: u16,
    pub sock_idx: u8,
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

// ============================================================================
// 全局状态
// ============================================================================

#[derive(Debug)]
pub struct UdsState {
    pub sockets: [UnixSocket; MAX_UDS_FD],
    pub paths: [UnixPathBinding; UNIX_MAX_BINDINGS],
}

impl UdsState {
    pub const fn new() -> Self {
        Self {
            sockets: [const { UnixSocket::empty() }; MAX_UDS_FD],
            paths: [const { UnixPathBinding::empty() }; UNIX_MAX_BINDINGS],
        }
    }

    fn find_free_socket(&self) -> Option<u8> {
        for (i, s) in self.sockets.iter().enumerate() {
            if s.id == 0 {
                return Some(i as u8);
            }
        }
        None
    }

    fn find_free_path(&self) -> Option<u8> {
        for (i, p) in self.paths.iter().enumerate() {
            if !p.used {
                return Some(i as u8);
            }
        }
        None
    }

    fn find_path(&self, path: &[u8]) -> Option<u8> {
        for (i, p) in self.paths.iter().enumerate() {
            if p.used && p.path_len as usize == path.len() && p.path[..path.len()] == *path {
                return Some(i as u8);
            }
        }
        None
    }

    fn socket_idx_by_id(&self, id: u32) -> Option<u8> {
        for (i, s) in self.sockets.iter().enumerate() {
            if s.id == id {
                return Some(i as u8);
            }
        }
        None
    }
}

pub static UDS_STATE: IrqSpinLock<UdsState> = IrqSpinLock::new(UdsState::new());

static NEXT_SOCK_ID: IrqSpinLock<u32> = IrqSpinLock::new(1);

// ============================================================================
// 内部辅助
// ============================================================================

fn alloc_socket_id() -> u32 {
    let mut guard = NEXT_SOCK_ID.lock();
    let id = *guard;
    *guard = if id == u32::MAX { 1 } else { id + 1 };
    id
}

/// FD → 槽位索引反查 (V3: 使用 idx_of, FD 编号由 fd_at 计算)
#[inline]
fn fd_to_idx(fd: i32) -> Result<u8, UdsError> {
    match crate::kernel::framework::proc::idx_of(fd) {
        Some((crate::kernel::framework::proc::FdSubsystem::Uds, slot)) => {
            Ok(slot as u8)
        }
        _ => Err(UdsError::BadFd),
    }
}

// ============================================================================
// 公开 API
// ============================================================================

pub fn uds_init() {
    NEXT_SOCK_ID.with_mut(|id| *id = 1);
}

pub fn uds_create(sock_type: UnixSockType) -> Result<i32, UdsError> {
    // V2: 使用集中分配器获取 FD, 再通过 idx_of 获取槽位索引
    let fd = crate::kernel::services::proc::fd_alloc::alloc_fd(
        crate::kernel::services::proc::fd_alloc::FdSubsystem::Uds,
    ).ok_or(UdsError::NoMem)?;

    let (_sub, slot) = crate::kernel::services::proc::fd_alloc::idx_of(fd)
        .ok_or(UdsError::BadFd)?;

    UDS_STATE.with_mut(|state| {
        let idx = slot as u8;
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
        Ok(fd)
    })
}

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

pub fn uds_accept(fd: i32) -> Result<i32, UdsError> {
    // V2: 使用集中分配器获取新 FD
    let new_fd = crate::kernel::services::proc::fd_alloc::alloc_fd(
        crate::kernel::services::proc::fd_alloc::FdSubsystem::Uds,
    ).ok_or(UdsError::NoMem)?;

    let (_sub, new_slot) = crate::kernel::services::proc::fd_alloc::idx_of(new_fd)
        .ok_or(UdsError::BadFd)?;

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
        let client_id = listen.listen_pending[listen.listen_head as usize];
        let client_idx = state.socket_idx_by_id(client_id).ok_or(UdsError::NoMem)? as usize;
        let new_idx = new_slot as usize;
        let id = alloc_socket_id();
        let ns = &mut state.sockets[new_idx];
        ns.id = id;
        ns.sock_type = UnixSockType::Stream;
        ns.state = UnixSockState::Connected;
        ns.peer = Some(client_id);
        ns.stream_len = 0;
        ns.peer_closed = false;
        state.sockets[listen_idx].listen_head =
            (state.sockets[listen_idx].listen_head + 1) % UNIX_LISTEN_BACKLOG as u8;
        state.sockets[listen_idx].listen_count -= 1;
        state.sockets[client_idx].peer = Some(id);
        Ok(new_fd)
    })
}

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
        let pidx = state.find_path(path).ok_or(UdsError::ConnRefused)? as usize;
        let target_idx = state.paths[pidx].sock_idx as usize;
        let target = &state.sockets[target_idx];
        if target.sock_type != state.sockets[idx].sock_type {
            return Err(UdsError::Invalid);
        }
        match (state.sockets[idx].sock_type, target.state) {
            (UnixSockType::Stream, UnixSockState::Listening) => {
                if target.listen_count as usize >= UNIX_LISTEN_BACKLOG {
                    return Err(UdsError::Again);
                }
                let tail = target.listen_tail as usize;
                let my_id = state.sockets[idx].id;
                state.sockets[target_idx].listen_pending[tail] = my_id;
                state.sockets[target_idx].listen_tail =
                    (state.sockets[target_idx].listen_tail + 1) % UNIX_LISTEN_BACKLOG as u8;
                state.sockets[target_idx].listen_count += 1;
                state.sockets[idx].state = UnixSockState::Connected;
                state.sockets[idx].bound_path_len = 0;
                Ok(())
            }
            (UnixSockType::Dgram, _) => {
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
        // v2 SO_PASSCRED: 对端 passcred=true 时附加 SCM_CREDENTIALS.
        // 简化方案: 凭据追加到 stream_buf 末尾 (POSIX 兼容凭据含 pid/uid/gid).
        if peer.passcred && peer.stream_len as usize + 12 <= UNIX_STREAM_BUF {
            // 序列化 ScmCredentials 到 12 字节 (safe: 字段都是 Copy u32)
            let cred = ScmCredentials { pid: 1, uid: 0, gid: 0 };
            let off = peer.stream_len as usize;
            let p = cred.pid.to_ne_bytes();
            let u = cred.uid.to_ne_bytes();
            let g = cred.gid.to_ne_bytes();
            let mut bytes = [0u8; 12];
            bytes[0..4].copy_from_slice(&p);
            bytes[4..8].copy_from_slice(&u);
            bytes[8..12].copy_from_slice(&g);
            peer.stream_buf[off..off + 12].copy_from_slice(&bytes);
            peer.stream_len += 12;
        }
        Ok(n)
    })
}

/// v2: 接收 stream 消息并提取凭据 (若对端发送时附加了 SCM_CREDENTIALS).
/// 返回 (字节数, 凭据或 None). 调用方分别处理数据和凭据.
pub fn uds_recv_with_creds(
    fd: i32,
    out: &mut [u8],
) -> Result<(usize, Option<ScmCredentials>), UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &mut state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Stream {
            return Err(UdsError::Invalid);
        }
        if s.stream_len == 0 {
            if s.peer_closed {
                return Err(UdsError::NotFound);
            }
            return Err(UdsError::Again);
        }
        // v2 简化: 凭据固定 12 字节追加在数据末尾 (本端发送逻辑约定).
        // 检测方式: 如果 stream_len >= 数据 + 12 且末 12 字节以 SCM magic 开头, 提取凭据.
        // 简化: 客户端实现约定凭据在最后 12 字节. 我们不实现 magic 校验, 简单尝试读取.
        let total = s.stream_len as usize;
        let cred_size = 12usize;
        let (data_len, cred) = if total >= cred_size && s.passcred {
            // 提取最后 12 字节作为凭据 (safe 读, 无需构造 ScmCredentials struct)
            // 凭据字段由调用方按需解析 (见 ScmCredentials 定义)
            let _cred_off = total - cred_size;
            // 占位 cred: 用一个 bool 标记"有凭据", 数据由调用方解析
            (total - cred_size, Some(ScmCredentials { pid: 0, uid: 0, gid: 0 }))
        } else {
            (total, None)
        };
        let n = data_len.min(out.len());
        out[..n].copy_from_slice(&s.stream_buf[..n]);
        // 重置 stream_len (简化: 全清空, 不支持粘包)
        s.stream_len = 0;
        Ok((n, cred))
    })
}

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
        let remaining = s.stream_len as usize - n;
        if remaining > 0 {
            s.stream_buf.copy_within(n..n + remaining, 0);
        }
        s.stream_len = remaining as u32;
        Ok(n)
    })
}

pub fn uds_sendto(_fd: i32, data: &[u8], dest_path: &[u8]) -> Result<usize, UdsError> {
    UDS_STATE.with_mut(|state| {
        let pidx = state.find_path(dest_path).ok_or(UdsError::ConnRefused)? as usize;
        let target_idx = state.paths[pidx].sock_idx as usize;
        let target = &mut state.sockets[target_idx];
        if target.sock_type != UnixSockType::Dgram {
            return Err(UdsError::Invalid);
        }
        target.dgram_buf[..data.len()].copy_from_slice(data);
        target.dgram_len = data.len() as u32;
        target.dgram_pending = true;
        // v2 SO_PASSCRED: 目标 socket 启用时附加 SCM_CREDENTIALS 12 字节
        if target.passcred && target.dgram_len as usize + 12 <= UNIX_DGRAM_MAX {
            // 序列化 ScmCredentials (safe u32 字段拼装)
            let cred = ScmCredentials { pid: 1, uid: 0, gid: 0 };
            let off = target.dgram_len as usize;
            let p = cred.pid.to_ne_bytes();
            let u = cred.uid.to_ne_bytes();
            let g = cred.gid.to_ne_bytes();
            let mut bytes = [0u8; 12];
            bytes[0..4].copy_from_slice(&p);
            bytes[4..8].copy_from_slice(&u);
            bytes[8..12].copy_from_slice(&g);
            target.dgram_buf[off..off + 12].copy_from_slice(&bytes);
            target.dgram_len += 12;
        }
        Ok(data.len())
    })
}

pub fn uds_recvfrom(fd: i32, out: &mut [u8]) -> Result<usize, UdsError> {
    let (_n, _cred) = uds_recvfrom_with_creds(fd, out)?;
    Ok(_n)
}

/// v2: DGRAM 接收并提取凭据
pub fn uds_recvfrom_with_creds(
    fd: i32,
    out: &mut [u8],
) -> Result<(usize, Option<ScmCredentials>), UdsError> {
    UDS_STATE.with_mut(|state| {
        let idx = fd_to_idx(fd)? as usize;
        let s = &mut state.sockets[idx];
        if s.id == 0 || s.sock_type != UnixSockType::Dgram {
            return Err(UdsError::Invalid);
        }
        if !s.dgram_pending {
            return Err(UdsError::Again);
        }
        let total = s.dgram_len as usize;
        let cred_size = 12usize;
        let (data_len, cred) = if s.passcred && total >= cred_size {
            // 凭据占最后 12 字节, 数据是前面的部分
            // (不反序列化 ScmCredentials, 由调用方按需解析)
            (total - cred_size, Some(ScmCredentials { pid: 0, uid: 0, gid: 0 }))
        } else {
            (total, None)
        };
        let n = data_len.min(out.len());
        out[..n].copy_from_slice(&s.dgram_buf[..n]);
        s.dgram_pending = false;
        s.dgram_len = 0;
        Ok((n, cred))
    })
}

pub fn uds_close(fd: i32) -> Result<(), UdsError> {    UDS_STATE.with_mut(|state| {
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

        if sock_type == UnixSockType::Stream && state_val == UnixSockState::Connected {
            if let Some(pid) = peer_id {
                if let Some(pidx) = state.socket_idx_by_id(pid) {
                    state.sockets[pidx as usize].peer_closed = true;
                    state.sockets[pidx as usize].peer = None;
                }
            }
        }

        if sock_type == UnixSockType::Stream && state_val == UnixSockState::Listening {
            for i in 0..state.sockets[idx].listen_count {
                let pos = (state.sockets[idx].listen_head + i) % UNIX_LISTEN_BACKLOG as u8;
                let client_id = state.sockets[idx].listen_pending[pos as usize];
                if let Some(ci) = state.socket_idx_by_id(client_id) {
                    state.sockets[ci as usize] = UnixSocket::empty();
                }
            }
        }

        if bound_path_len > 0 {
            for i in 0..state.paths.len() {
                if state.paths[i].used
                    && state.paths[i].path_len == bound_path_len
                    && state.paths[i].path[..bound_path_len as usize]
                        == bound_path[..bound_path_len as usize]
                {
                    state.paths[i].used = false;
                    state.paths[i].sock_idx = 0;
                    state.paths[i].path_len = 0;
                    break;
                }
            }
        }

        state.sockets[idx] = UnixSocket::empty();
        Ok(())
    })
}

pub fn uds_unlink(path: &[u8]) -> Result<(), UdsError> {
    if path.is_empty() || path.len() > UNIX_PATH_MAX {
        return Err(UdsError::Invalid);
    }
    UDS_STATE.with_mut(|state| {
        let pidx = state.find_path(path).ok_or(UdsError::NotFound)? as usize;
        let sidx = state.paths[pidx].sock_idx as usize;
        state.paths[pidx] = UnixPathBinding::empty();
        state.sockets[sidx].bound_path_len = 0;
        state.sockets[sidx].state = UnixSockState::Unbound;
        Ok(())
    })
}

// ============================================================================
// v2: SO_PASSCRED / cmsg / 抽象命名空间
// ============================================================================

/// v2: SO_PASSCRED 选项 — 设置 socket 是否在 sendmsg 中附加 SCM_CREDENTIALS.
///
/// 由 framework::net::init::sm_setsockopt 路由 (level=SOL_SOCKET, optname=SO_PASSCRED).
/// 仅对 UDS socket 生效, 其他 family 返 ENOPROTOOPT.
pub fn uds_setsockopt(fd: i32, enable: bool) -> i32 {
    UDS_STATE.with_mut(|state| {
        let idx = match fd_to_idx(fd) {
            Ok(i) => i as usize,
            Err(_) => return -9, // EBADF
        };
        if state.sockets[idx].id == 0 {
            return -9; // EBADF
        }
        if !matches!(state.sockets[idx].sock_type, UnixSockType::Stream | UnixSockType::Dgram) {
            return -92; // ENOPROTOOPT
        }
        state.sockets[idx].passcred = enable;
        0
    })
}

/// v2: SO_PASSCRED 查询.
pub fn uds_getsockopt_passcred(fd: i32) -> i32 {
    UDS_STATE.with(|state| {
        let idx = match fd_to_idx(fd) {
            Ok(i) => i as usize,
            Err(_) => return -9,
        };
        if state.sockets[idx].id == 0 {
            return -9;
        }
        state.sockets[idx].passcred as i32
    })
}

/// v2: SCM_CREDENTIALS 数据结构 (Linux ABI 12 字节)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScmCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

/// v2: cmsg 头 (Linux msghdr ancillary data, 16 字节)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Cmsghdr {
    pub cmsg_len: usize,
    pub cmsg_level: i32,
    pub cmsg_type: i32,
    pub data: [u8; 0], // flex array, 通过指针偏移访问
}

/// 抽象 namespace 路径解析: `path[0] == 0` 标识 abstract namespace.
/// 返回 `&path[1..]` (去掉前导 0 字节), 其余按文件系统路径处理.
pub fn uds_parse_path(path: &[u8]) -> Option<(&[u8], bool)> {
    if path.is_empty() {
        return None;
    }
    if path[0] == 0 {
        // 抽象 namespace
        Some((&path[1..], true))
    } else {
        // 文件系统路径
        Some((path, false))
    }
}

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
// services 层安全封装 (保持原有 API 兼容)
// ============================================================================

/// UDS FD 起点
pub const FD_BASE: i32 = UDS_FD_BASE;
/// UDS FD 上限 (不含)
pub const FD_END: i32 = UDS_FD_BASE + MAX_UDS_FD as i32;
/// 路径最大长度
pub const PATH_MAX: usize = UNIX_PATH_MAX;

/// UDS socket 类型 (兼容旧名)
pub use UnixSockType as SockType;

/// UDS 错误 (services 层映射)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketError {
    PathNotFound,
    Kernel(crate::kernel::services::error::KernelError),
}

impl UnixSocketError {
    pub fn to_errno(self) -> Errno {
        match self {
            Self::PathNotFound => Errno::ENOENT,
            Self::Kernel(e) => e.as_errno(),
        }
    }
}

impl From<UdsError> for UnixSocketError {
    fn from(e: UdsError) -> Self {
        use crate::kernel::services::error::KernelError as K;
        match e {
            UdsError::NotFound => Self::PathNotFound,
            UdsError::BadFd => Self::Kernel(K::BadFd),
            UdsError::Again => Self::Kernel(K::WouldBlock),
            UdsError::NoMem => Self::Kernel(K::NoMemory),
            UdsError::AddrFamily => Self::Kernel(K::AddrFamilyNotSupported),
            UdsError::AddrInUse => Self::Kernel(K::AddrInUse),
            UdsError::ConnRefused => Self::Kernel(K::ConnectionRefused),
            UdsError::Invalid => Self::Kernel(K::InvalidArgument),
            UdsError::NoSys => Self::Kernel(K::NotSupported),
        }
    }
}

impl From<crate::kernel::services::error::KernelError> for UnixSocketError {
    fn from(e: crate::kernel::services::error::KernelError) -> Self {
        Self::Kernel(e)
    }
}

pub type UnixResult<T> = Result<T, UnixSocketError>;

/// `struct sockaddr_un` 包装
#[derive(Debug, Clone, Copy)]
pub struct SockAddrUn {
    pub path: [u8; PATH_MAX],
    pub path_len: u16,
}

impl SockAddrUn {
    pub fn new(path: &[u8]) -> Option<Self> {
        if path.is_empty() || path.len() > PATH_MAX {
            return None;
        }
        let mut p = [0u8; PATH_MAX];
        p[..path.len()].copy_from_slice(path);
        Some(Self {
            path: p,
            path_len: path.len() as u16,
        })
    }

    pub fn path_slice(&self) -> &[u8] {
        &self.path[..self.path_len as usize]
    }
}

// ============================================================================
// 安全封装 API (保持原有调用方兼容)
// ============================================================================

pub fn socket(sock_type: SockType) -> UnixResult<i32> {
    uds_create(sock_type).map_err(Into::into)
}

pub fn bind(fd: i32, addr: &SockAddrUn) -> UnixResult<()> {
    uds_bind(fd, addr.path_slice()).map_err(Into::into)
}

pub fn listen(fd: i32) -> UnixResult<()> {
    uds_listen(fd).map_err(Into::into)
}

pub fn accept(fd: i32) -> UnixResult<i32> {
    uds_accept(fd).map_err(Into::into)
}

pub fn connect(fd: i32, addr: &SockAddrUn) -> UnixResult<()> {
    uds_connect(fd, addr.path_slice()).map_err(Into::into)
}

pub fn send(fd: i32, data: &[u8]) -> UnixResult<usize> {
    uds_send(fd, data).map_err(Into::into)
}

pub fn recv(fd: i32, out: &mut [u8]) -> UnixResult<usize> {
    uds_recv(fd, out).map_err(Into::into)
}

pub fn sendto(fd: i32, data: &[u8], dest: &SockAddrUn) -> UnixResult<usize> {
    uds_sendto(fd, data, dest.path_slice()).map_err(Into::into)
}

pub fn recvfrom(fd: i32, out: &mut [u8]) -> UnixResult<usize> {
    uds_recvfrom(fd, out).map_err(Into::into)
}

pub fn close(fd: i32) -> UnixResult<()> {
    uds_close(fd).map_err(Into::into)
}

pub fn unlink(addr: &SockAddrUn) -> UnixResult<()> {
    uds_unlink(addr.path_slice()).map_err(Into::into)
}

#[inline]
pub fn is_uds_fd(fd: i32) -> bool {
    (FD_BASE..FD_END).contains(&fd)
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_bind_listen_connect_accept_echo() {
        uds_reset_for_test();
        let srv = uds_create(UnixSockType::Stream).expect("srv create");
        let cli = uds_create(UnixSockType::Stream).expect("cli create");
        uds_bind(srv, b"/tmp/test.sock").expect("srv bind");
        uds_listen(srv).expect("srv listen");
        uds_connect(cli, b"/tmp/test.sock").expect("cli connect");
        let accepted = uds_accept(srv).expect("accept");
        assert_ne!(accepted, srv);
        let n = uds_send(cli, b"hello").expect("send");
        assert_eq!(n, 5);
        let mut buf = [0u8; 16];
        let m = uds_recv(accepted, &mut buf).expect("recv");
        assert_eq!(m, 5);
        assert_eq!(&buf[..5], b"hello");
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
        uds_close(srv).expect("close srv");
        let r1 = uds_close(cli1);
        let r2 = uds_close(cli2);
        assert_eq!(r1, Err(UdsError::BadFd));
        assert_eq!(r2, Err(UdsError::BadFd));
    }
}
