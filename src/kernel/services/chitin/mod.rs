#![deny(unsafe_code)]
//! Chitin 设备驱动框架 — services 层安全代理
//!
//! ## 状态 (v2.7, 2026-06-04)
//!
//! 已完成 1/4 子系统迁移 (chitin 整体), 封装 `kernel::chitin::*` 老 API:
//! - [x] chitin (本文件) — 设备注册表/查找/块设备 IO/字符设备 IO/输入设备
//! - [ ] devtree — 设备树 (后续 Phase 2.4.x)
//! - [ ] composite — 复合设备 (后续 Phase 2.4.x)
//! - [ ] proto_* — 协议族 (后续 Phase 2.4.x)
//! - [ ] user_driver — 用户态驱动 (后续 Phase 2.4.x)
//!
//! ## 迁移方法
//!
//! 1. 把 `i32` 错误码 → `Result<_, ChitinError>` 强类型
//! 2. 把设备 ID `u32` → `DeviceId` 新类型
//! 3. 块设备 IO 用 `&mut [u8]`/`&[u8]` 切片替代裸指针
//! 4. 0 unsafe 出现在 services 层
//!
//! 评估日期: 2026-06-04

extern crate alloc;

use alloc::vec::Vec;

use crate::kernel::framework::chitin;

pub mod devtree;
pub mod composite;
pub use devtree::{
    ChitinNode, DevTreeError, DevTreeNodeId, DevTreeResult, NodeId,
    Property, PropertyValue,
    root_id, find_compatible, get_node, children, walk,
    read_addr, read_irq, properties,
    add_prop, set_compatible,
    set_user_mapped, clear_user_mapped, clear_user_mapped_by_pid, get_user_mapped,
    bind_device, init as devtree_init, print_tree, create_node,
};
pub use composite::{probe as composite_probe, probe_init as composite_probe_init};

// ============================================================================
// 错误
// ============================================================================

/// Chitin 操作错误 (强类型, 替代内核 `i32`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChitinError {
    /// 设备未找到
    NotFound,
    /// 设备已注册
    AlreadyExists,
    /// IO 失败
    Io,
    /// 无效参数
    InvalidArgument,
    /// 表已满
    NoResources,
    /// 设备未就绪
    NotReady,
    /// 设备类型不匹配
    WrongType,
    /// 权限不足
    PermissionDenied,
    /// 其他
    Other(i32),
}

impl ChitinError {
    pub fn from_i32(rc: i32) -> Self {
        match rc {
            -2 => Self::NotFound,
            -17 => Self::AlreadyExists,
            -5 => Self::Io,
            -22 => Self::InvalidArgument,
            -28 => Self::NoResources,
            -19 => Self::NotReady,
            -1 => Self::Other(rc),
            _ => Self::Other(rc),
        }
    }
}

/// services 层结果类型别名
pub type ChitinResult<T> = Result<T, ChitinError>;

// ============================================================================
// 设备 ID
// ============================================================================

/// Chitin 设备 ID (强类型, 替代裸 `u32`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct DeviceId(pub u32);

impl DeviceId {
    /// 原始 ID
    pub fn raw(&self) -> u32 { self.0 }
}

// ============================================================================
// 协议类型
// ============================================================================

/// 设备协议 (与 kernel::chitin::ChitinProto 对齐)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Proto {
    Block = 1,
    Char = 2,
    Net = 3,
    Input = 4,
    Bus = 5,
    Other = 255,
}

impl Proto {
    pub fn as_str(&self) -> &'static str {
        match self {
            Proto::Block => "block",
            Proto::Char => "char",
            Proto::Net => "net",
            Proto::Input => "input",
            Proto::Bus => "bus",
            Proto::Other => "other",
        }
    }
}

impl From<chitin::ChitinProto> for Proto {
    fn from(p: chitin::ChitinProto) -> Self {
        match p {
            chitin::ChitinProto::Block => Proto::Block,
            chitin::ChitinProto::Char => Proto::Char,
            chitin::ChitinProto::Net => Proto::Net,
            chitin::ChitinProto::Input => Proto::Input,
            chitin::ChitinProto::Bus => Proto::Bus,
            chitin::ChitinProto::Other => Proto::Other,
        }
    }
}

/// 设备状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Uninit,
    Probing,
    Ready,
    Failed,
    Removed,
}

impl From<chitin::DeviceState> for DeviceState {
    fn from(s: chitin::DeviceState) -> Self {
        match s {
            chitin::DeviceState::Uninit => Self::Uninit,
            chitin::DeviceState::Probing => Self::Probing,
            chitin::DeviceState::Ready => Self::Ready,
            chitin::DeviceState::Failed => Self::Failed,
            chitin::DeviceState::Removed => Self::Removed,
        }
    }
}

/// 设备描述符
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: alloc::string::String,
    pub proto: Proto,
    pub state: DeviceState,
}

// ============================================================================
// 注册表 API
// ============================================================================

/// 注册设备 (通用)
///
/// # 注意
/// 由于内核 `chitin_register` 需要 `&'static str`, services 层无法直接传 `&str`。
/// 调用方应在启动期 (`&'static` 上下文) 直接调用 `chitin::chitin_register`,
/// 或使用本函数 (内部泄漏 `Box<str>` 转 `&'static str`, 仅用于一次性注册)。
pub fn register(
    name: &str,
    proto: Proto,
    driver_data: *mut u8,
) -> ChitinResult<DeviceId> {
    let chitin_proto = match proto {
        Proto::Block => chitin::ChitinProto::Block,
        Proto::Char => chitin::ChitinProto::Char,
        Proto::Net => chitin::ChitinProto::Net,
        Proto::Input => chitin::ChitinProto::Input,
        Proto::Bus => chitin::ChitinProto::Bus,
        Proto::Other => chitin::ChitinProto::Other,
    };
    // SAFETY leak: 仅启动期一次性使用, 名称永久驻留
    let leaked: &'static str = alloc::string::String::from(name).leak();
    let id = chitin::chitin_register(leaked, chitin_proto, None, None, driver_data);
    if id == u32::MAX {
        Err(ChitinError::NoResources)
    } else {
        Ok(DeviceId(id))
    }
}

/// 按 ID 查找
pub fn find_by_id(id: DeviceId) -> Option<usize> {
    chitin::chitin_find_by_id(id.0)
}

/// 按名称查找
pub fn find_by_name(name: &str) -> Option<usize> {
    chitin::chitin_find_by_name(name)
}

/// 按协议查找第一个
pub fn find_by_proto(proto: Proto) -> Option<usize> {
    let p = match proto {
        Proto::Block => chitin::ChitinProto::Block,
        Proto::Char => chitin::ChitinProto::Char,
        Proto::Net => chitin::ChitinProto::Net,
        Proto::Input => chitin::ChitinProto::Input,
        Proto::Bus => chitin::ChitinProto::Bus,
        Proto::Other => chitin::ChitinProto::Other,
    };
    chitin::chitin_find_by_proto(p)
}

/// 列出所有设备
pub fn list() -> Vec<DeviceInfo> {
    let raw = chitin::chitin_list();
    raw.into_iter()
        .map(|(id, name, proto, state)| DeviceInfo {
            id: DeviceId(id),
            name: alloc::string::String::from(name),
            proto: Proto::from(proto),
            state: DeviceState::from(state),
        })
        .collect()
}

/// 设备总数
pub fn count() -> usize {
    chitin::chitin_count()
}

/// 按协议计数
pub fn count_by_proto(proto: Proto) -> usize {
    let p = match proto {
        Proto::Block => chitin::ChitinProto::Block,
        Proto::Char => chitin::ChitinProto::Char,
        Proto::Net => chitin::ChitinProto::Net,
        Proto::Input => chitin::ChitinProto::Input,
        Proto::Bus => chitin::ChitinProto::Bus,
        Proto::Other => chitin::ChitinProto::Other,
    };
    chitin::chitin_count_by_proto(p)
}

/// 查找网络设备 (返回 (NetOps, driver_data, mac))
pub fn find_net_device() -> Option<(
    &'static crate::kernel::framework::chitin::proto_net::NetOps,
    *mut u8,
    [u8; 6],
)> {
    chitin::chitin_find_net_device()
}

/// 注销设备
pub fn unregister(id: DeviceId) -> Option<*mut u8> {
    chitin::chitin_unregister(id.0)
}

/// 设置设备状态
pub fn set_state(id: DeviceId, state: DeviceState) {
    let s = match state {
        DeviceState::Uninit => chitin::DeviceState::Uninit,
        DeviceState::Probing => chitin::DeviceState::Probing,
        DeviceState::Ready => chitin::DeviceState::Ready,
        DeviceState::Failed => chitin::DeviceState::Failed,
        DeviceState::Removed => chitin::DeviceState::Removed,
    };
    chitin::chitin_set_state(id.0, s);
}

/// 初始化所有设备
pub fn init_all() {
    chitin::chitin_init_all();
}

/// 关闭所有设备
pub fn shutdown_all() {
    chitin::chitin_shutdown_all();
}

// ============================================================================
// 块设备 IO (按 drive 索引)
// ============================================================================

/// 读块设备
///
/// # 参数
/// - `drive`: 块设备索引
/// - `sector`: 起始扇区
/// - `buf`: 接收缓冲区 (至少 512 字节)
pub fn blk_read(drive: u8, sector: u64, buf: &mut [u8]) -> ChitinResult<()> {
    let rc = chitin::chitin_blk_read(drive, sector, buf);
    if rc == 0 { Ok(()) } else { Err(ChitinError::from_i32(rc)) }
}

/// 写块设备
pub fn blk_write(drive: u8, sector: u64, buf: &[u8]) -> ChitinResult<()> {
    let rc = chitin::chitin_blk_write(drive, sector, buf);
    if rc == 0 { Ok(()) } else { Err(ChitinError::from_i32(rc)) }
}

/// 块设备是否存在
pub fn blk_is_present(drive: u8) -> bool {
    chitin::chitin_blk_is_present(drive)
}

/// 块设备总扇区数
pub fn blk_total_sectors(drive: u8) -> u64 {
    chitin::chitin_blk_total_sectors(drive)
}

/// 块设备名称
pub fn blk_name(drive: u8) -> Option<alloc::string::String> {
    chitin::chitin_blk_name(drive).map(alloc::string::String::from)
}

/// 块设备信息
pub fn blk_info(drive: u8) -> Option<(alloc::string::String, bool, u64)> {
    let (name, present, sectors) = chitin::chitin_blk_info(drive);
    Some((alloc::string::String::from(name), present, sectors))
}

/// 块设备总数
pub fn blk_count() -> usize {
    chitin::chitin_blk_count()
}

// ============================================================================
// 字符设备 IO
// ============================================================================

/// 字符设备写
pub fn char_write(data: &[u8]) {
    chitin::chitin_char_write(data);
}

/// 字符设备读
pub fn char_read(buf: &mut [u8]) -> usize {
    chitin::chitin_char_read(buf)
}

// ============================================================================
// 输入设备
// ============================================================================

/// 输入设备读 (非阻塞)
pub fn input_read() -> Option<u8> {
    chitin::chitin_input_read()
}

/// 输入设备是否有数据
pub fn input_has_data() -> bool {
    chitin::chitin_input_has_data()
}
