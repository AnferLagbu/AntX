//! 设备树 (Device Tree) — services 层安全代理
//!
//! ## 状态 (v2.10, 2026-06-04)
//!
//! Phase 2.4 net/chitin 3/4 子系统迁移: 封装 `kernel::chitin::devtree::*`:
//! - [x] 节点查询 (root_id / find_compatible / find_by_name / get_node / count / children)
//! - [x] 节点属性 (add_prop / set_state / set_compatible / get_prop)
//! - [x] 用户态映射 (set_user_mapped / clear_user_mapped / clear_user_mapped_by_pid / get_user_mapped)
//! - [x] 地址/中断读取 (read_addr / read_irq)
//! - [x] 设备绑定 (bind_device / walk)
//!
//! ## 迁移方法
//!
//! 1. `unsafe impl Send/Sync for ChitinNode` 留在 framework 层 (内部)
//! 2. `pub fn devtree_*` 直接转发, services 层做参数验证
//! 3. `PropertyValue` 强类型 + `From` 转换器
//!
//! 评估日期: 2026-06-04

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::kernel::framework::chitin;
pub use crate::kernel::framework::chitin::devtree::{
    ChitinNode, NodeId, Property, PropertyValue,
};

// ============================================================================
// 强类型 ID
// ============================================================================

/// 设备树节点 ID (新类型包装, 替代裸 `u32`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct DevTreeNodeId(pub NodeId);

impl DevTreeNodeId {
    pub const ROOT: Self = Self(0);

    pub fn raw(self) -> NodeId {
        self.0
    }
}

// ============================================================================
// 设备树错误
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevTreeError {
    /// 节点未找到
    NotFound,
    /// 父节点不存在
    ParentNotFound,
    /// 无效参数
    InvalidArgument,
    /// 其他
    Other(i32),
}

impl DevTreeError {
    pub fn from_i32(rc: i32) -> Self {
        match rc {
            -2 => Self::NotFound,
            -22 => Self::InvalidArgument,
            _ => Self::Other(rc),
        }
    }
}

pub type DevTreeResult<T> = Result<T, DevTreeError>;

// ============================================================================
// 节点查询
// ============================================================================

/// 根节点 ID
pub fn root_id() -> DevTreeNodeId {
    DevTreeNodeId(chitin::devtree::devtree_root_id())
}

/// 按 compatible 字符串查找节点
pub fn find_compatible(compat: &str) -> Option<DevTreeNodeId> {
    chitin::devtree::devtree_find_compatible(compat)
        .map(DevTreeNodeId)
}

/// 按名称查找节点
pub fn find_by_name(name: &str) -> Option<DevTreeNodeId> {
    chitin::devtree::devtree_find_by_name(name)
        .map(DevTreeNodeId)
}

/// 获取节点完整快照
pub fn get_node(id: DevTreeNodeId) -> Option<ChitinNode> {
    chitin::devtree::devtree_get_node(id.0)
}

/// 子节点 ID 列表
pub fn children(id: DevTreeNodeId) -> Vec<DevTreeNodeId> {
    chitin::devtree::devtree_children(id.0)
        .into_iter()
        .map(DevTreeNodeId)
        .collect()
}

/// 节点总数
pub fn count() -> usize {
    chitin::devtree::devtree_count()
}

/// 遍历设备树 (DFS), 对每个节点调用回调
pub fn walk<F: FnMut(&ChitinNode)>(f: F) {
    chitin::devtree::devtree_walk(f);
}

// ============================================================================
// 属性读取
// ============================================================================

/// 读取 "reg" 属性的地址部分
pub fn read_addr(id: DevTreeNodeId) -> Option<u64> {
    chitin::devtree::devtree_read_addr(id.0)
}

/// 读取 "interrupts" 属性的 IRQ 号
pub fn read_irq(id: DevTreeNodeId) -> Option<u32> {
    chitin::devtree::devtree_read_irq(id.0)
}

/// 获取节点属性映射
pub fn properties(id: DevTreeNodeId) -> Option<BTreeMap<&'static str, PropertyValue>> {
    get_node(id).map(|n| n.properties)
}

// ============================================================================
// 节点管理 (framework 侧实现)
// ============================================================================

/// 添加属性到节点
///
/// # 注意
/// PropertyValue 仍需 `&'static str`/`&'static [u32]` 切片约束, 由 framework 侧持有
pub fn add_prop(id: DevTreeNodeId, name: &'static str, value: PropertyValue) {
    chitin::devtree::devtree_add_prop(id.0, name, value);
}

/// 设置 compatible 列表
pub fn set_compatible(id: DevTreeNodeId, compat: Vec<&'static str>) {
    chitin::devtree::devtree_set_compatible(id.0, compat);
}

/// 设置设备状态
pub fn set_state(id: DevTreeNodeId, state: super::DeviceState) {
    chitin::devtree::devtree_set_state(id.0, match state {
        super::DeviceState::Uninit => chitin::DeviceState::Uninit,
        super::DeviceState::Probing => chitin::DeviceState::Probing,
        super::DeviceState::Ready => chitin::DeviceState::Ready,
        super::DeviceState::Failed => chitin::DeviceState::Failed,
        super::DeviceState::Removed => chitin::DeviceState::Removed,
    });
}

// ============================================================================
// 用户态映射管理
// ============================================================================

/// 标记节点被指定 PID 的用户进程映射 (如用户态驱动 mmap)
pub fn set_user_mapped(id: DevTreeNodeId, pid: u32) {
    chitin::devtree::devtree_set_user_mapped(id.0, pid);
}

/// 清除节点的 user_mapped 标记
pub fn clear_user_mapped(id: DevTreeNodeId) {
    chitin::devtree::devtree_clear_user_mapped(id.0);
}

/// 进程退出时调用: 清除该 PID 在所有节点上的 user_mapped 标记
pub fn clear_user_mapped_by_pid(pid: u32) {
    chitin::devtree::devtree_clear_user_mapped_by_pid(pid);
}

/// 查询节点是否被某个 PID 映射
pub fn get_user_mapped(id: DevTreeNodeId) -> Option<u32> {
    chitin::devtree::devtree_get_user_mapped(id.0)
}

// ============================================================================
// 设备绑定
// ============================================================================

/// 将设备树节点绑定到 Chitin 全局设备表
///
/// # 参数
/// - `id`: 节点 ID
/// - `io_base`: MMIO 基地址 (None 表示纯中断设备)
/// - `irq`: 中断号
/// - `driver_data`: 驱动私有数据指针 (由驱动维护)
///
/// # 返回
/// 成功返回 Chitin 设备 ID, 失败返回 `DevTreeError::NotFound`
pub fn bind_device(
    id: DevTreeNodeId,
    io_base: Option<u64>,
    irq: Option<u8>,
    driver_data: *mut u8,
) -> DevTreeResult<u32> {
    match chitin::devtree::devtree_bind_device(id.0, io_base, irq, driver_data) {
        Some(dev_id) => Ok(dev_id),
        None => Err(DevTreeError::NotFound),
    }
}

// ============================================================================
// 初始化 / 调试
// ============================================================================

/// 初始化设备树 (创建根节点)
pub fn init() {
    chitin::devtree::devtree_init();
}

/// 打印设备树 (调试用)
pub fn print_tree() {
    chitin::devtree::devtree_print();
}

// ============================================================================
// 便利: 创建新节点
// ============================================================================

/// 创建设备树节点 (含父节点关联)
///
/// # 参数
/// - `name`: 节点名 (`&'static str`, 内部驻留)
/// - `proto`: 协议类型
/// - `parent_id`: 父节点 ID (None = 根节点)
///
/// # 返回
/// 成功返回新节点 ID, 失败返回 `DevTreeError::ParentNotFound`
pub fn create_node(
    name: &'static str,
    proto: super::Proto,
    parent_id: Option<DevTreeNodeId>,
) -> DevTreeResult<DevTreeNodeId> {
    let chitin_proto = match proto {
        super::Proto::Block => chitin::ChitinProto::Block,
        super::Proto::Char => chitin::ChitinProto::Char,
        super::Proto::Net => chitin::ChitinProto::Net,
        super::Proto::Input => chitin::ChitinProto::Input,
        super::Proto::Bus => chitin::ChitinProto::Bus,
        super::Proto::Other => chitin::ChitinProto::Other,
    };
    // SAFETY: devtree_create_node_impl 由 framework 侧管理, 内部锁保护
    match chitin::devtree::devtree_create_node_impl(name, chitin_proto, parent_id.map(|p| p.0)) {
        Some(id) => Ok(DevTreeNodeId(id)),
        None => Err(DevTreeError::ParentNotFound),
    }
}
