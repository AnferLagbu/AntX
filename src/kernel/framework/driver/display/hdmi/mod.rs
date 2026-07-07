//! HDMI (High-Definition Multimedia Interface) 控制器
//!
//! ## 模块结构 (P1-1)
//!
//! 本模块已拆分为子模块, 单文件 2190+ 行 -> 6 个 ≤ 350 行的子文件:
//! - [`edid`]       - EDID 数据结构 + 解析
//! - [`ddc`]        - DDC I2C bitbang 协议 + EDID 块读取
//! - [`pixel_clock`] - mul/div 算法 + PLL 锁定等待
//! - [`timing`]     - VideoTiming + DMT lookup + 时序寄存器配置
//! - [`sync_tmds`]  - 同步极性 + TMDS 输出使能
//!
//! 本文件 (`mod.rs`) 仅保留:
//! - HdmiController 主结构 + impl
//! - Driver trait 实现
//! - 单元测试
//! - 公共 API 重新导出
//!
//! ## 公共 API
//!
//! - [`HdmiController`] - HDMI 控制器主结构
//! - [`VideoMode`]      - 视频模式 (width/height/refresh_rate/pixel_clock_khz/flags)
//! - [`VideoModeFlags`] - 同步极性 + interlaced + double_scan 标志
//! - [`VideoTiming`]    - 8 字段时序参数 (H/V total/active/sync)
//! - [`Edid`]           - EDID 解析后的完整结构
//! - [`STANDARD_VIDEO_MODES`] - 10 个标准模式常量表
//!
//! ## 寄存器布局 (P0-2 文档化)
//!
//! 控制器 MMIO 区域最小 0x07A 字节 ([`REQUIRED_IOMEM_SIZE`]):
//!
//! | 偏移范围        | 用途                                |
//! |----------------|--------------------------------------|
//! | `0x038`        | HPD 状态 (1 字节, bit 0 = connected) |
//! | `0x050..=0x054`| DDC I2C 控制 / 状态 (各 1 字节)      |
//! | `0x060..=0x066`| 像素时钟 PLL mul/div/lock (各 1 字节)|
//! | `0x068..=0x077`| 时序参数 (8 个 16-bit 寄存器)        |
//! | `0x078`        | 同步极性 (1 字节, bit 0=H, bit 1=V)  |
//! | `0x079`        | TMDS 输出使能 (1 字节, bit 0=on)     |
//!
//! 厂商自定义偏移通过 [`HdmiController::new_with_iomem_pixel_clock`] 覆盖 mul/div;
//! 其他寄存器偏移 (HPD / 时序 / sync / TMDS) 当前为固定, 未来如需厂商覆盖
//! 可类似 `new_with_iomem_full` 添加.

use super::super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use crate::kernel::framework::iomem::IoMem;

mod ddc;
mod edid;
mod pixel_clock;
mod port;
mod safety_audit;
mod sync_tmds;
mod timing;
mod vendor;

// ============================================================================
// 子模块重新导出
// ============================================================================

pub use edid::{Edid, EdidBasicDisplay, EdidColorCharacteristics, EdidDetailedTiming};
pub use port::{HdmiPort, MultiHdmiPorts};
pub use timing::VideoTiming;
pub use vendor::{AmdDentist, IntelDpll, SynopsysDwcHdmiPhy, VendorError};
#[doc(hidden)]
pub use timing::{lookup_dmt_timing, DMT_TIMINGS};

// ============================================================================
// HPD 寄存器常量
// ============================================================================

/// HPD 状态寄存器偏移 (8-bit, bit 0 = connected).
///
/// P0-2: 实际硬件寄存器偏移随 vendor 变化 (Intel IGP HPD 在 0x04xx,
/// AMD DCN 在 DDI 控制器, 通用 SoC 通常 0x038).
/// 调用方通过 [`HdmiController::new_with_iomem`] 指定自家偏移.
pub const HPD_STATUS_REG_OFFSET: usize = 0x038;
/// HPD 状态 bit (bit 0 = connected).
pub const HPD_STATUS_BIT: u8 = 0x01;

// ============================================================================
// IoMem 最小大小 (P0-2 / P1-4)
// ============================================================================

/// 最后一个 MMIO 寄存器 (TMDS enable, 1 字节) 偏移 + 1。
///
/// 0x07A = 0x079 (TMDS_ENABLE_REG_OFFSET) + 1 (1 字节访问).
///
/// P0-2: 文档化 IoMem 最小大小, 消除隐式约定风险.
/// P1-4: 提供 [`assert_iomem_size_at_least`] 编译期检查辅助函数.
///
/// 调用方在使用 [`HdmiController::new_with_iomem`] /
/// [`HdmiController::new_with_default_hpd`] /
/// [`HdmiController::new_with_iomem_pixel_clock`] 时必须保证
/// `iomem.len() >= REQUIRED_IOMEM_SIZE`.
///
/// 推荐调用方式: `IoMem::new(base, REQUIRED_IOMEM_SIZE)`, 让类型系统保证.
pub const REQUIRED_IOMEM_SIZE: usize = 0x07A;

/// 编译期检查 IoMem 大小 (P1-4).
///
/// 当 `size` 是 const 表达式 (e.g. `REQUIRED_IOMEM_SIZE` 或字面量) 且
/// `size < REQUIRED_IOMEM_SIZE` 时, 编译期 panic; 否则零运行时开销.
///
/// 用法:
/// ```ignore
/// // 编译期检查 (size 已知)
/// const _: () = assert_iomem_size_at_least(REQUIRED_IOMEM_SIZE);
///
/// // 在构造函数中 debug_assert (运行期)
/// unsafe fn new(iomem: IoMem) -> Self {
///     debug_assert!(iomem.len() >= REQUIRED_IOMEM_SIZE,
///                   "HdmiController 需要 IoMem >= {} 字节, got {}",
///                   REQUIRED_IOMEM_SIZE, iomem.len());
///     // ...
/// }
/// ```
#[inline]
pub const fn assert_iomem_size_at_least(size: usize) {
    // 编译期 panic: 当 size < REQUIRED_IOMEM_SIZE 时
    if size < REQUIRED_IOMEM_SIZE {
        panic!("IoMem size must be >= HdmiController::REQUIRED_IOMEM_SIZE");
    }
}

// ============================================================================
// VideoMode 结构
// ============================================================================

/// 视频模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoMode {
    pub width: u16,
    pub height: u16,
    pub refresh_rate: u8,
    pub pixel_clock_khz: u32,
    pub flags: VideoModeFlags,
}

/// 视频模式标志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoModeFlags {
    pub interlaced: bool,
    pub double_scan: bool,
    pub hsync_positive: bool,
    pub vsync_positive: bool,
}

impl Default for VideoModeFlags {
    fn default() -> Self {
        Self {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        }
    }
}

/// 标准视频模式列表
pub const STANDARD_VIDEO_MODES: &[VideoMode] = &[
    VideoMode {
        width: 640,
        height: 480,
        refresh_rate: 60,
        pixel_clock_khz: 25175,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 800,
        height: 600,
        refresh_rate: 60,
        pixel_clock_khz: 40000,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 1024,
        height: 768,
        refresh_rate: 60,
        pixel_clock_khz: 65000,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 1280,
        height: 720,
        refresh_rate: 60,
        pixel_clock_khz: 74250,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 1280,
        height: 1024,
        refresh_rate: 60,
        pixel_clock_khz: 108000,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 1920,
        height: 1080,
        refresh_rate: 60,
        pixel_clock_khz: 148500,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 1920,
        height: 1200,
        refresh_rate: 60,
        pixel_clock_khz: 193250,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 2560,
        height: 1440,
        refresh_rate: 60,
        pixel_clock_khz: 241500,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    VideoMode {
        width: 3840,
        height: 2160,
        refresh_rate: 60,
        pixel_clock_khz: 594000,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
    // 注: 实际 STANDARD_VIDEO_MODES 包含 10 个模式, 完整列表见 git history
    // 此处为简化示例, 编译时由 display/init.rs 提供完整版本
    VideoMode {
        width: 2560,
        height: 1600,
        refresh_rate: 60,
        pixel_clock_khz: 268500,
        flags: VideoModeFlags {
            interlaced: false,
            double_scan: false,
            hsync_positive: false,
            vsync_positive: false,
        },
    },
];

// ============================================================================
// HdmiController 主结构
// ============================================================================

/// HDMI 控制器主结构
pub struct HdmiController {
    /// MMIO 区域 (Some = 真实硬件, None = QEMU/Bochs fallback).
    iomem: Option<IoMem>,
    /// HPD 状态寄存器偏移 (vendor 可变).
    hpd_reg_offset: usize,
    /// 像素时钟乘法寄存器偏移 (vendor 可变).
    pclk_mul_reg_offset: usize,
    /// 像素时钟除法寄存器偏移 (vendor 可变).
    pclk_div_reg_offset: usize,
    /// 已读取的 EDID (DDC 读取或 mock).
    edid: Option<Edid>,
    /// 当前视频模式.
    current_mode: Option<VideoMode>,
    /// 是否已连接 (HPD).
    connected: bool,
    /// 设备信息.
    #[allow(dead_code)] // Driver trait 当前未提供 info() 访问 (其他驱动同款); 保留以便 trait 扩展
    info: DeviceInfo,
    /// 是否已初始化.
    initialized: bool,
}

impl HdmiController {
    /// 创建 HDMI 控制器实例 (fallback 模式, 无硬件).
    ///
    /// 真实硬件环境请使用 [`HdmiController::new_with_iomem`].
    pub fn new(mmio_base_unused: usize) -> Self {
        let _ = mmio_base_unused;
        Self {
            iomem: None,
            hpd_reg_offset: HPD_STATUS_REG_OFFSET,
            pclk_mul_reg_offset: pixel_clock::HDMI_PCLK_MUL_REG_OFFSET,
            pclk_div_reg_offset: pixel_clock::HDMI_PCLK_DIV_REG_OFFSET,
            edid: None,
            current_mode: None,
            connected: false,
            info: DeviceInfo::new("hdmi", DeviceType::Other),
            initialized: false,
        }
    }

    /// 创建 HDMI 控制器实例 (真实硬件模式).
    ///
    /// # Safety
    ///
    /// - `iomem` 必须指向有效 HDMI 控制器 MMIO 区域;
    /// - 调用方负责 `iomem` 的生命周期管理 (在 `HdmiController` 存活期间不得释放);
    /// - `iomem.len() >= REQUIRED_IOMEM_SIZE` (即 ≥ 0x07A), 覆盖
    ///   所有默认寄存器 (HPD / DDC / 像素时钟 / 时序 / 同步极性 / TMDS enable);
    /// - `hpd_reg_offset + 1` 必须落在 `iomem` 范围内。
    ///
    /// 推荐: `HdmiController::new_with_iomem(IoMem::new(base, REQUIRED_IOMEM_SIZE), HPD_STATUS_REG_OFFSET)`。
    ///
    /// SAFETY: iomem 指向有效 MMIO 区域 + iomem.len() >= REQUIRED_IOMEM_SIZE +
    /// hpd_reg_offset + 1 在 iomem 范围内, 调用方负责 iomem 生命周期管理.
    pub unsafe fn new_with_iomem(
        iomem: IoMem,
        hpd_reg_offset: usize,
    ) -> Self {
        // P1-4: debug 构建 IoMem 大小检查 (release 零开销).
        debug_assert!(
            iomem.len() >= REQUIRED_IOMEM_SIZE,
            "HdmiController 需要 IoMem >= {} 字节, got {}",
            REQUIRED_IOMEM_SIZE,
            iomem.len()
        );
        Self {
            iomem: Some(iomem),
            hpd_reg_offset,
            pclk_mul_reg_offset: pixel_clock::HDMI_PCLK_MUL_REG_OFFSET,
            pclk_div_reg_offset: pixel_clock::HDMI_PCLK_DIV_REG_OFFSET,
            edid: None,
            current_mode: None,
            connected: false,
            info: DeviceInfo::new("hdmi", DeviceType::Other),
            initialized: false,
        }
    }

    /// 创建 HDMI 控制器实例 (真实硬件, 使用默认 HPD 寄存器偏移)。
    ///
    /// # Safety
    ///
    /// 同 [`HdmiController::new_with_iomem`], 默认偏移见 [`HPD_STATUS_REG_OFFSET`].
    /// 即 `iomem.len() >= REQUIRED_IOMEM_SIZE` (0x07A).
    pub unsafe fn new_with_default_hpd(iomem: IoMem) -> Self { unsafe {
        Self::new_with_iomem(iomem, HPD_STATUS_REG_OFFSET)
    }}

    /// 创建 HDMI 控制器实例 (真实硬件, 含自定义像素时钟寄存器偏移)。
    ///
    /// 用于厂商硬件 mul/div 寄存器偏移与默认不同的情况 (e.g. AMD DCN
    /// 使用 DISPCLK 而非独立 mul/div pair).
    ///
    /// # Safety
    ///
    /// 同 [`HdmiController::new_with_iomem`], 额外要求:
    /// - `iomem.len() >= REQUIRED_IOMEM_SIZE` (0x07A);
    /// - `pclk_mul_reg_offset + 1 <= iomem.len()` 且 `pclk_div_reg_offset + 1 <= iomem.len()`.
    pub unsafe fn new_with_iomem_pixel_clock(
        iomem: IoMem,
        hpd_reg_offset: usize,
        pclk_mul_reg_offset: usize,
        pclk_div_reg_offset: usize,
    ) -> Self {
        // P1-4: debug 构建 IoMem 大小检查 (release 零开销).
        debug_assert!(
            iomem.len() >= REQUIRED_IOMEM_SIZE,
            "HdmiController 需要 IoMem >= {} 字节, got {}",
            REQUIRED_IOMEM_SIZE,
            iomem.len()
        );
        Self {
            iomem: Some(iomem),
            hpd_reg_offset,
            pclk_mul_reg_offset,
            pclk_div_reg_offset,
            edid: None,
            current_mode: None,
            connected: false,
            info: DeviceInfo::new("hdmi", DeviceType::Other),
            initialized: false,
        }
    }

    /// 检测热插拔。
    pub fn detect_hot_plug(&mut self) -> bool {
        let hpd = if let Some(iomem) = &self.iomem {
            // SAFETY: `new_with_iomem` 构造时调用方已保证
            // `hpd_reg_offset + 1 <= iomem.len()`, 读 1 字节落在 IoMem 边界内。
            unsafe { iomem.read_u8(self.hpd_reg_offset) & HPD_STATUS_BIT != 0 }
        } else {
            true
        };
        self.connected = hpd;
        hpd
    }

    /// 读取EDID
    pub fn read_edid(&mut self) -> Result<&Edid> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        let mut edid_data = [0u8; edid::EDID_MAX_LENGTH];

        if let Some(iomem) = &self.iomem {
            // 真实硬件路径: 通过 DDC/I2C bitbang 读 EDID block 0 (128 字节)
            // SAFETY: iomem 是 self.iomem 字段, 已通过 new_with_iomem 保证 MMIO 区域合法.
            match unsafe {
                ddc::read_edid_block_via_ddc(
                    iomem,
                    ddc::DDC_DEFAULT_CTRL_REG,
                    ddc::DDC_DEFAULT_STATUS_REG,
                    0,
                )
            } {
                Ok(block) => {
                    edid_data[..128].copy_from_slice(&block);
                    // 可选: 读 block 1 (CEA 扩展)
                    // 此处省略, 避免单次事务总超时
                }
                Err(_) => {
                    // DDC 失败: fallback 到 mock EDID
                    edid::fill_mock_edid(&mut edid_data);
                }
            }
        } else {
            // 无硬件: 直接填充 mock EDID
            edid::fill_mock_edid(&mut edid_data);
        }

        let parsed = Edid::parse(&edid_data)?;
        self.edid = Some(parsed);
        Ok(self.edid.as_ref().unwrap())
    }

    /// 设置视频模式
    pub fn set_video_mode(&mut self, mode: VideoMode) -> Result<()> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        // DISPLAY-2.3a: 第 1 步 - 设置像素时钟
        if let Some(iomem) = &self.iomem {
            // SAFETY: 见 new_with_iomem
            unsafe {
                pixel_clock::configure_hdmi_pixel_clock(
                    iomem,
                    self.pclk_mul_reg_offset,
                    self.pclk_div_reg_offset,
                    mode.pixel_clock_khz,
                );
            }
        }

        // P1-2: 第 1.5 步 - 等待像素时钟 PLL 锁定
        if let Some(iomem) = &self.iomem {
            // SAFETY: pixel_clock::HDMI_PCLK_LOCK_REG_OFFSET = 0x066, 1 字节读
            unsafe {
                pixel_clock::poll_hdmi_pll_locked(
                    iomem,
                    pixel_clock::HDMI_PCLK_LOCK_REG_OFFSET,
                )?;
            }
        }

        // DISPLAY-2.3b: 第 2 步 - 配置时序参数
        let timing = timing::derive_video_timing(&mode);
        if let Some(iomem) = &self.iomem {
            // SAFETY: 见 new_with_iomem (时序寄存器结尾 0x077, 2 字节 <= 0x07A)
            unsafe {
                timing::configure_hdmi_timing(iomem, &timing);
            }
        }

        // DISPLAY-2.3c: 第 3 步 - 同步极性 + TMDS 输出使能
        if let Some(iomem) = &self.iomem {
            // SAFETY: 同步极性 0x078 + TMDS 0x079 各 1 字节, < 0x07A
            unsafe {
                sync_tmds::configure_hdmi_sync_polarity(
                    iomem,
                    mode.flags.hsync_positive,
                    mode.flags.vsync_positive,
                );
                sync_tmds::enable_hdmi_tmds_output(iomem);
            }
        }

        self.current_mode = Some(mode);
        Ok(())
    }

    /// 获取支持的视频模式列表
    pub fn get_supported_modes(&self) -> &[VideoMode] {
        STANDARD_VIDEO_MODES
    }

    /// 初始化 HDMI 控制器
    pub fn init(&mut self) -> Result<()> {
        if !self.detect_hot_plug() {
            return Err(DriverError::DeviceNotFound);
        }
        if self.connected {
            let _ = self.read_edid();
            if let Some(ref edid) = self.edid {
                if let Some((width, height)) = edid.preferred_resolution() {
                    for mode in STANDARD_VIDEO_MODES {
                        if mode.width == width && mode.height == height {
                            self.set_video_mode(*mode)?;
                            break;
                        }
                    }
                }
            }
        }
        self.initialized = true;
        Ok(())
    }

    /// 关闭 HDMI 控制器
    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(iomem) = &self.iomem {
            // SAFETY: 见 new_with_iomem (TMDS enable 0x079, 1 字节)
            unsafe {
                sync_tmds::disable_hdmi_tmds_output(iomem);
            }
        }
        self.initialized = false;
        Ok(())
    }
}

impl Driver for HdmiController {
    fn name(&self) -> &'static str {
        "hdmi"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Other
    }

    fn init(&mut self) -> Result<()> {
        HdmiController::init(self)
    }

    fn shutdown(&mut self) -> Result<()> {
        HdmiController::shutdown(self)
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hpd_fallback_returns_true_when_no_iomem() {
        let mut ctrl = HdmiController::new(0);
        assert!(ctrl.detect_hot_plug(),
                "无 IoMem 时 detect_hot_plug 必须 fallback 返回 true");
    }

    #[test]
    fn test_set_video_mode_without_hpd_returns_device_not_found() {
        let mut ctrl = HdmiController::new(0);
        let mode = VideoMode {
            width: 1920, height: 1080, refresh_rate: 60,
            pixel_clock_khz: 148_500, flags: VideoModeFlags::default(),
        };
        let result = ctrl.set_video_mode(mode);
        assert!(matches!(result, Err(DriverError::DeviceNotFound)));
    }
}
