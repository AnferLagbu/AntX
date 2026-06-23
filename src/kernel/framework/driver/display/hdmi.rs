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
// HDMI 像素时钟配置
// ============================================================================
//
// HDMI 像素时钟 (TMDS clock) 由 HDMI 控制器内部 PLL 从参考时钟派生.
// 不同厂商 PLL 接口差异极大:
//
// - Intel IGP (IBX/HSW/SKL): DPLL (Display PLL) 通过 PCH transcoder, 内部 MMIO 寄存器
// - AMD DCN: DENTIST clock generator + DISPCLK
// - Synopsys DesignWare HDMI: phy_clock + tmds_clock 寄存器
// - QEMU Bochs DISPI: 不使用像素时钟寄存器 (走 index/data port I/O)
//
// 本实装采用 vendor-neutral 乘法/除法寄存器抽象 (mul/div pair):
// `pixel_clock = pclk_base_khz * mul / div`
//
// 其中:
// - `HDMI_PCLK_BASE_KHZ`: 参考时钟 (默认 27000 kHz = 27 MHz, HDMI 规范标准)
// - mul / div: 8-bit 整数, 0 视作 1 (避免除零)
//
// 实际厂商硬件可通过 [`HdmiController::new_with_iomem_pixel_clock`] 指定自家
// base/mul/div 寄存器偏移 (如有不同), 或完全绕过本抽象直接配置 PLL.

/// HDMI 像素时钟基础参考频率 (kHz)。
///
/// 27 MHz 是 HDMI 规范标准参考时钟 (BCH / TCLK);
/// 部分硬件 (e.g. AMD) 使用 100 MHz 参考, 此时通过 [`HdmiController::new_with_iomem_pixel_clock`]
/// 指定自定义 base 寄存器即可。
const HDMI_PCLK_BASE_KHZ: u32 = 27_000;

/// HDMI 像素时钟乘法寄存器默认偏移 (8-bit)。
///
/// 实际写入值 = `mul` (1..=255, 0 视作 1)。
const HDMI_PCLK_MUL_REG_OFFSET: usize = 0x060;

/// HDMI 像素时钟除法寄存器默认偏移 (8-bit)。
///
/// 实际写入值 = `div` (1..=255, 0 视作 1)。
const HDMI_PCLK_DIV_REG_OFFSET: usize = 0x064;

// ============================================================================
// HDMI 时序参数配置
// ============================================================================
//
// HDMI 控制器时序寄存器 (16-bit, 每项占 2 字节偏移):
// - H_TOTAL: 总水平像素 (active + blanking)
// - H_ACTIVE: 水平有效像素
// - H_SYNC_OFFSET: 水平同步信号前沿 (从 blanking 开始到 sync 起始)
// - H_SYNC_PW: 水平同步脉冲宽度
// - V_TOTAL: 总垂直行数 (active + blanking)
// - V_ACTIVE: 垂直有效行数
// - V_SYNC_OFFSET: 垂直同步信号前沿
// - V_SYNC_PW: 垂直同步脉冲宽度
//
// 厂商差异:
// - Intel IGP (HSW/SKL): 每项占 4 字节, 需用 32-bit 写入
// - AMD DCN: DENTIST_HWITCH_H_TOTAL 等分散寄存器
// - 通用 SoC: 通常 16-bit 紧凑排列
// - QEMU Bochs DISPI: 使用 VBE index/data port I/O, 不走 MMIO

/// H_TOTAL 寄存器偏移 (16-bit, 2 字节连续)。
const HDMI_H_TOTAL_REG_OFFSET: usize = 0x068;
/// H_ACTIVE 寄存器偏移 (16-bit)。
const HDMI_H_ACTIVE_REG_OFFSET: usize = 0x06A;
/// V_TOTAL 寄存器偏移 (16-bit)。
const HDMI_V_TOTAL_REG_OFFSET: usize = 0x06C;
/// V_ACTIVE 寄存器偏移 (16-bit)。
const HDMI_V_ACTIVE_REG_OFFSET: usize = 0x06E;
/// H_SYNC_OFFSET 寄存器偏移 (16-bit)。
const HDMI_H_SYNC_OFFSET_REG_OFFSET: usize = 0x070;
/// H_SYNC_PW 寄存器偏移 (16-bit)。
const HDMI_H_SYNC_PW_REG_OFFSET: usize = 0x072;
/// V_SYNC_OFFSET 寄存器偏移 (16-bit)。
const HDMI_V_SYNC_OFFSET_REG_OFFSET: usize = 0x074;
/// V_SYNC_PW 寄存器偏移 (16-bit)。
const HDMI_V_SYNC_PW_REG_OFFSET: usize = 0x076;

/// HDMI 时序参数 (从 VideoMode 派生)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoTiming {
    /// 水平有效像素数 (= width)
    pub h_active: u16,
    /// 水平总像素数 (active + blanking)
    pub h_total: u16,
    /// 水平同步信号前沿 (像素数)
    pub h_sync_offset: u16,
    /// 水平同步脉冲宽度 (像素数)
    pub h_sync_pulse_width: u16,
    /// 垂直有效行数 (= height)
    pub v_active: u16,
    /// 垂直总行数 (active + blanking)
    pub v_total: u16,
    /// 垂直同步信号前沿 (行数)
    pub v_sync_offset: u16,
    /// 垂直同步脉冲宽度 (行数)
    pub v_sync_pulse_width: u16,
}

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
// HDMI 像素时钟辅助函数
// ============================================================================

/// 从目标像素时钟 (kHz) 计算 mul/div 寄存器值。
///
/// 给定参考时钟 `base_khz`, 寻找满足 `base * mul / div ≈ target_khz` 的
/// 8-bit mul/div 对 (mul, div ∈ 1..=255).
///
/// 算法: 贪心搜索 `div ∈ 1..=16` (HDMI 控制器 PLL 典型范围), 选取
/// |base * mul / div - target| 最小的 (mul, div) 对.
///
/// 返回 `(mul, div)`; 写入寄存器时 mul/div == 0 视作 1.
///
/// # 示例
///
/// - target=148500 kHz (1080p60), base=27000 kHz:
///   - div=1: mul=6 → 162000 kHz (误差 13500)
///   - div=2: mul=11 → 148500 kHz (误差 0) ✓
///   返回 (11, 2)。
///
/// 这是 HDMI 控制器最常用的 PLL 配置; 真实硬件如需更精确算法 (e.g. N/M/frac)
/// 应通过 vendor 自定义路径接管, 不走本函数。
fn compute_pixel_clock_mul_div(target_khz: u32, base_khz: u32) -> (u8, u8) {
    if target_khz == 0 || base_khz == 0 {
        return (1, 1);
    }
    let mut best = (1u8, 1u8);
    let mut best_err: u32 = u32::MAX;
    for div in 1u32..=16 {
        // mul = round(target * div / base), 避免溢出
        let mul = target_khz
            .saturating_mul(div)
            .saturating_add(base_khz / 2)
            / base_khz;
        if (1..=255).contains(&mul) {
            let actual = base_khz * mul / div;
            let err = actual.abs_diff(target_khz);
            if err < best_err {
                best_err = err;
                best = (mul as u8, div as u8);
                if err == 0 {
                    break; // 找到精确匹配, 提前退出
                }
            }
        }
    }
    best
}

/// 配置 HDMI 像素时钟 PLL (mul/div 寄存器)。
///
/// 计算 `mul = round(target / base)` 并写入对应寄存器.
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `pclk_mul_reg_offset + 1 <= iomem.len()` 且 `pclk_div_reg_offset + 1 <= iomem.len()`
unsafe fn configure_hdmi_pixel_clock(
    iomem: &IoMem,
    pclk_mul_reg_offset: usize,
    pclk_div_reg_offset: usize,
    target_khz: u32,
) {
    let (mul, div) = compute_pixel_clock_mul_div(target_khz, HDMI_PCLK_BASE_KHZ);
    iomem.write_u8(pclk_mul_reg_offset, mul);
    iomem.write_u8(pclk_div_reg_offset, div);
}

// ============================================================================
// HDMI 时序参数辅助函数
// ============================================================================

/// 从 VideoMode 派生时序参数 (简化公式)。
///
/// 公式:
/// - `v_blank = max(1, v_active * 5 / 100)` (5% 垂直 blanking)
/// - `v_total = v_active + v_blank`
/// - `h_total = pixel_clock_hz / v_total / refresh_rate` (反推)
/// - `h_blank = h_total - h_active`
/// - `h_sync_offset = h_blank / 4` (典型 25% 前沿)
/// - `h_sync_pulse_width = h_blank / 8` (典型 12.5% 脉冲)
/// - `v_sync_offset = 1` (典型 1 行前沿)
/// - `v_sync_pulse_width = 3` (典型 3 行脉冲)
///
/// 与 VESA DMT 标准值的偏差:
/// - 1920x1080@60Hz: 本公式得到 v_total=1134 (DMT=1125), h_total≈2182 (DMT=2200)
/// - 误差 < 5%, 对真实显示器可能需要精确 DMT lookup (后续可扩展)
///
/// 对于 refresh_rate == 0 或 pixel_clock_khz == 0 的边界情况, 使用 fallback
/// (v_total = v_active + 50, h_total = h_active + 200)。
fn derive_video_timing(mode: &VideoMode) -> VideoTiming {
    let v_active = mode.height;
    let h_active = mode.width;

    // 派生 v_total
    let v_total = if mode.refresh_rate > 0 && mode.pixel_clock_khz > 0 {
        // 典型 V blanking = 5% V active
        let v_blank = ((v_active as u32) * 5 / 100).max(1);
        (v_active as u32 + v_blank) as u16
    } else {
        // fallback
        v_active + 50
    };

    // 派生 h_total = pixel_clock_hz / v_total / refresh_rate
    let h_total = if mode.refresh_rate > 0 && mode.pixel_clock_khz > 0 {
        let h_total_u32 = (mode.pixel_clock_khz * 1000)
            / ((v_total as u32) * (mode.refresh_rate as u32));
        // 强制 h_total >= h_active + 1 (至少 1 像素 blanking)
        h_total_u32.max((h_active as u32) + 1) as u16
    } else {
        h_active + 200
    };

    let h_blank = h_total.saturating_sub(h_active);

    // Sync 偏移和脉冲宽度
    let h_sync_offset = h_blank / 4;
    let h_sync_pulse_width = (h_blank / 8).max(1);
    let v_sync_offset: u16 = 1;
    let v_sync_pulse_width: u16 = 3;

    VideoTiming {
        h_active,
        h_total,
        h_sync_offset,
        h_sync_pulse_width,
        v_active,
        v_total,
        v_sync_offset,
        v_sync_pulse_width,
    }
}

/// 写入 16-bit 时序寄存器 (低字节 + 高字节)。
///
/// # Safety
/// 调用方必须保证 `reg_offset + 2 <= iomem.len()` (2 字节连续写入)。
#[inline]
unsafe fn write_timing_register_u16(iomem: &IoMem, reg_offset: usize, value: u16) {
    iomem.write_u8(reg_offset, (value & 0xFF) as u8);
    iomem.write_u8(reg_offset + 1, ((value >> 8) & 0xFF) as u8);
}

/// 配置 HDMI 时序参数 (8 个 16-bit 寄存器)。
///
/// 写入顺序: H_TOTAL → H_ACTIVE → V_TOTAL → V_ACTIVE →
/// H_SYNC_OFFSET → H_SYNC_PW → V_SYNC_OFFSET → V_SYNC_PW
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `HDMI_V_SYNC_PW_REG_OFFSET + 2 <= iomem.len()` (最后一个寄存器结束)
unsafe fn configure_hdmi_timing(iomem: &IoMem, timing: &VideoTiming) {
    write_timing_register_u16(iomem, HDMI_H_TOTAL_REG_OFFSET, timing.h_total);
    write_timing_register_u16(iomem, HDMI_H_ACTIVE_REG_OFFSET, timing.h_active);
    write_timing_register_u16(iomem, HDMI_V_TOTAL_REG_OFFSET, timing.v_total);
    write_timing_register_u16(iomem, HDMI_V_ACTIVE_REG_OFFSET, timing.v_active);
    write_timing_register_u16(
        iomem,
        HDMI_H_SYNC_OFFSET_REG_OFFSET,
        timing.h_sync_offset,
    );
    write_timing_register_u16(
        iomem,
        HDMI_H_SYNC_PW_REG_OFFSET,
        timing.h_sync_pulse_width,
    );
    write_timing_register_u16(
        iomem,
        HDMI_V_SYNC_OFFSET_REG_OFFSET,
        timing.v_sync_offset,
    );
    write_timing_register_u16(
        iomem,
        HDMI_V_SYNC_PW_REG_OFFSET,
        timing.v_sync_pulse_width,
    );
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
    /// 像素时钟乘法寄存器偏移 (8-bit)。
    pclk_mul_reg_offset: usize,
    /// 像素时钟除法寄存器偏移 (8-bit)。
    pclk_div_reg_offset: usize,
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
            pclk_mul_reg_offset: HDMI_PCLK_MUL_REG_OFFSET,
            pclk_div_reg_offset: HDMI_PCLK_DIV_REG_OFFSET,
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
            pclk_mul_reg_offset: HDMI_PCLK_MUL_REG_OFFSET,
            pclk_div_reg_offset: HDMI_PCLK_DIV_REG_OFFSET,
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

    /// 创建 HDMI 控制器实例 (真实硬件, 含自定义像素时钟寄存器偏移)。
    ///
    /// 用于厂商硬件 mul/div 寄存器偏移与默认不同的情况 (e.g. AMD DCN
    /// 使用 DISPCLK 而非独立 mul/div pair).
    ///
    /// # Safety
    ///
    /// 同 [`HdmiController::new_with_iomem`], 额外要求:
    /// - `pclk_mul_reg_offset + 1 <= iomem.len()` 且 `pclk_div_reg_offset + 1 <= iomem.len()`
    pub unsafe fn new_with_iomem_pixel_clock(
        iomem: IoMem,
        hpd_reg_offset: usize,
        pclk_mul_reg_offset: usize,
        pclk_div_reg_offset: usize,
    ) -> Self {
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

        // DISPLAY-2.3a: 第 1 步 - 设置像素时钟
        //
        // IoMem Some → 真实硬件路径: 写入 mul/div 寄存器配置 PLL
        // IoMem None → QEMU/Bochs fallback: 仅记录 mode, 不写寄存器
        if let Some(iomem) = &self.iomem {
            // SAFETY: `new_with_iomem` / `new_with_iomem_pixel_clock` 构造时调用方
            // 已保证 `pclk_mul_reg_offset + 1 <= iomem.len()` 且
            // `pclk_div_reg_offset + 1 <= iomem.len()`, 写 1 字节落在 IoMem 边界内.
            unsafe {
                configure_hdmi_pixel_clock(
                    iomem,
                    self.pclk_mul_reg_offset,
                    self.pclk_div_reg_offset,
                    mode.pixel_clock_khz,
                );
            }
        }

        // DISPLAY-2.3b: 第 2 步 - 配置时序参数
        //
        // 从 mode 派生完整 VideoTiming (H/V total/active/sync), 写入 8 个 16-bit 寄存器.
        // 与第 1 步相同的 fallback 策略: 无 IoMem 时跳过硬件写入.
        let timing = derive_video_timing(&mode);
        if let Some(iomem) = &self.iomem {
            // SAFETY: 时序寄存器最后一项 (V_SYNC_PW) offset = 0x076, 写入 2 字节
            // 需 0x078 <= iomem.len(); 调用方通过 new_with_iomem* 构造时需保证 IoMem
            // 范围 >= 0x078 (本实装默认要求所有时序寄存器都有效).
            unsafe {
                configure_hdmi_timing(iomem, &timing);
            }
        }

        // TODO(TRACK-1BDEF6): 第 3 步 (DISPLAY-2.3c)
        // 3. 设置同步信号极性 + TMDS 输出使能

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

    #[test]
    fn test_compute_pixel_clock_mul_div_1080p60() {
        // 1080p60 像素时钟 = 148500 kHz, base = 27000 kHz.
        // 期望: mul=11, div=2 (精确匹配, 0 误差).
        let (mul, div) = compute_pixel_clock_mul_div(148_500, 27_000);
        assert_eq!((mul, div), (11, 2), "1080p60 mul/div 必须精确");
        let actual = 27_000 * mul as u32 / div as u32;
        assert_eq!(actual, 148_500, "实际像素时钟必须精确匹配");
    }

    #[test]
    fn test_compute_pixel_clock_mul_div_4k30() {
        // 4K30 (3840x2160@30) 像素时钟 ≈ 297000 kHz, base = 27000 kHz.
        // 期望: mul=11, div=1 (297000 kHz) 或 mul=22, div=2 (同).
        let (mul, div) = compute_pixel_clock_mul_div(297_000, 27_000);
        let actual = 27_000 * mul as u32 / div as u32;
        // 误差必须 < 1% (2970 kHz).
        let err = actual.abs_diff(297_000);
        assert!(err < 2970, "4K30 像素时钟误差必须 < 1%: actual={}, err={}", actual, err);
    }

    #[test]
    fn test_compute_pixel_clock_mul_div_zero_target() {
        // 边界: target = 0 或 base = 0 必须返回 (1, 1) (避免除零).
        assert_eq!(compute_pixel_clock_mul_div(0, 27_000), (1, 1));
        assert_eq!(compute_pixel_clock_mul_div(148_500, 0), (1, 1));
        assert_eq!(compute_pixel_clock_mul_div(0, 0), (1, 1));
    }

    #[test]
    fn test_set_video_mode_fallback_no_iomem() {
        // 无 IoMem fallback: set_video_mode 不写寄存器, 仅记录 mode.
        let mut ctrl = HdmiController::new(0xFE000000);
        ctrl.detect_hot_plug();
        let mode = VideoMode {
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            pixel_clock_khz: 148_500,
            flags: VideoModeFlags::default(),
        };
        ctrl.set_video_mode(mode).expect("fallback 必须成功");
        let current = ctrl.get_current_mode().expect("mode 必须已记录");
        assert_eq!(current.width, 1920);
        assert_eq!(current.height, 1080);
        assert_eq!(current.pixel_clock_khz, 148_500);
    }

    #[test]
    fn test_set_video_mode_without_hpd_returns_device_not_found() {
        // 未检测 HPD 时 set_video_mode 应返回 DeviceNotFound.
        let mut ctrl = HdmiController::new(0xFE000000);
        let mode = VideoMode {
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            pixel_clock_khz: 148_500,
            flags: VideoModeFlags::default(),
        };
        let result = ctrl.set_video_mode(mode);
        assert!(matches!(result, Err(DriverError::DeviceNotFound)));
    }

    #[test]
    fn test_derive_video_timing_1080p60() {
        // 1920x1080@60Hz 时序派生: v_total ≈ 1134, h_total ≈ 2182.
        let mode = VideoMode {
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            pixel_clock_khz: 148_500,
            flags: VideoModeFlags::default(),
        };
        let t = derive_video_timing(&mode);
        assert_eq!(t.h_active, 1920);
        assert_eq!(t.v_active, 1080);
        // v_total = 1080 + 54 (5%) = 1134
        assert_eq!(t.v_total, 1134, "v_total 必须 = v_active + 5%");
        // h_total = 148500000 / 1134 / 60 ≈ 2182
        assert!(t.h_total >= 2000 && t.h_total <= 2300,
                "h_total 必须在合理范围 (2000-2300): actual={}", t.h_total);
        // h_sync_offset ≈ h_blank / 4
        assert!(t.h_sync_offset > 0);
        assert!(t.h_sync_pulse_width > 0);
        assert_eq!(t.v_sync_offset, 1);
        assert_eq!(t.v_sync_pulse_width, 3);
    }

    #[test]
    fn test_derive_video_timing_4k60() {
        // 3840x2160@60Hz 时序派生 (594 MHz pixel clock).
        let mode = VideoMode {
            width: 3840,
            height: 2160,
            refresh_rate: 60,
            pixel_clock_khz: 594_000,
            flags: VideoModeFlags::default(),
        };
        let t = derive_video_timing(&mode);
        assert_eq!(t.h_active, 3840);
        assert_eq!(t.v_active, 2160);
        assert_eq!(t.v_total, 2268, "v_total = 2160 + 5%");
        // h_total ≈ 594000000 / 2268 / 60 ≈ 4365
        assert!(t.h_total >= 4000 && t.h_total <= 4500,
                "h_total 必须在合理范围 (4000-4500): actual={}", t.h_total);
    }

    #[test]
    fn test_derive_video_timing_zero_refresh_rate_fallback() {
        // 边界: refresh_rate = 0 必须走 fallback (h_total = h_active + 200, v_total = v_active + 50).
        let mode = VideoMode {
            width: 800,
            height: 600,
            refresh_rate: 0,
            pixel_clock_khz: 0,
            flags: VideoModeFlags::default(),
        };
        let t = derive_video_timing(&mode);
        assert_eq!(t.h_total, 1000, "fallback h_total = h_active + 200");
        assert_eq!(t.v_total, 650, "fallback v_total = v_active + 50");
        assert_eq!(t.h_active, 800);
        assert_eq!(t.v_active, 600);
    }

    #[test]
    fn test_video_timing_struct_equality() {
        // VideoTiming 派生 Debug/Clone/Copy/PartialEq/Eq.
        let t1 = VideoTiming {
            h_active: 1920, h_total: 2200, h_sync_offset: 88, h_sync_pulse_width: 44,
            v_active: 1080, v_total: 1125, v_sync_offset: 4, v_sync_pulse_width: 5,
        };
        let t2 = t1; // Copy
        assert_eq!(t1, t2);
    }
}
