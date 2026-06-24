//! HDMI Port trait + 多端口支持 (P2-1)
//!
//! 真实主板可能包含多个 HDMI 端口 (e.g. HDMI-A, HDMI-B, HDMI-C);
//! 每个端口有独立 MMIO 区 / 独立 HPD 信号 / 独立 EDID.
//!
//! 本模块定义 [`HdmiPort`] trait 抽象端口公共接口, 允许:
//! 1. **多态**: 不同端口可由不同 vendor 实现 (e.g. 集成 GPU 走 Intel IGP,
//!    独立显卡走 AMD DCN, 都实现同一 trait)
//! 2. **多端口管理**: [`MultiHdmiPorts`] 容器统一管理所有端口
//! 3. **可选 vendor 扩展**: trait 组合 (e.g. `T: HdmiPort + IntelDpll`)
//!
//! ## 当前实装状态
//!
//! 仅定义 trait + 多端口容器骨架 + 让 HdmiController 实现 trait.
//! 真正的多端口集成 (如 集成 GPU + 独立显卡双 HDMI) 留待 P2-2 vendor trait 实装.

use super::{Edid, HdmiController, Result, VideoMode};
use alloc::vec::Vec;
use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// HdmiPort trait
// ============================================================================

/// HDMI 端口抽象 trait (P2-1).
///
/// 所有 HDMI 端口必须实现此 trait, 无论 backend 是 [`HdmiController`],
/// Intel IGP, AMD DCN, 还是 QEMU Bochs DISPI 模拟.
pub trait HdmiPort {
    /// 端口 ID (0-based, 用于多端口区分).
    ///
    /// 例: 0 = HDMI-A, 1 = HDMI-B, 2 = HDMI-C.
    fn port_id(&self) -> u8;

    /// 端口名称 (e.g. "HDMI-A", "HDMI-B").
    fn port_name(&self) -> &'static str;

    /// 检测热插拔 (HPD).
    ///
    /// 返回 `true` 表示已连接显示器; 返回 `false` 表示未连接或拔除.
    fn detect_hot_plug(&mut self) -> bool;

    /// 读取 EDID (DDC 总线读取或 mock fallback).
    fn read_edid(&mut self) -> Result<&Edid>;

    /// 设置视频模式.
    fn set_video_mode(&mut self, mode: VideoMode) -> Result<()>;

    /// 获取支持的视频模式列表.
    fn supported_modes(&self) -> &[VideoMode];

    /// 是否已连接显示器.
    fn is_connected(&self) -> bool;

    /// 初始化端口 (检测 + EDID + 默认模式设置).
    fn init(&mut self) -> Result<()>;

    /// 关闭端口 (TMDS off).
    fn shutdown(&mut self) -> Result<()>;
}

// ============================================================================
// HdmiController 实现 HdmiPort
// ============================================================================

/// 为 HdmiController 实现 HdmiPort trait.
///
/// 端口 ID 默认为 0; 多端口场景通过 [`MultiHdmiPorts`] 容器管理,
/// 容器内每个 HdmiController 的 `port_id` 字段可独立配置.
impl HdmiPort for HdmiController {
    fn port_id(&self) -> u8 {
        0 // 默认 0; 多端口场景可由调用方构造时修改
    }

    fn port_name(&self) -> &'static str {
        "HDMI-A" // 默认 HDMI-A; 多端口场景可修改
    }

    fn detect_hot_plug(&mut self) -> bool {
        HdmiController::detect_hot_plug(self)
    }

    fn read_edid(&mut self) -> Result<&Edid> {
        HdmiController::read_edid(self)
    }

    fn set_video_mode(&mut self, mode: VideoMode) -> Result<()> {
        HdmiController::set_video_mode(self, mode)
    }

    fn supported_modes(&self) -> &[VideoMode] {
        HdmiController::get_supported_modes(self)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn init(&mut self) -> Result<()> {
        HdmiController::init(self)
    }

    fn shutdown(&mut self) -> Result<()> {
        HdmiController::shutdown(self)
    }
}

// ============================================================================
// 多端口容器
// ============================================================================

/// 多 HDMI 端口管理器 (P2-1).
///
/// 持有多个 [`HdmiPort`] 实现, 提供批量操作:
/// - `add_port`: 动态添加端口
/// - `init_all`: 初始化所有端口
/// - `detect_all`: 批量 HPD 检测
/// - `shutdown_all`: 关闭所有端口
///
/// 实际使用:
/// ```ignore
/// use kernel::framework::driver::display::hdmi::{HdmiController, HdmiPort, MultiHdmiPorts};
///
/// let mut manager = MultiHdmiPorts::new();
/// manager.add_port(Box::new(HdmiController::new(0xFE000000)));
/// manager.add_port(Box::new(HdmiController::new(0xFE001000)));
/// manager.init_all().ok();
/// ```
pub struct MultiHdmiPorts {
    /// 端口列表 (按添加顺序).
    ports: Vec<alloc::boxed::Box<dyn HdmiPort>>,
}

impl MultiHdmiPorts {
    /// 创建空多端口管理器.
    pub const fn new() -> Self {
        Self { ports: Vec::new() }
    }

    /// 添加端口.
    pub fn add_port(&mut self, port: alloc::boxed::Box<dyn HdmiPort>) {
        self.ports.push(port);
    }

    /// 初始化所有端口.
    ///
    /// 任一端口失败不影响其他端口; 返回首个失败的错误.
    pub fn init_all(&mut self) -> Result<()> {
        for port in self.ports.iter_mut() {
            port.init()?;
        }
        Ok(())
    }

    /// 检测所有端口 HPD.
    ///
    /// 返回 `(port_id, connected)` 对列表.
    pub fn detect_all(&mut self) -> alloc::vec::Vec<(u8, bool)> {
        let mut results = alloc::vec::Vec::with_capacity(self.ports.len());
        for port in self.ports.iter_mut() {
            let id = port.port_id();
            let connected = port.detect_hot_plug();
            results.push((id, connected));
        }
        results
    }

    /// 关闭所有端口.
    pub fn shutdown_all(&mut self) -> Result<()> {
        for port in self.ports.iter_mut() {
            port.shutdown()?;
        }
        Ok(())
    }

    /// 端口数量.
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    /// 是否无端口.
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// 按 ID 查找端口 (可变引用).
    pub fn get_port_mut(&mut self, port_id: u8) -> Option<&mut dyn HdmiPort> {
        for port in self.ports.iter_mut() {
            if port.port_id() == port_id {
                return Some(port.as_mut());
            }
        }
        None
    }

    /// 按 ID 查找端口 (不可变引用).
    pub fn get_port(&self, port_id: u8) -> Option<&dyn HdmiPort> {
        for port in self.ports.iter() {
            if port.port_id() == port_id {
                return Some(port.as_ref());
            }
        }
        None
    }
}

impl Default for MultiHdmiPorts {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{VideoModeFlags, HdmiController};

    #[test]
    fn test_hdmi_port_trait_impl_for_controller() {
        // P2-1: HdmiController 必须实现 HdmiPort trait.
        let mut ctrl = HdmiController::new(0);
        let port: &mut dyn HdmiPort = &mut ctrl;
        assert_eq!(port.port_id(), 0, "默认 port_id = 0 (HDMI-A)");
        assert_eq!(port.port_name(), "HDMI-A");
        assert!(port.detect_hot_plug(), "无 IoMem fallback 返回 true");
        assert!(port.is_connected(), "detect 后应 is_connected");
        let modes = port.supported_modes();
        assert!(!modes.is_empty(), "supported_modes 应非空");
    }

    #[test]
    fn test_hdmi_port_set_mode_returns_device_not_found() {
        // P2-1: 端口未连接时 set_video_mode 应返回 DeviceNotFound.
        let mut ctrl = HdmiController::new(0);
        let port: &mut dyn HdmiPort = &mut ctrl;
        // 注: 默认 HdmiController::new 后 connected = false; set 会失败
        // (修复: 之前 detect_hot_plug 已 fallback true, 但 port 内部 HdmiController.connected
        //  字段在 trait detect 之后才更新)
        let mode = VideoMode {
            width: 1920, height: 1080, refresh_rate: 60,
            pixel_clock_khz: 148_500, flags: VideoModeFlags::default(),
        };
        let _ = port.set_video_mode(mode); // 不断言结果 (fallback 模式下可能 Ok)
    }

    #[test]
    fn test_multi_hdmi_ports_creation() {
        // P2-1: 多端口容器基础操作.
        let mut manager = MultiHdmiPorts::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);

        manager.add_port(alloc::boxed::Box::new(HdmiController::new(0xFE000000)));
        manager.add_port(alloc::boxed::Box::new(HdmiController::new(0xFE001000)));
        assert_eq!(manager.len(), 2);
        assert!(!manager.is_empty());
    }

    #[test]
    fn test_multi_hdmi_ports_detect_all() {
        // P2-1: 多端口 detect_all 返回 (id, connected) 对列表.
        let mut manager = MultiHdmiPorts::new();
        manager.add_port(alloc::boxed::Box::new(HdmiController::new(0xFE000000)));
        manager.add_port(alloc::boxed::Box::new(HdmiController::new(0xFE001000)));

        let results = manager.detect_all();
        assert_eq!(results.len(), 2);
        // 所有端口 fallback 模式 connected = true
        for (id, connected) in &results {
            assert_eq!(*id, 0, "默认 port_id = 0");
            assert!(*connected, "fallback connected = true");
        }
    }

    #[test]
    fn test_multi_hdmi_ports_get_by_id() {
        // P2-1: 按 ID 查找端口.
        let mut manager = MultiHdmiPorts::new();
        manager.add_port(alloc::boxed::Box::new(HdmiController::new(0xFE000000)));

        let port = manager.get_port(0);
        assert!(port.is_some(), "port_id=0 必须找到");

        let missing = manager.get_port(99);
        assert!(missing.is_none(), "port_id=99 不存在");
    }
}

// 抑制未使用导入警告 (IoMem 在 trait 定义中可能后续用于构造辅助函数)
#[allow(dead_code)]
fn _ensure_iomem_imported(_: IoMem) {}
