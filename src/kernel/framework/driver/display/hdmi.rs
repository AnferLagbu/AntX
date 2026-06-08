//! HDMI 驱动 (HDMI Driver)
//!
//! 提供HDMI显示接口支持：
//! - **EDID读取**: 自动检测显示器信息
//! - **视频模式**: 分辨率和刷新率配置
//! - **音频支持**: HDMI音频传输
//! - **热插拔**: 显示器动态连接检测
//!
//! ## 硬件接口
//!
//! ```text
//! HDMI Controller:
//! ├── I2C/DDC: EDID读取 (地址 0xA0)
//! ├── HPD: 热插拔检测
//! ├── TMDS: 视频数据传输
//! └── Audio: 音频数据包
//! ```
//!
//! # Safety
//! HDMI驱动涉及MMIO寄存器和I2C通信。

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use alloc::vec::Vec;

// ============================================================================
// HDMI 常量定义
// ============================================================================

/// EDID I2C地址
const EDID_I2C_ADDR: u8 = 0xA0;

/// EDID最大长度
const EDID_MAX_LENGTH: usize = 256;

/// 标准EDID头
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

// ============================================================================
// EDID 结构定义
// ============================================================================

/// EDID 基本显示参数
#[derive(Debug, Clone, Copy)]
pub struct EdidBasicDisplay {
    pub video_input_type: u8,
    pub horizontal_image_size: u8,
    pub vertical_image_size: u8,
    pub display_transfer_characteristic: u8,
    pub feature_support: u8,
}

/// EDID 颜色特性
#[derive(Debug, Clone, Copy)]
pub struct EdidColorCharacteristics {
    pub red_green_low: u8,
    pub blue_white_low: u8,
    pub red_x: u8,
    pub red_y: u8,
    pub green_x: u8,
    pub green_y: u8,
    pub blue_x: u8,
    pub blue_y: u8,
    pub white_x: u8,
    pub white_y: u8,
}

/// EDID 详细时序描述符
#[derive(Debug, Clone, Copy)]
pub struct EdidDetailedTiming {
    pub pixel_clock: u16,
    pub horizontal_active: u8,
    pub horizontal_blanking: u8,
    pub horizontal_active_high: u8,
    pub horizontal_blanking_high: u8,
    pub vertical_active: u8,
    pub vertical_blanking: u8,
    pub vertical_active_blanking_high: u8,
    pub horizontal_sync_offset: u8,
    pub horizontal_sync_pulse_width: u8,
    pub vertical_sync_offset_width: u8,
    pub sync_type: u8,
    pub image_size: u8,
    pub border: u8,
    pub features: u8,
}

impl EdidDetailedTiming {
    /// 获取水平分辨率
    pub fn horizontal_resolution(&self) -> u16 {
        (self.horizontal_active as u16) | ((self.horizontal_active_high as u16 & 0xF0) << 4)
    }

    /// 获取垂直分辨率
    pub fn vertical_resolution(&self) -> u16 {
        (self.vertical_active as u16) | ((self.vertical_active_blanking_high as u16 & 0xF0) << 4)
    }

    /// 获取刷新率 (近似)
    pub fn refresh_rate(&self) -> u32 {
        if self.pixel_clock == 0 {
            return 60;
        }

        let h_total = self.horizontal_resolution() as u32
            + ((self.horizontal_blanking as u32)
                | ((self.horizontal_blanking_high as u32 & 0x0F) << 8));
        let v_total = self.vertical_resolution() as u32
            + ((self.vertical_blanking as u32)
                | ((self.vertical_active_blanking_high as u32 & 0x0F) << 8));

        if h_total == 0 || v_total == 0 {
            return 60;
        }

        let pixel_clock_khz = self.pixel_clock as u32 * 10;
        pixel_clock_khz * 1000 / (h_total * v_total)
    }
}

/// 完整EDID数据结构
#[derive(Debug, Clone)]
pub struct Edid {
    /// 原始数据
    pub raw: [u8; EDID_MAX_LENGTH],
    /// 厂商名称
    pub manufacturer: [u8; 4],
    /// 产品代码
    pub product_code: u16,
    /// 序列号
    pub serial_number: u32,
    /// 制造周
    pub week: u8,
    /// 制造年
    pub year: u16,
    /// EDID版本
    pub version: u8,
    /// EDID修订
    pub revision: u8,
    /// 基本显示参数
    pub basic_display: EdidBasicDisplay,
    /// 颜色特性
    pub color_characteristics: EdidColorCharacteristics,
    /// 支持的视频模式
    pub supported_modes: Vec<VideoMode>,
    /// 详细时序描述符
    pub detailed_timings: [Option<EdidDetailedTiming>; 4],
}

impl Edid {
    /// 从原始数据解析EDID
    pub fn parse(data: &[u8; EDID_MAX_LENGTH]) -> Result<Self> {
        // 验证EDID头
        if data[0..8] != EDID_HEADER {
            return Err(DriverError::InvalidParameter);
        }

        // 验证校验和
        let checksum: u8 = data[0..128].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        if checksum != 0 {
            return Err(DriverError::InvalidParameter);
        }

        // 解析厂商名称
        let mut manufacturer = [0u8; 4];
        let man_id = ((data[8] as u16) << 8) | (data[9] as u16);
        manufacturer[0] = b'@' + ((man_id >> 10) & 0x1F) as u8;
        manufacturer[1] = b'@' + ((man_id >> 5) & 0x1F) as u8;
        manufacturer[2] = b'@' + (man_id & 0x1F) as u8;
        manufacturer[3] = 0;

        // 解析详细时序描述符
        let mut detailed_timings: [Option<EdidDetailedTiming>; 4] = [None, None, None, None];

        for i in 0..4 {
            let offset = 54 + i * 18;
            if data[offset] != 0 || data[offset + 1] != 0 {
                detailed_timings[i] = Some(EdidDetailedTiming {
                    pixel_clock: (data[offset] as u16) | ((data[offset + 1] as u16) << 8),
                    horizontal_active: data[offset + 2],
                    horizontal_blanking: data[offset + 3],
                    horizontal_active_high: data[offset + 4],
                    horizontal_blanking_high: data[offset + 5],
                    vertical_active: data[offset + 6],
                    vertical_blanking: data[offset + 7],
                    vertical_active_blanking_high: data[offset + 8],
                    horizontal_sync_offset: data[offset + 9],
                    horizontal_sync_pulse_width: data[offset + 10],
                    vertical_sync_offset_width: data[offset + 11],
                    sync_type: data[offset + 12],
                    image_size: data[offset + 13],
                    border: data[offset + 14],
                    features: data[offset + 15],
                });
            }
        }

        Ok(Self {
            raw: *data,
            manufacturer,
            product_code: (data[10] as u16) | ((data[11] as u16) << 8),
            serial_number: (data[12] as u32)
                | ((data[13] as u32) << 8)
                | ((data[14] as u32) << 16)
                | ((data[15] as u32) << 24),
            week: data[16],
            year: data[17] as u16 + 1990,
            version: data[18],
            revision: data[19],
            basic_display: EdidBasicDisplay {
                video_input_type: data[20],
                horizontal_image_size: data[21],
                vertical_image_size: data[22],
                display_transfer_characteristic: data[23],
                feature_support: data[24],
            },
            color_characteristics: EdidColorCharacteristics {
                red_green_low: data[25],
                blue_white_low: data[26],
                red_x: data[27],
                red_y: data[28],
                green_x: data[29],
                green_y: data[30],
                blue_x: data[31],
                blue_y: data[32],
                white_x: data[33],
                white_y: data[34],
            },
            supported_modes: Vec::new(),
            detailed_timings,
        })
    }

    /// 获取首选分辨率
    pub fn preferred_resolution(&self) -> Option<(u16, u16)> {
        for timing in &self.detailed_timings {
            if let Some(t) = timing {
                return Some((t.horizontal_resolution(), t.vertical_resolution()));
            }
        }
        None
    }
}

// ============================================================================
// 视频模式
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
        refresh_rate: 30,
        pixel_clock_khz: 297000,
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
];

// ============================================================================
// HDMI 控制器
// ============================================================================

/// HDMI 控制器驱动
pub struct HdmiController {
    /// MMIO基地址
    mmio_base: usize,
    /// EDID数据
    edid: Option<Edid>,
    /// 当前视频模式
    current_mode: Option<VideoMode>,
    /// 是否连接显示器
    connected: bool,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl HdmiController {
    /// 创建新的HDMI控制器实例
    pub fn new(mmio_base: usize) -> Self {
        Self {
            mmio_base,
            edid: None,
            current_mode: None,
            connected: false,
            info: DeviceInfo::new("hdmi", DeviceType::Other),
            initialized: false,
        }
    }

    /// 检测热插拔
    pub fn detect_hot_plug(&mut self) -> bool {
        // TODO(TRACK-CD5DA5): 读取HPD引脚状态
        // 这里简化实现，假设已连接
        self.connected = true;
        self.connected
    }

    /// 读取EDID
    pub fn read_edid(&mut self) -> Result<&Edid> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        let mut edid_data = [0u8; EDID_MAX_LENGTH];

        // TODO(TRACK-7CCB60): 通过I2C/DDC读取EDID
        // 这里使用模拟数据
        edid_data[0..8].copy_from_slice(&EDID_HEADER);

        // 设置厂商ID (示例: "ANTX")
        edid_data[8] = 0x04; // 'A'
        edid_data[9] = 0x5D; // 'NTX' packed

        // 设置EDID版本
        edid_data[18] = 1; // version 1.3
        edid_data[19] = 3;

        // 设置基本显示参数
        edid_data[20] = 0x80; // 数字输入
        edid_data[21] = 53; // 水平尺寸 (cm)
        edid_data[22] = 30; // 垂直尺寸 (cm)

        // 设置详细时序 (1920x1080 @ 60Hz)
        let timing_offset = 54;
        edid_data[timing_offset] = 0x69; // pixel clock low
        edid_data[timing_offset + 1] = 0x03; // pixel clock high (148.5 MHz)
        edid_data[timing_offset + 2] = 0x80; // horizontal active
        edid_data[timing_offset + 3] = 0x98; // horizontal blanking
        edid_data[timing_offset + 4] = 0x31; // horizontal active/blanking high
        edid_data[timing_offset + 5] = 0x02; // horizontal blanking high
        edid_data[timing_offset + 6] = 0x38; // vertical active
        edid_data[timing_offset + 7] = 0x1D; // vertical blanking

        // 计算校验和
        let mut checksum = 0u8;
        for i in 0..127 {
            checksum = checksum.wrapping_add(edid_data[i]);
        }
        edid_data[127] = (256 - checksum as usize) as u8;

        let edid = Edid::parse(&edid_data)?;
        self.edid = Some(edid);

        Ok(self.edid.as_ref().unwrap())
    }

    /// 设置视频模式
    pub fn set_video_mode(&mut self, mode: VideoMode) -> Result<()> {
        if !self.connected {
            return Err(DriverError::DeviceNotFound);
        }

        // TODO(TRACK-1BDEF6): 配置HDMI控制器寄存器
        // 1. 设置像素时钟
        // 2. 配置时序参数
        // 3. 设置同步信号极性

        self.current_mode = Some(mode);
        Ok(())
    }

    /// 获取支持的视频模式列表
    pub fn get_supported_modes(&self) -> Vec<VideoMode> {
        if let Some(ref edid) = self.edid {
            let mut modes = Vec::new();

            // 从详细时序描述符提取模式
            for timing in &edid.detailed_timings {
                if let Some(t) = timing {
                    modes.push(VideoMode {
                        width: t.horizontal_resolution(),
                        height: t.vertical_resolution(),
                        refresh_rate: t.refresh_rate() as u8,
                        pixel_clock_khz: t.pixel_clock as u32 * 10,
                        flags: VideoModeFlags::default(),
                    });
                }
            }

            modes
        } else {
            STANDARD_VIDEO_MODES.to_vec()
        }
    }

    /// 获取当前视频模式
    pub fn get_current_mode(&self) -> Option<&VideoMode> {
        self.current_mode.as_ref()
    }

    /// 获取EDID
    pub fn get_edid(&self) -> Option<&Edid> {
        self.edid.as_ref()
    }

    /// 检查是否连接显示器
    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for HdmiController {
    fn name(&self) -> &'static str {
        "HDMI Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Other
    }

    fn init(&mut self) -> Result<()> {
        // 检测热插拔
        self.detect_hot_plug();

        if self.connected {
            // 读取EDID
            let _ = self.read_edid();

            // 设置首选视频模式
            if let Some(ref edid) = self.edid {
                if let Some((width, height)) = edid.preferred_resolution() {
                    // 查找匹配的视频模式
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

    fn shutdown(&mut self) -> Result<()> {
        self.connected = false;
        self.edid = None;
        self.current_mode = None;
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized && self.connected
    }

    fn status(&self) -> &'static str {
        if !self.initialized {
            "HDMI not initialized"
        } else if !self.connected {
            "HDMI no display connected"
        } else if let Some(ref _mode) = self.current_mode {
            "HDMI connected and active"
        } else {
            "HDMI connected"
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
    fn test_edid_header() {
        assert_eq!(
            EDID_HEADER,
            [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]
        );
    }

    #[test]
    fn test_standard_video_modes() {
        assert!(!STANDARD_VIDEO_MODES.is_empty());

        // 检查1920x1080 @ 60Hz
        let mode = &STANDARD_VIDEO_MODES[6];
        assert_eq!(mode.width, 1920);
        assert_eq!(mode.height, 1080);
        assert_eq!(mode.refresh_rate, 60);
    }

    #[test]
    fn test_hdmi_controller_creation() {
        let ctrl = HdmiController::new(0xFE000000);
        assert_eq!(ctrl.name(), "HDMI Controller");
        assert!(!ctrl.is_ready());
        assert!(!ctrl.is_connected());
    }

    #[test]
    fn test_video_mode_flags_default() {
        let flags = VideoModeFlags::default();
        assert!(!flags.interlaced);
        assert!(!flags.double_scan);
        assert!(!flags.hsync_positive);
        assert!(!flags.vsync_positive);
    }
}
