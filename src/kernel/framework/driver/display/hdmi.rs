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
use crate::kernel::framework::iomem::IoMem;
use alloc::vec::Vec;

// ============================================================================
// HDMI 常量定义
// ============================================================================

/// EDID I2C地址
#[allow(dead_code)] // 规范定义, 待 EDID I2C 读取启用后使用。
const EDID_I2C_ADDR: u8 = 0xA0;

/// EDID最大长度
const EDID_MAX_LENGTH: usize = 256;

/// 标准EDID头
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// HDMI HPD (Hot Plug Detect) 状态寄存器偏移。
///
/// 该偏移基于通用 DDC/HPD 规范; 实际硬件 (Intel/AMD/Nvidia) 偏移量差异较大,
/// 应通过 `new_with_iomem` 时由调用方根据自家芯片手册覆盖。
///
/// 参考:
/// - DDC/CI spec: HPD 信号在 DDC 通道脚位 19
/// - Intel IGP: MMIO +0xC8 bit0-3 (端口 A-D HPD)
/// - AMD DCN: MMIO +0x5E bit0
const HPD_STATUS_REG_OFFSET: usize = 0x038;

/// HPD 状态位 (bit 0)
const HPD_STATUS_BIT: u8 = 0x01;

// ============================================================================
// DDC (Display Data Channel) — HDMI I2C 主机
// ============================================================================
//
// DDC 是简化版 I2C 协议, 用于读取显示器 EDID. 通过 HDMI 控制器 MMIO 寄存器的
// bitbang 模式实现 (SDA/SCL 走控制器内的 GPIO-like 寄存器).
//
// 厂商偏移参考:
// - Intel IGP GMBus: 16-bit 端口 I/O, 走专有 GMBus 控制器 (非 bitbang, 不适用本实现)
// - AMD DCN: DDC bitbang 寄存器通常在 DDI 控制器 MMIO 区
// - 通用 SoC HDMI (Synopsys/DesignWare/IT66121): 8-bit bitbang 寄存器
// - QEMU Bochs DISPI: 无 DDC, fallback 到 mock EDID
//
// 本实装采用通用 8-bit bitbang 路径, 适用于大多数 SoC HDMI 控制器.

/// DDC 控制寄存器默认偏移 (8-bit 写: bit0=SDA_out, bit1=SCL_out)。
/// 1 = 高电平 (开漏, 实际由上拉电阻拉高), 0 = 主机驱动低电平。
const DDC_DEFAULT_CTRL_REG: usize = 0x050;

/// DDC 状态寄存器默认偏移 (8-bit 读: bit0=SDA_in, bit1=SCL_in)。
const DDC_DEFAULT_STATUS_REG: usize = 0x054;

/// SDA 输出位 (bitbang 控制寄存器)
const DDC_SDA_OUT_BIT: u8 = 0x01;
/// SCL 输出位 (bitbang 控制寄存器)
const DDC_SCL_OUT_BIT: u8 = 0x02;
/// SDA 输入位 (bitbang 状态寄存器)
const DDC_SDA_IN_BIT: u8 = 0x01;

/// EDID I2C 从机地址 (写模式, 0xA0 = 0x50 << 1)
const DDC_EDID_ADDR_WRITE: u8 = 0xA0;
/// EDID I2C 从机地址 (读模式, 0xA1 = 0x50 << 1 | 1)
const DDC_EDID_ADDR_READ: u8 = 0xA1;

/// DDC I2C 时序延时针 (spin loop 次数, 适配 ~100 kHz 标准模式)。
///
/// 内核上下文不允许睡眠, 通过 `core::hint::spin_loop` 实现短延时。
/// 50 次 spin_loop 在现代 CPU 上约 1-2 微秒 (接近 I2C 标准模式周期)。
const DDC_I2C_DELAY_ITERS: usize = 50;

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
// DDC I2C bitbang 原语
// ============================================================================

/// I2C 总线短延时 (≈ 1-2 µs, 适配 100 kHz 标准模式)。
///
/// 内核上下文不允许睡眠, 通过 `core::hint::spin_loop` 实现短延时;
/// 现代 CPU 上 50 次 spin_loop 约 1-2 微秒, 接近 I2C 标准模式半周期。
#[inline]
fn ddc_delay() {
    for _ in 0..DDC_I2C_DELAY_ITERS {
        core::hint::spin_loop();
    }
}

/// 同时设置 SDA 与 SCL 输出电平。
///
/// 1 = 高电平 (开漏, 实际由上拉电阻拉高);
/// 0 = 主机驱动低电平。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_set_sda_scl(iomem: &IoMem, ctrl_reg_offset: usize, sda_high: bool, scl_high: bool) {
    let mut val = 0u8;
    if sda_high {
        val |= DDC_SDA_OUT_BIT;
    }
    if scl_high {
        val |= DDC_SCL_OUT_BIT;
    }
    iomem.write_u8(ctrl_reg_offset, val);
}

/// I2C START 条件: SDA 在 SCL 高电平时由高变低。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_i2c_start(iomem: &IoMem, ctrl_reg_offset: usize) {
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay();
    ddc_set_sda_scl(iomem, ctrl_reg_offset, false, true);
    ddc_delay();
}

/// I2C STOP 条件: SDA 在 SCL 高电平时由低变高。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_i2c_stop(iomem: &IoMem, ctrl_reg_offset: usize) {
    ddc_set_sda_scl(iomem, ctrl_reg_offset, false, true);
    ddc_delay();
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay();
}

/// I2C 写 1 字节 (MSB first) 并采样 ACK.
///
/// 返回 `true` 表示从机 ACK (SDA=low), `false` 表示 NACK 或总线错误。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()` 与
/// `status_reg_offset + 1 <= iomem.len()`。
unsafe fn ddc_i2c_write_byte(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    byte: u8,
) -> bool {
    for i in 0..8u8 {
        let bit = (byte >> (7 - i)) & 1 != 0;
        ddc_set_sda_scl(iomem, ctrl_reg_offset, bit, false);
        ddc_delay();
        ddc_set_sda_scl(iomem, ctrl_reg_offset, bit, true);
        ddc_delay();
    }
    // 释放 SDA 让从机 ACK
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    ddc_delay();
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay();
    // 采样 SDA: 0 = ACK, 1 = NACK
    let sda = iomem.read_u8(status_reg_offset) & DDC_SDA_IN_BIT;
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    sda == 0
}

/// I2C 读 1 字节 (MSB first), 由主机发送 ACK/NACK。
///
/// `send_ack = true` 表示读完后主机 ACK (从机继续发送, 用于读 0..126 字节),
/// `send_ack = false` 表示 NACK (从机停止发送, 用于读最后 1 字节)。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()` 与
/// `status_reg_offset + 1 <= iomem.len()`。
unsafe fn ddc_i2c_read_byte(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    send_ack: bool,
) -> u8 {
    let mut byte = 0u8;
    // 释放 SDA 让从机驱动
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    ddc_delay();

    for i in 0..8u8 {
        ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
        ddc_delay();
        let bit = iomem.read_u8(status_reg_offset) & DDC_SDA_IN_BIT;
        byte |= bit << (7 - i);
        ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
        ddc_delay();
    }

    // 主机发送 ACK/NACK
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, false);
    ddc_delay();
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, true);
    ddc_delay();
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, false);

    byte
}

/// 填充 mock EDID 数据 (用于无硬件 / DDC 失败 fallback)。
///
/// 内容为硬编码的 1920x1080 @ 60Hz EDID block 0,
/// 校验和正确, 可被 `Edid::parse` 解析通过。
fn fill_mock_edid(edid_data: &mut [u8; EDID_MAX_LENGTH]) {
    edid_data[0..8].copy_from_slice(&EDID_HEADER);

    // 厂商 ID (示例: "ANTX")
    edid_data[8] = 0x04; // 'A'
    edid_data[9] = 0x5D; // 'NTX' packed

    // EDID 版本
    edid_data[18] = 1;
    edid_data[19] = 3;

    // 基本显示参数
    edid_data[20] = 0x80; // 数字输入
    edid_data[21] = 53;   // 水平尺寸 (cm)
    edid_data[22] = 30;   // 垂直尺寸 (cm)

    // 详细时序 (1920x1080 @ 60Hz)
    let timing_offset = 54;
    edid_data[timing_offset] = 0x69;     // pixel clock low
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
}

/// 通过 DDC 总线读取 EDID 块 (128 字节)。
///
/// I2C 事务序列:
/// ```text
/// START -> [0xA0] -> [offset] -> REPEATED_START -> [0xA1] -> [128 字节] -> STOP
/// ```
///
/// 返回 `Ok([u8; 128])` 表示读取成功 (含 ACK), 失败返回 `Err`。
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `ctrl_reg_offset + 1 <= iomem.len()` 且 `status_reg_offset + 1 <= iomem.len()`
unsafe fn read_edid_block_via_ddc(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    block: u8,
) -> core::result::Result<[u8; 128], DriverError> {
    let mut data = [0u8; 128];

    ddc_i2c_start(iomem, ctrl_reg_offset);
    if !ddc_i2c_write_byte(iomem, ctrl_reg_offset, status_reg_offset, DDC_EDID_ADDR_WRITE) {
        ddc_i2c_stop(iomem, ctrl_reg_offset);
        return Err(DriverError::HardwareError);
    }
    // EDID 块偏移 = block * 128 (block 0 起始于 0, block 1 起始于 128)
    let offset = block.wrapping_mul(128);
    if !ddc_i2c_write_byte(iomem, ctrl_reg_offset, status_reg_offset, offset) {
        ddc_i2c_stop(iomem, ctrl_reg_offset);
        return Err(DriverError::HardwareError);
    }

    // REPEATED START 切换到读模式
    ddc_i2c_start(iomem, ctrl_reg_offset);
    if !ddc_i2c_write_byte(iomem, ctrl_reg_offset, status_reg_offset, DDC_EDID_ADDR_READ) {
        ddc_i2c_stop(iomem, ctrl_reg_offset);
        return Err(DriverError::HardwareError);
    }

    // 读 128 字节: 前 127 字节 ACK, 最后 1 字节 NACK
    for i in 0..127 {
        data[i] = ddc_i2c_read_byte(iomem, ctrl_reg_offset, status_reg_offset, true);
    }
    data[127] = ddc_i2c_read_byte(iomem, ctrl_reg_offset, status_reg_offset, false);

    ddc_i2c_stop(iomem, ctrl_reg_offset);
    Ok(data)
}

// ============================================================================
// HDMI 控制器
// ============================================================================

/// HDMI 控制器驱动
pub struct HdmiController {
    /// MMIO 句柄 (Option, 在无硬件/虚拟化环境为 None)。
    ///
    /// - `Some(iomem)`: 真实硬件路径, 通过 MMIO 寄存器读取 HPD 等状态。
    /// - `None`: 无硬件路径 (QEMU 默认/QEMU Bochs DISPI), HPD 检测走 fallback
    ///   (假设已连接, 由调用方决定是否启用), 仅用于开发环境。
    iomem: Option<IoMem>,
    /// HPD 寄存器偏移 (相对 `iomem` 基地址)。
    /// 不同厂商 HDMI 控制器偏移量不同; 默认使用通用 DDC 偏移, 调用方可通过
    /// `new_with_iomem_offset` 指定自家硬件偏移。
    hpd_reg_offset: usize,
    /// EDID数据
    edid: Option<Edid>,
    /// 当前视频模式
    current_mode: Option<VideoMode>,
    /// 是否连接显示器
    connected: bool,
    /// 设备信息 (待驱动框架 Device trait 集成后使用)。
    #[allow(dead_code)] // 待驱动框架 Device trait 集成后使用。
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl HdmiController {
    /// 创建 HDMI 控制器实例 (无硬件 fallback 模式)。
    ///
    /// 此模式 `detect_hot_plug` 直接返回 `true` (假设已连接),
    /// 用于 QEMU/QEMU+bochs-vbe 等无真实 HDMI 控制器的开发环境。
    /// 真实硬件环境请使用 [`HdmiController::new_with_iomem`].
    pub fn new(mmio_base_unused: usize) -> Self {
        // 参数保留以兼容旧调用方; iomem 路径走 fallback。
        let _ = mmio_base_unused;
        Self {
            iomem: None,
            hpd_reg_offset: HPD_STATUS_REG_OFFSET,
            edid: None,
            current_mode: None,
            connected: false,
            info: DeviceInfo::new("hdmi", DeviceType::Other),
            initialized: false,
        }
    }

    /// 创建 HDMI 控制器实例 (真实硬件模式)。
    ///
    /// # Safety
    ///
    /// - `iomem` 必须指向有效 HDMI 控制器 MMIO 区域;
    /// - 调用方负责 `iomem` 的生命周期管理 (在 `HdmiController` 存活期间不得释放);
    /// - `hpd_reg_offset + 1` 必须落在 `iomem` 范围内。
    pub unsafe fn new_with_iomem(
        iomem: IoMem,
        hpd_reg_offset: usize,
    ) -> Self {
        Self {
            iomem: Some(iomem),
            hpd_reg_offset,
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
    pub unsafe fn new_with_default_hpd(iomem: IoMem) -> Self {
        Self::new_with_iomem(iomem, HPD_STATUS_REG_OFFSET)
    }

    /// 检测热插拔。
    ///
    /// 真实硬件: 从 MMIO 读 `HPD_STATUS_REG_OFFSET` 寄存器, bit 0 == 1 表示已连接。
    /// 无硬件 fallback: 返回 `true` (兼容 QEMU + Bochs DISPI 开发环境)。
    pub fn detect_hot_plug(&mut self) -> bool {
        let hpd = if let Some(iomem) = &self.iomem {
            // SAFETY: `new_with_iomem` 构造时调用方已保证
            // `hpd_reg_offset + 1 <= iomem.len()`, 读 1 字节落在 IoMem 边界内。
            unsafe { iomem.read_u8(self.hpd_reg_offset) & HPD_STATUS_BIT != 0 }
        } else {
            // 无硬件 fallback: 假设已连接 (兼容 QEMU Bochs DISPI 开发环境)。
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

        let mut edid_data = [0u8; EDID_MAX_LENGTH];

        if let Some(iomem) = &self.iomem {
            // 真实硬件路径: 通过 DDC/I2C bitbang 读 EDID block 0 (128 字节)。
            //
            // SAFETY: `new_with_iomem` 构造时调用方已保证 IoMem 边界有效,
            // DDC 寄存器偏移量见 `DDC_DEFAULT_CTRL_REG` + `DDC_DEFAULT_STATUS_REG`,
            // 二者均为 8-bit 访问且 offset + 1 落在 IoMem 范围内 (假设 IoMem >= 0x055).
            unsafe {
                match read_edid_block_via_ddc(iomem, DDC_DEFAULT_CTRL_REG, DDC_DEFAULT_STATUS_REG, 0) {
                    Ok(block0) => {
                        edid_data[..128].copy_from_slice(&block0);
                        // 尝试读 extension block (block 1, CEA-861 等)
                        if block0[126] != 0 {
                            if let Ok(block1) = read_edid_block_via_ddc(
                                iomem,
                                DDC_DEFAULT_CTRL_REG,
                                DDC_DEFAULT_STATUS_REG,
                                1,
                            ) {
                                edid_data[128..256].copy_from_slice(&block1);
                            }
                            // 失败则 extension 段保持 0, 仍能通过 block 0 校验
                        }
                    }
                    Err(_) => {
                        // DDC 读失败 (设备未应答/总线错误), 回落到 mock EDID
                        // 以保证调用方拿到可用 (虽然不准确) 的 EDID 数据.
                        fill_mock_edid(&mut edid_data);
                    }
                }
            }
        } else {
            // 无硬件 fallback: QEMU/Bochs DISPI 无 DDC, 使用 mock EDID.
            fill_mock_edid(&mut edid_data);
        }

        let edid = Edid::parse(&edid_data)?;
        self.edid = Some(edid);

        // SAFETY: 刚在上面设为 Some, 不会为 None
        Ok(self.edid.as_ref().expect("hdmi: edid 刚已设为 Some"))
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
    fn test_hpd_fallback_returns_true_when_no_iomem() {
        // 无硬件 fallback 模式: detect_hot_plug 必须返回 true (兼容 QEMU/Bochs)。
        let mut ctrl = HdmiController::new(0xFE000000);
        assert!(ctrl.detect_hot_plug(), "无 IoMem 时 fallback 必须返回 true");
        assert!(ctrl.is_connected());
    }

    #[test]
    fn test_video_mode_flags_default() {
        let flags = VideoModeFlags::default();
        assert!(!flags.interlaced);
        assert!(!flags.double_scan);
        assert!(!flags.hsync_positive);
        assert!(!flags.vsync_positive);
    }

    #[test]
    fn test_fill_mock_edid_checksum_valid() {
        // fill_mock_edid 必须产生通过 Edid::parse 校验的 EDID (校验和正确)。
        let mut edid_data = [0u8; EDID_MAX_LENGTH];
        fill_mock_edid(&mut edid_data);

        // 校验和检查: 128 字节累加必须 = 0
        let checksum: u8 = edid_data[..128].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(checksum, 0, "mock EDID 校验和必须正确");

        // 必须能解析
        let edid = Edid::parse(&edid_data).expect("mock EDID 必须可解析");
        assert_eq!(edid.raw[0..8], EDID_HEADER);
    }

    #[test]
    fn test_read_edid_fallback_when_no_iomem() {
        // 无硬件 fallback: read_edid 必须返回 mock EDID (可解析).
        let mut ctrl = HdmiController::new(0xFE000000);
        ctrl.detect_hot_plug(); // 触发 fallback connected = true
        let edid = ctrl.read_edid().expect("无硬件 fallback 必须返回 mock EDID");
        assert_eq!(edid.raw[0..8], EDID_HEADER);
    }

    #[test]
    fn test_read_edid_without_hpd_returns_device_not_found() {
        // 未检测 HPD 时 (connected = false) read_edid 应返回 DeviceNotFound.
        let mut ctrl = HdmiController::new(0xFE000000);
        // 不调用 detect_hot_plug, connected 保持 false
        let result = ctrl.read_edid();
        assert!(matches!(result, Err(DriverError::DeviceNotFound)));
    }
}
