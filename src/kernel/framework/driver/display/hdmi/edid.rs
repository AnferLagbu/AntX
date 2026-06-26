//! EDID (Extended Display Identification Data) 解析
//!
//! EDID 是显示器提供的描述自身能力的数据结构, 由 VESA 标准化.
//! HDMI 通过 DDC (I2C) 总线读取 EDID block 0 (128 字节), 解析后得到:
//! - 厂商 / 产品 / 序列号
//! - 基本显示参数 (尺寸 / 输入类型)
//! - 颜色特性
//! - 详细时序描述符 (用于 preferred resolution)
//!
//! 本模块不涉及硬件 I/O; 硬件访问由 [`super::ddc`] 提供.
//!
//! ## EDID v1.4 块结构 (128 字节)
//!
//! | 偏移 | 长度 | 内容 |
//! |------|------|------|
//! | 0..8  | 8  | Header (固定 `00 FF FF FF FF FF FF 00`) |
//! | 8..10 | 2  | 厂商 ID (3 字符 packed) |
//! | 10..12 | 2 | 产品代码 (LE) |
//! | 12..16 | 4 | 序列号 (LE) |
//! | 16    | 1 | 制造周 |
//! | 17    | 1 | 制造年 (offset from 1990) |
//! | 18..19 | 2 | EDID 版本 / 修订 |
//! | 20..25 | 6 | 基本显示参数 |
//! | 25..35 | 10 | 颜色特性 |
//! | 35..38 | 3 | 已建立时序 (EST) |
//! | 38..53 | 16 | 标准时序标识符 (8 个) |
//! | 54..126 | 72 | 详细时序描述符 (4 个, 各 18 字节) |
//! | 127   | 1 | 校验和 (前 127 字节累加 = 0) |

use alloc::vec::Vec;
use super::super::super::framework::{DriverError, Result};

/// EDID 数据最大长度 (block 0 + extension block).
pub(super) const EDID_MAX_LENGTH: usize = 256;

/// EDID 块 0 长度.
pub(super) const EDID_BLOCK_SIZE: usize = 128;

/// EDID 头 (固定 8 字节 magic).
pub(super) const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

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
    pub supported_modes: Vec<super::VideoMode>,
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
        let checksum: u8 = data[0..EDID_BLOCK_SIZE].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
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

/// 填充 mock EDID 数据 (用于无硬件 / DDC 失败 fallback)。
///
/// 内容为硬编码的 1920x1080 @ 60Hz EDID block 0,
/// 校验和正确, 可被 `Edid::parse` 解析通过。
pub(super) fn fill_mock_edid(edid_data: &mut [u8; EDID_MAX_LENGTH]) {
    edid_data[0..8].copy_from_slice(&EDID_HEADER);

    // 厂商 ID (示例: "QUEENX")
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
    for i in 0..(EDID_BLOCK_SIZE - 1) {
        checksum = checksum.wrapping_add(edid_data[i]);
    }
    edid_data[EDID_BLOCK_SIZE - 1] = (256 - checksum as usize) as u8;
}
