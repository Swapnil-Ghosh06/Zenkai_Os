//! Zenkai OS - UEFI Graphics Output Protocol (GOP) & Framebuffer Driver
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Direct hardware 32-bit linear framebuffer rendering with bounded viewports.

use crate::font::{get_glyph, FONT_HEIGHT, FONT_WIDTH};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    // Modern Slate/Indigo Desktop Palette
    pub const BG_OBSIDIAN: Self = Self::new(11, 16, 27);      // #0B101B Desktop Canvas
    pub const BG_BAR: Self = Self::new(17, 24, 39);           // #111827 Top Bar & Dock
    pub const BG_PANEL: Self = Self::new(22, 31, 48);         // #161F30 Window / Panel
    pub const BG_CARD: Self = Self::new(30, 41, 59);          // #1E293B Elevated Card
    pub const BG_SELECT: Self = Self::new(37, 56, 84);        // #253854 Selected Row
    pub const CYAN: Self = Self::new(0, 210, 255);            // #00D2FF Cyber Cyan
    pub const INDIGO: Self = Self::new(99, 102, 241);         // #6366F1 Accent Indigo
    pub const EMERALD: Self = Self::new(16, 230, 128);        // #10E680 Status Green
    pub const AMBER: Self = Self::new(245, 158, 11);          // #F59E0B Warning / Highlight
    pub const CRIMSON: Self = Self::new(239, 68, 68);         // #EF4444 Error / Fault
    pub const WHITE: Self = Self::new(241, 245, 249);         // #F1F5F9 Crisp White
    pub const SLATE: Self = Self::new(148, 163, 184);         // #94A3B8 Muted Text
    pub const DARK_SLATE: Self = Self::new(51, 65, 85);       // #334155 Borders & Dividers
}

pub struct Framebuffer {
    base: *mut u32,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub is_bgr: bool,
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Creates a Framebuffer instance from raw UEFI parameters.
    pub unsafe fn from_raw(base: *mut u32, width: usize, height: usize, stride: usize, is_bgr: bool) -> Self {
        Self {
            base,
            width,
            height,
            stride,
            is_bgr,
        }
    }

    #[inline(always)]
    pub fn pack_color(&self, color: Color) -> u32 {
        if self.is_bgr {
            ((color.b as u32) << 16) | ((color.g as u32) << 8) | (color.r as u32)
        } else {
            ((color.r as u32) << 16) | ((color.g as u32) << 8) | (color.b as u32)
        }
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x < self.width && y < self.height {
            let offset = y * self.stride + x;
            unsafe {
                self.base.add(offset).write_volatile(self.pack_color(color));
            }
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if x >= x_end || y >= y_end {
            return;
        }
        let packed = self.pack_color(color);

        for cy in y..y_end {
            let row_start = cy * self.stride + x;
            let count = x_end - x;
            unsafe {
                let ptr = self.base.add(row_start);
                for i in 0..count {
                    ptr.add(i).write_volatile(packed);
                }
            }
        }
    }

    pub fn draw_border(&mut self, x: usize, y: usize, w: usize, h: usize, thickness: usize, color: Color) {
        self.fill_rect(x, y, w, thickness, color);
        self.fill_rect(x, y + h.saturating_sub(thickness), w, thickness, color);
        self.fill_rect(x, y, thickness, h, color);
        self.fill_rect(x + w.saturating_sub(thickness), y, thickness, h, color);
    }

    /// Draws a modern card with subtle 2px chamfered corners and 1px border.
    pub fn draw_card(&mut self, x: usize, y: usize, w: usize, h: usize, fill: Color, border: Color) {
        if w < 6 || h < 6 {
            return;
        }
        self.fill_rect(x + 2, y, w - 4, h, fill);
        self.fill_rect(x, y + 2, w, h - 4, fill);
        // Border edges
        self.fill_rect(x + 2, y, w - 4, 1, border);
        self.fill_rect(x + 2, y + h - 1, w - 4, 1, border);
        self.fill_rect(x, y + 2, 1, h - 4, border);
        self.fill_rect(x + w - 1, y + 2, 1, h - 4, border);
        // Corner bevels
        self.set_pixel(x + 1, y + 1, border);
        self.set_pixel(x + w - 2, y + 1, border);
        self.set_pixel(x + 1, y + h - 2, border);
        self.set_pixel(x + w - 2, y + h - 2, border);
    }

    pub fn draw_char(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Option<Color>, scale: usize) {
        let glyph = get_glyph(ch);
        let packed_fg = self.pack_color(fg);
        let packed_bg = bg.map(|c| self.pack_color(c));

        for row in 0..FONT_HEIGHT {
            let bits = glyph[row];
            for col in 0..FONT_WIDTH {
                let is_fg = (bits & (0x80 >> col)) != 0;
                let px_color = if is_fg { Some(packed_fg) } else { packed_bg };

                if let Some(val) = px_color {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let cx = x + col * scale + sx;
                            let cy = y + row * scale + sy;
                            if cx < self.width && cy < self.height {
                                let offset = cy * self.stride + cx;
                                unsafe {
                                    self.base.add(offset).write_volatile(val);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn draw_text(&mut self, x: usize, y: usize, text: &str, fg: Color, bg: Option<Color>, scale: usize) {
        let mut cur_x = x;
        let mut cur_y = y;

        for ch in text.chars() {
            if ch == '\n' {
                cur_x = x;
                cur_y += FONT_HEIGHT * scale;
                continue;
            }
            self.draw_char(cur_x, cur_y, ch, fg, bg, scale);
            cur_x += FONT_WIDTH * scale;
        }
    }

    /// Scrolls only a bounded rectangular region `(rx, ry, rw, rh)` upward by `pixel_rows`.
    pub fn scroll_region(&mut self, rx: usize, ry: usize, rw: usize, rh: usize, pixel_rows: usize, fill_color: Color) {
        let rx_end = (rx + rw).min(self.width);
        let ry_end = (ry + rh).min(self.height);
        if rx >= rx_end || ry >= ry_end {
            return;
        }
        let actual_w = rx_end - rx;
        let actual_h = ry_end - ry;

        if pixel_rows >= actual_h {
            self.fill_rect(rx, ry, actual_w, actual_h, fill_color);
            return;
        }

        let rows_to_copy = actual_h - pixel_rows;
        for row in 0..rows_to_copy {
            let dst_y = ry + row;
            let src_y = ry + row + pixel_rows;
            let dst_offset = dst_y * self.stride + rx;
            let src_offset = src_y * self.stride + rx;
            unsafe {
                core::ptr::copy(self.base.add(src_offset), self.base.add(dst_offset), actual_w);
            }
        }

        self.fill_rect(rx, ry + rows_to_copy, actual_w, pixel_rows, fill_color);
    }
}
