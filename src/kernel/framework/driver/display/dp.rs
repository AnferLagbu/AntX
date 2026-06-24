//! DisplayPort 驱动 (DisplayPort Driver)
//!
//! 提供DisplayPort显示接口支持：
//! - **AUX通道**: 边带通信通道
//! - **链路训练**: 链路初始化和优化
//! - **DPCD读取**: 显示器配置数据
//! - **MST支持**: 多流传输
//!
//! ## 硬件接口
//!
//! ```text
//! DisplayPort:
//! ├── Main Link: 1/2/4通道，每通道5.4/8.1 Gbps
//! ├── AUX Channel: 边带通信 (1 Mbps)
//! ├── HPD: 热插拔检测
//! └── DPCD: 显示端口配置数据
//! ```
//!
//! # Safety
//! DisplayPort驱动涉及高速串行通信和链路训练。

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use crate::kernel::framework::iomem::IoMem;
use alloc::vec;
use alloc::vec::Vec;

// ============================================================================
// DisplayPort 常量定义
// ============================================================================

/// DisplayPort DPCD 地址 — VESA DP 规范 §2.4
///
/// 当前使用: TRAINING_PTN_SET, LINK_BW_SET, LANE_COUNT_SET
/// 其余 DPCD 字段供参考: 接收器能力、链路训练状态、Sink 状态等
mod aux_address {
    pub const TRAINING_PTN_SET: u16 = 0x0106;
    pub const LINK_BW_SET: u16 = 0x0100;
    pub const LANE_COUNT_SET: u16 = 0x0101;
}

/// DP HPD (Hot Plug Detect) 状态寄存器偏移。
///
/// 厂商差异:
/// - Intel IGP: 与 HDMI 共享 HPD 寄存器 (MMIO +0xC8 bit 0-3), 调用方应传入与 HDMI 相同偏移
/// - AMD DCN: 与 HDMI 类似, 通常共享 HPD 控制器
/// - 独立 DP 控制器 (e.g. 板载 DP chip): 单独的 HPD GPIO/状态寄存器, 默认偏移 0x040
///
/// 本实装默认偏移 0x040, 假设 DP 控制器为独立 chip;
/// 与 HDMI 共享 HPD 的厂商应通过 [`DpController::new_with_iomem`] 显式指定偏移。
const DP_HPD_REG_OFFSET: usize = 0x040;

/// 当前实装阶段 (仅 HPD) DP 控制器所需的最小 IoMem 大小.
///
/// P0-2: 文档化 IoMem 最小大小, 消除隐式约定风险.
/// P1-4: 提供 [`assert_iomem_size_at_least`] 编译期检查辅助函数.
///
/// 实际映射需求: HPD 寄存器 (0x040, 1 字节) → 至少 0x041.
///
/// 未来扩展预留 (DISPLAY-2.5 / 2.6 / 2.7 实装时同步增大):
/// - AUX 通道: 0x100..=0x110 (16 字节, 含 CMD/STA/DAT0-7)
/// - 链路训练状态: 0x200..=0x210
/// - 链路训练调整: 0x300..=0x310
///
/// 当前常量 0x041 已满足 HPD-only 阶段; 后续实装会同步调整.
pub const REQUIRED_IOMEM_SIZE: usize = 0x041;

/// 编译期检查 IoMem 大小 (P1-4).
///
/// 当 `size` 是 const 表达式且 `size < REQUIRED_IOMEM_SIZE` 时, 编译期 panic;
/// 否则零运行时开销. 用法见 hdmi.rs 对应函数.
#[inline]
pub const fn assert_iomem_size_at_least(size: usize) {
    if size < REQUIRED_IOMEM_SIZE {
        panic!("IoMem size must be >= DpController::REQUIRED_IOMEM_SIZE");
    }
}

/// DP HPD 状态位 (bit 0)
const DP_HPD_STATUS_BIT: u8 = 0x01;

/// 链路速率
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LinkRate {
    Rbr = 0x06,  // 1.62 Gbps per lane
    Hbr = 0x0A,  // 2.7 Gbps per lane
    Hbr2 = 0x14, // 5.4 Gbps per lane
    Hbr3 = 0x1E, // 8.1 Gbps per lane
}

impl LinkRate {
    pub fn bandwidth_gbps(&self) -> u32 {
        match self {
            Self::Rbr => 162,
            Self::Hbr => 270,
            Self::Hbr2 => 540,
            Self::Hbr3 => 810,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x06 => Some(Self::Rbr),
            0x0A => Some(Self::Hbr),
            0x14 => Some(Self::Hbr2),
            0x1E => Some(Self::Hbr3),
            _ => None,
        }
    }
}

/// 通道数量
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LaneCount {
    One = 1,
    Two = 2,
    Four = 4,
}

impl LaneCount {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

/// 链路训练状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingState {
    Disabled,
    Training1,
    Training2,
    Trained,
    Error,
}

// ============================================================================
// DPCD 数据结构
// ============================================================================

/// DisplayPort配置数据 (DPCD)
#[derive(Debug, Clone)]
pub struct Dpcd {
    /// DPCD版本
    pub revision: u8,
    /// 最大链路速率
    pub max_link_rate: LinkRate,
    /// 最大通道数
    pub max_lane_count: LaneCount,
    /// 是否支持下行扩频
    pub max_downspread: bool,
    /// 是否支持MST
    pub mst_capable: bool,
    /// 是否支持增强帧
    pub enhanced_frame_capable: bool,
    /// TPS3支持
    pub tps3_supported: bool,
    /// 接收器数量
    pub sink_count: u8,
}

impl Dpcd {
    /// 从AUX读取的数据解析DPCD
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 16 {
            return Err(DriverError::BufferTooSmall);
        }

        let revision = data[0];
        let max_link_rate = LinkRate::from_u8(data[1]).ok_or(DriverError::InvalidParameter)?;
        let max_lane_count =
            LaneCount::from_u8(data[2] & 0x1F).ok_or(DriverError::InvalidParameter)?;

        Ok(Self {
            revision,
            max_link_rate,
            max_lane_count,
            max_downspread: (data[3] & 0x01) != 0,
            mst_capable: (data[3] & 0x04) != 0,
            enhanced_frame_capable: (data[2] & 0x80) != 0,
            tps3_supported: (data[4] & 0x40) != 0,
            sink_count: data[5] & 0x3F,
        })
    }
}

// ============================================================================
// AUX 通道操作
// ============================================================================

/// AUX命令类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuxCommand {
    I2cWrite = 0x00,
    I2cRead = 0x01,
    I2cWriteStatus = 0x02,
    I2cReadStatus = 0x03,
    Write = 0x04,
    Read = 0x05,
}

/// AUX事务结果
#[derive(Debug, Clone, Copy)]
pub struct AuxTransaction {
    pub command: AuxCommand,
    pub address: u16,
    pub length: u8,
    pub data: [u8; 16],
    pub bytes_read: usize,
}

// ============================================================================
// DisplayPort 控制器
// ============================================================================

/// DisplayPort 控制器驱动
pub struct DpController {
    /// MMIO 句柄 (Option, 在无硬件/虚拟化环境为 None)。
    ///
    /// - `Some(iomem)`: 真实硬件路径, 通过 MMIO 寄存器读取 HPD 状态。
    /// - `None`: 无硬件路径 (QEMU/QEMU+bochs-vbe), HPD 检测走 fallback
    ///   (假设已连接), 仅用于开发环境。
    iomem: Option<IoMem>,
    /// HPD 寄存器偏移 (相对 `iomem` 基地址)。
    /// 不同厂商 DP 控制器偏移量不同; 默认 0x040 (假设独立 DP chip),
    /// 调用方可通过 [`DpController::new_with_iomem`] 指定自家硬件偏移。
    hpd_reg_offset: usize,
    /// DPCD数据
    dpcd: Option<Dpcd>,
    /// 当前链路速率
    current_link_rate: Option<LinkRate>,
    /// 当前通道数
    current_lane_count: Option<LaneCount>,
    /// 链路训练状态
    training_state: TrainingState,
    /// 是否连接显示器
    connected: bool,
    /// 设备信息 (待驱动框架 Device trait 集成后使用)。
    #[allow(dead_code)] // 待驱动框架 Device trait 集成后使用。
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl DpController {
    /// 创建 DisplayPort 控制器实例 (无硬件 fallback 模式)。
    ///
    /// 此模式 `detect_hot_plug` 直接返回 `true` (假设已连接),
    /// 用于 QEMU/QEMU+bochs-vbe 等无真实 DP 控制器的开发环境。
    /// 真实硬件环境请使用 [`DpController::new_with_iomem`].
    pub fn new(mmio_base_unused: usize) -> Self {
        // 参数保留以兼容旧调用方; iomem 路径走 fallback。
        let _ = mmio_base_unused;
        Self {
            iomem: None,
            hpd_reg_offset: DP_HPD_REG_OFFSET,
            dpcd: None,
            current_link_rate: None,
            current_lane_count: None,
            training_state: TrainingState::Disabled,
            connected: false,
            info: DeviceInfo::new("displayport", DeviceType::Other),
            initialized: false,
        }
    }

    /// 创建 DisplayPort 控制器实例 (真实硬件模式)。
    ///
    /// # Safety
    ///
    /// - `iomem` 必须指向有效 DP 控制器 MMIO 区域;
    /// - 调用方负责 `iomem` 的生命周期管理 (在 `DpController` 存活期间不得释放);
    /// - `iomem.len() >= REQUIRED_IOMEM_SIZE` (0x041), 当前仅覆盖 HPD 寄存器;
    /// - `hpd_reg_offset + 1` 必须落在 `iomem` 范围内。
    ///
    /// 推荐: `DpController::new_with_iomem(IoMem::new(base, REQUIRED_IOMEM_SIZE), DP_HPD_REG_OFFSET)`。
    pub unsafe fn new_with_iomem(
        iomem: IoMem,
        hpd_reg_offset: usize,
    ) -> Self {
        // P1-4: debug 构建 IoMem 大小检查 (release 零开销).
        debug_assert!(
            iomem.len() >= REQUIRED_IOMEM_SIZE,
            "DpController 需要 IoMem >= {} 字节, got {}",
            REQUIRED_IOMEM_SIZE,
            iomem.len()
        );
        Self {
            iomem: Some(iomem),
            hpd_reg_offset,
            dpcd: None,
            current_link_rate: None,
            current_lane_count: None,
            training_state: TrainingState::Disabled,
            connected: false,
            info: DeviceInfo::new("displayport", DeviceType::Other),
            initialized: false,
        }
    }

    /// 创建 DisplayPort 控制器实例 (真实硬件, 使用默认 HPD 寄存器偏移)。
    ///
    /// # Safety
    ///
    /// 同 [`DpController::new_with_iomem`], 默认偏移见 [`DP_HPD_REG_OFFSET`].
    /// 即 `iomem.len() >= REQUIRED_IOMEM_SIZE` (0x041).
    pub unsafe fn new_with_default_hpd(iomem: IoMem) -> Self {
        Self::new_with_iomem(iomem, DP_HPD_REG_OFFSET)
    }

    /// 检测热插拔。
    ///
    /// 真实硬件: 从 MMIO 读 `hpd_reg_offset` 寄存器, bit 0 == 1 表示已连接。
    /// 无硬件 fallback: 返回 `true` (兼容 QEMU + Bochs DISPI 开发环境)。
    pub fn detect_hot_plug(&mut self) -> bool {
        let hpd = if let Some(iomem) = &self.iomem {
            // SAFETY: `new_with_iomem` 构造时调用方已保证
            // `hpd_reg_offset + 1 <= iomem.len()`, 读 1 字节落在 IoMem 边界内。
            unsafe { iomem.read_u8(self.hpd_reg_offset) & DP_HPD_STATUS_BIT != 0 }
        } else {
            // 无硬件 fallback: 假设已连接 (兼容 QEMU Bochs DISPI 开发环境)。
            true
        };
        self.connected = hpd;
        hpd
    }

    /// AUX通道读操作
    pub fn aux_read(&mut self, address: u16, length: u8) -> Result<Vec<u8>> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        // TODO(TRACK-B61830): 实现实际的AUX通道读取
        // 这里返回模拟的DPCD数据
        let mut data = vec![0u8; length as usize];

        if address == 0x0000 && length >= 16 {
            // 模拟DPCD数据
            data[0] = 0x12; // DPCD rev 1.2
            data[1] = LinkRate::Hbr2 as u8; // 5.4 Gbps
            data[2] = 0x84; // 4 lanes, enhanced frame
            data[3] = 0x01; // downspread supported
            data[4] = 0x00;
            data[5] = 0x01; // 1 sink
        }

        Ok(data)
    }

    /// AUX通道写操作
    pub fn aux_write(&mut self, _address: u16, _data: &[u8]) -> Result<()> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        // TODO(TRACK-9B691E): 实现实际的AUX通道写入

        Ok(())
    }

    /// 读取DPCD
    pub fn read_dpcd(&mut self) -> Result<&Dpcd> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        let data = self.aux_read(0x0000, 16)?;
        let dpcd = Dpcd::parse(&data)?;
        self.dpcd = Some(dpcd);

        // SAFETY: 刚在上面设为 Some, 不会为 None
        Ok(self.dpcd.as_ref().expect("dp: dpcd 刚已设为 Some"))
    }

    /// 链路训练
    pub fn link_train(&mut self) -> Result<()> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        let dpcd = self.dpcd.as_ref().ok_or(DriverError::NotInitialized)?;

        // 选择链路速率和通道数
        let link_rate = dpcd.max_link_rate;
        let lane_count = dpcd.max_lane_count;

        // 阶段1: 链路训练模式1
        self.training_state = TrainingState::Training1;
        self.training_phase1(link_rate, lane_count)?;

        // 阶段2: 链路训练模式2
        self.training_state = TrainingState::Training2;
        self.training_phase2(link_rate, lane_count)?;

        // 训练完成
        self.current_link_rate = Some(link_rate);
        self.current_lane_count = Some(lane_count);
        self.training_state = TrainingState::Trained;

        Ok(())
    }

    /// 链路训练阶段1
    fn training_phase1(&mut self, link_rate: LinkRate, lane_count: LaneCount) -> Result<()> {
        // 设置链路速率和通道数
        self.aux_write(aux_address::LINK_BW_SET, &[link_rate as u8])?;
        self.aux_write(aux_address::LANE_COUNT_SET, &[lane_count as u8])?;

        // 设置训练模式
        self.aux_write(aux_address::TRAINING_PTN_SET, &[0x21])?;

        // 等待训练完成
        // TODO(TRACK-0350FE): 轮询LANE0_1_STATUS寄存器

        Ok(())
    }

    /// 链路训练阶段2
    fn training_phase2(&mut self, _link_rate: LinkRate, _lane_count: LaneCount) -> Result<()> {
        // 设置训练模式2
        self.aux_write(aux_address::TRAINING_PTN_SET, &[0x22])?;

        // 等待训练完成
        // TODO(TRACK-3C1169): 轮询LANE_ALIGN_STATUS_UPDATED寄存器

        // 结束训练
        self.aux_write(aux_address::TRAINING_PTN_SET, &[0x00])?;

        Ok(())
    }

    /// 获取当前带宽 (Gbps)
    pub fn get_bandwidth_gbps(&self) -> Option<u32> {
        let rate = self.current_link_rate?;
        let lanes = self.current_lane_count?;

        Some(rate.bandwidth_gbps() * lanes as u32)
    }

    /// 检查链路是否已训练
    pub fn is_link_trained(&self) -> bool {
        self.training_state == TrainingState::Trained
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for DpController {
    fn name(&self) -> &'static str {
        "DisplayPort Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Other
    }

    fn init(&mut self) -> Result<()> {
        // 检测热插拔
        self.detect_hot_plug();

        if self.connected {
            // 读取DPCD
            let _ = self.read_dpcd();

            // 链路训练
            let _ = self.link_train();
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        self.connected = false;
        self.dpcd = None;
        self.current_link_rate = None;
        self.current_lane_count = None;
        self.training_state = TrainingState::Disabled;
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized && self.connected && self.is_link_trained()
    }

    fn status(&self) -> &'static str {
        if !self.initialized {
            "DP not initialized"
        } else if !self.connected {
            "DP no display connected"
        } else if self.is_link_trained() {
            if let Some(_bw) = self.get_bandwidth_gbps() {
                "DP link trained"
            } else {
                "DP link trained"
            }
        } else {
            "DP link training failed"
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_rate_bandwidth() {
        assert_eq!(LinkRate::Rbr.bandwidth_gbps(), 162);
        assert_eq!(LinkRate::Hbr.bandwidth_gbps(), 270);
        assert_eq!(LinkRate::Hbr2.bandwidth_gbps(), 540);
        assert_eq!(LinkRate::Hbr3.bandwidth_gbps(), 810);
    }

    #[test]
    fn test_link_rate_from_u8() {
        assert_eq!(LinkRate::from_u8(0x06), Some(LinkRate::Rbr));
        assert_eq!(LinkRate::from_u8(0x0A), Some(LinkRate::Hbr));
        assert_eq!(LinkRate::from_u8(0x14), Some(LinkRate::Hbr2));
        assert_eq!(LinkRate::from_u8(0x1E), Some(LinkRate::Hbr3));
        assert_eq!(LinkRate::from_u8(0x00), None);
    }

    #[test]
    fn test_lane_count_from_u8() {
        assert_eq!(LaneCount::from_u8(1), Some(LaneCount::One));
        assert_eq!(LaneCount::from_u8(2), Some(LaneCount::Two));
        assert_eq!(LaneCount::from_u8(4), Some(LaneCount::Four));
        assert_eq!(LaneCount::from_u8(3), None);
    }

    #[test]
    fn test_dp_controller_creation() {
        let ctrl = DpController::new(0xFE000000);
        assert_eq!(ctrl.name(), "DisplayPort Controller");
        assert!(!ctrl.is_ready());
        assert!(!ctrl.connected);
        assert_eq!(ctrl.training_state, TrainingState::Disabled);
    }

    #[test]
    fn test_dp_hpd_fallback_returns_true_when_no_iomem() {
        // 无硬件 fallback 模式: detect_hot_plug 必须返回 true (兼容 QEMU/Bochs)。
        let mut ctrl = DpController::new(0xFE000000);
        assert!(ctrl.detect_hot_plug(), "无 IoMem 时 fallback 必须返回 true");
        assert!(ctrl.connected);
    }

    #[test]
    fn test_dpcd_parse() {
        let data = [
            0x12, // rev 1.2
            0x14, // HBR2 (5.4 Gbps)
            0x84, // 4 lanes, enhanced frame
            0x01, // downspread
            0x00, 0x01, // 1 sink
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        let dpcd = Dpcd::parse(&data).unwrap();
        assert_eq!(dpcd.revision, 0x12);
        assert_eq!(dpcd.max_link_rate, LinkRate::Hbr2);
        assert_eq!(dpcd.max_lane_count, LaneCount::Four);
        assert!(dpcd.max_downspread);
        assert!(dpcd.enhanced_frame_capable);
    }
}
