use crate::kernel::driver::display::framebuffer::{Color, Framebuffer};
use crate::kernel::driver::display::framebuffer::colors;
use crate::kernel::driver::display::font::Font;

/// 图形控制台 — 在帧缓冲上模拟文本终端
///
/// 替代 VGA 文本模式，使用 PSF 字体在帧缓冲上绘制字符。
/// 支持自动换行、屏幕滚动。
pub struct GfxConsole {
    fb: *mut Framebuffer,
    font: &'static Font,
    cursor_x: u32,
    cursor_y: u32,
    cols: u32,
    rows: u32,
    fg_color: Color,
    bg_color: Color,
    top_margin: u32,
}

impl GfxConsole {
    pub fn new(fb: *mut Framebuffer, font: &'static Font) -> Self {
        let fb_ref = unsafe { &*fb };
        let cols = fb_ref.width() / font.glyph_width;
        let rows = fb_ref.height() / font.glyph_height;
        Self {
            fb,
            font,
            cursor_x: 0,
            cursor_y: 0,
            cols,
            rows,
            fg_color: colors::WHITE,
            bg_color: colors::BLACK,
            top_margin: 0,
        }
    }

    #[inline]
    unsafe fn fb_mut(&self) -> &mut Framebuffer {
        &mut *self.fb
    }

    pub fn set_margin(&mut self, top: u32) {
        self.top_margin = top.min(self.rows);
    }

    pub fn set_colors(&mut self, fg: Color, bg: Color) {
        self.fg_color = fg;
        self.bg_color = bg;
    }

    pub fn clear(&mut self) {
        let fb = unsafe { self.fb_mut() };
        fb.fill_rect(
            crate::kernel::driver::display::framebuffer::Rect::new(
                0, 0, fb.width(), fb.height(),
            ),
            self.bg_color,
        );
        self.cursor_x = 0;
        self.cursor_y = self.top_margin;
    }

    pub fn putchar(&mut self, ch: char) {
        match ch {
            '\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            '\r' => self.cursor_x = 0,
            '\t' => self.cursor_x = (self.cursor_x + 4) & !3,
            '\x08' => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    let px = self.cursor_x * self.font.glyph_width;
                    let py = self.cursor_y * self.font.glyph_height;
                    let fg = self.fg_color;
                    let bg = self.bg_color;
                    let fb = unsafe { &mut *self.fb };
                    self.font.render_char(fb, ' ', px, py, fg, bg);
                }
            }
            _ => {
                if ch.is_ascii_graphic() || ch == ' ' {
                    let px = self.cursor_x * self.font.glyph_width;
                    let py = self.cursor_y * self.font.glyph_height;
                    let fg = self.fg_color;
                    let bg = self.bg_color;
                    let fb = unsafe { &mut *self.fb };
                    self.font.render_char(fb, ch, px, py, fg, bg);
                    self.cursor_x += 1;
                }
            }
        }

        if self.cursor_x >= self.cols {
            self.cursor_x = 0;
            self.cursor_y += 1;
        }
        if self.cursor_y >= self.rows {
            self.scroll_up(1);
        }
    }

    fn scroll_up(&mut self, lines: u32) {
        let fb = unsafe { self.fb_mut() };
        let glyph_h = self.font.glyph_height;
        let margin_px = self.top_margin * glyph_h;
        let scroll_h = glyph_h * lines;
        let scroll_start = margin_px + scroll_h;
        let scroll_end = fb.height();

        if scroll_start >= scroll_end {
            return;
        }

        let pitch = fb.pitch();

        unsafe {
            let src = fb.buffer_ptr().add((scroll_start * pitch) as usize);
            let dst = fb.buffer_ptr().add(margin_px as usize * pitch as usize);
            let count = ((scroll_end - scroll_start) * pitch) as usize;
            core::ptr::copy(src, dst, count);

            let clear_start = (scroll_end - scroll_h) * pitch;
            let clear_size = (scroll_h * pitch) as usize;
            core::ptr::write_bytes(
                fb.buffer_ptr().add(clear_start as usize),
                0u8,
                clear_size,
            );
        }

        self.cursor_y -= lines;
        if self.cursor_y < self.top_margin {
            self.cursor_y = self.top_margin;
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.putchar(ch);
        }
    }

    /// 以高亮样式写入一行日志（红色背景用于 CRIT，黄色前景用于 WARN）
    pub fn write_log_line(&mut self, s: &str) {
        for ch in s.chars() {
            self.putchar(ch);
        }
    }
}

impl core::fmt::Write for GfxConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_str(s);
        Ok(())
    }
}