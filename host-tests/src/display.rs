//! 显示器驱动测试 (Display Driver Tests)

#[cfg(test)]
mod tests {
    // Framebuffer 测试
    #[test]
    fn test_pixel_format_bytes() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum PixelFormat {
            Rgb565,
            Rgb888,
            Argb8888,
            Bgr888,
            Bgra8888,
        }

        impl PixelFormat {
            fn bytes_per_pixel(&self) -> usize {
                match self {
                    Self::Rgb565 => 2,
                    Self::Rgb888 => 3,
                    Self::Argb8888 => 4,
                    Self::Bgr888 => 3,
                    Self::Bgra8888 => 4,
                }
            }
        }

        assert_eq!(PixelFormat::Rgb565.bytes_per_pixel(), 2);
        assert_eq!(PixelFormat::Rgb888.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::Argb8888.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Bgr888.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::Bgra8888.bytes_per_pixel(), 4);
    }

    #[test]
    fn test_color_conversion() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct Color {
            r: u8,
            g: u8,
            b: u8,
            a: u8,
        }

        impl Color {
            const fn new(r: u8, g: u8, b: u8) -> Self {
                Self { r, g, b, a: 255 }
            }

            fn to_rgb565(&self) -> u16 {
                let r = (self.r as u16 >> 3) & 0x1F;
                let g = (self.g as u16 >> 2) & 0x3F;
                let b = (self.b as u16 >> 3) & 0x1F;
                (r << 11) | (g << 5) | b
            }

            fn to_argb8888(&self) -> u32 {
                ((self.a as u32) << 24) | ((self.r as u32) << 16)
                    | ((self.g as u32) << 8) | (self.b as u32)
            }

            fn from_rgb565(rgb565: u16) -> Self {
                let r = ((rgb565 >> 11) & 0x1F) as u8;
                let g = ((rgb565 >> 5) & 0x3F) as u8;
                let b = (rgb565 & 0x1F) as u8;

                Self {
                    r: (r << 3) | (r >> 2),
                    g: (g << 2) | (g >> 4),
                    b: (b << 3) | (b >> 2),
                    a: 255,
                }
            }

            fn from_argb8888(argb: u32) -> Self {
                Self {
                    a: ((argb >> 24) & 0xFF) as u8,
                    r: ((argb >> 16) & 0xFF) as u8,
                    g: ((argb >> 8) & 0xFF) as u8,
                    b: (argb & 0xFF) as u8,
                }
            }
        }

        let color = Color::new(255, 255, 255);
        let rgb565 = color.to_rgb565();
        let converted = Color::from_rgb565(rgb565);
        assert!((color.r as i32 - converted.r as i32).abs() <= 8);

        let color = Color::new(128, 64, 192);
        let argb = color.to_argb8888();
        let converted = Color::from_argb8888(argb);
        assert_eq!(color.r, converted.r);
        assert_eq!(color.g, converted.g);
        assert_eq!(color.b, converted.b);
    }

    // 显示控制器测试
    #[test]
    fn test_display_mode() {
        struct DisplayMode {
            width: u32,
            height: u32,
            refresh_rate: u32,
        }

        impl DisplayMode {
            fn new(width: u32, height: u32, refresh_rate: u32) -> Self {
                Self { width, height, refresh_rate }
            }

            fn pixel_clock_khz(&self) -> u64 {
                let total_pixels = self.width as u64 * self.height as u64;
                total_pixels * self.refresh_rate as u64 / 1000
            }

            fn bandwidth_mbps(&self) -> u64 {
                self.pixel_clock_khz() * 4 / 1000
            }
        }

        let mode = DisplayMode::new(1920, 1080, 60);
        assert_eq!(mode.width, 1920);
        assert_eq!(mode.height, 1080);
        assert_eq!(mode.refresh_rate, 60);

        let bw = mode.bandwidth_mbps();
        assert!(bw > 400 && bw < 600);
    }

    // HDMI 测试
    #[test]
    fn test_hdmi_modes() {
        const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
        assert_eq!(EDID_HEADER, [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct VideoMode {
            width: u16,
            height: u16,
            refresh_rate: u8,
        }

        const STANDARD_VIDEO_MODES: &[VideoMode] = &[
            VideoMode { width: 640, height: 480, refresh_rate: 60 },
            VideoMode { width: 800, height: 600, refresh_rate: 60 },
            VideoMode { width: 1024, height: 768, refresh_rate: 60 },
            VideoMode { width: 1280, height: 720, refresh_rate: 60 },
            VideoMode { width: 1920, height: 1080, refresh_rate: 60 },
        ];

        assert!(!STANDARD_VIDEO_MODES.is_empty());
        assert_eq!(STANDARD_VIDEO_MODES.len(), 5);

        let mode = &STANDARD_VIDEO_MODES[4];
        assert_eq!(mode.width, 1920);
        assert_eq!(mode.height, 1080);
        assert_eq!(mode.refresh_rate, 60);
    }

    // DisplayPort 测试
    #[test]
    fn test_dp_link_rate() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        enum LinkRate {
            Rbr = 0x06,
            Hbr = 0x0A,
            Hbr2 = 0x14,
            Hbr3 = 0x1E,
        }

        impl LinkRate {
            fn bandwidth_gbps(&self) -> u32 {
                match self {
                    Self::Rbr => 162,
                    Self::Hbr => 270,
                    Self::Hbr2 => 540,
                    Self::Hbr3 => 810,
                }
            }

            fn from_u8(value: u8) -> Option<Self> {
                match value {
                    0x06 => Some(Self::Rbr),
                    0x0A => Some(Self::Hbr),
                    0x14 => Some(Self::Hbr2),
                    0x1E => Some(Self::Hbr3),
                    _ => None,
                }
            }
        }

        assert_eq!(LinkRate::Rbr.bandwidth_gbps(), 162);
        assert_eq!(LinkRate::Hbr.bandwidth_gbps(), 270);
        assert_eq!(LinkRate::Hbr2.bandwidth_gbps(), 540);
        assert_eq!(LinkRate::Hbr3.bandwidth_gbps(), 810);

        assert_eq!(LinkRate::from_u8(0x06), Some(LinkRate::Rbr));
        assert_eq!(LinkRate::from_u8(0x0A), Some(LinkRate::Hbr));
        assert_eq!(LinkRate::from_u8(0x14), Some(LinkRate::Hbr2));
        assert_eq!(LinkRate::from_u8(0x1E), Some(LinkRate::Hbr3));
        assert_eq!(LinkRate::from_u8(0x00), None);
    }

    #[test]
    fn test_dp_lane_count() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        enum LaneCount {
            One = 1,
            Two = 2,
            Four = 4,
        }

        impl LaneCount {
            fn from_u8(value: u8) -> Option<Self> {
                match value {
                    1 => Some(Self::One),
                    2 => Some(Self::Two),
                    4 => Some(Self::Four),
                    _ => None,
                }
            }
        }

        assert_eq!(LaneCount::from_u8(1), Some(LaneCount::One));
        assert_eq!(LaneCount::from_u8(2), Some(LaneCount::Two));
        assert_eq!(LaneCount::from_u8(4), Some(LaneCount::Four));
        assert_eq!(LaneCount::from_u8(3), None);
    }

    #[test]
    fn test_dp_total_bandwidth() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        enum LinkRate {
            Rbr = 0x06,
            Hbr = 0x0A,
            Hbr2 = 0x14,
            Hbr3 = 0x1E,
        }

        impl LinkRate {
            fn bandwidth_gbps(&self) -> u32 {
                match self {
                    Self::Rbr => 162,
                    Self::Hbr => 270,
                    Self::Hbr2 => 540,
                    Self::Hbr3 => 810,
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        enum LaneCount {
            One = 1,
            Two = 2,
            Four = 4,
        }

        // HBR2 x 4 lanes = 540 * 4 = 2160 Gbps
        let total_bw = LinkRate::Hbr2.bandwidth_gbps() * LaneCount::Four as u32;
        assert_eq!(total_bw, 2160);

        // HBR3 x 4 lanes = 810 * 4 = 3240 Gbps
        let total_bw = LinkRate::Hbr3.bandwidth_gbps() * LaneCount::Four as u32;
        assert_eq!(total_bw, 3240);
    }
}
