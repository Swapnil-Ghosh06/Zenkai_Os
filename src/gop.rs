//! Zenkai OS - UEFI Graphics Output Protocol (GOP) & Framebuffer Driver
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Direct hardware 32-bit linear framebuffer rendering.

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

    // High-aesthetic modern dark-palette constants
    pub const BG_OBSIDIAN: Self = Self::new(10, 14, 20);      // #0A0E14
    pub const BG_PANEL: Self = Self::new(18, 26, 38);         // #121A26
    pub const BG_CARD: Self = Self::new(26, 38, 54);          // #1A2636
    pub const CYAN: Self = Self::new(0, 210, 255);            // #00D2FF Cyber Cyan
    pub const EMERALD: Self = Self::new(0, 230, 118);         // #00E676 High-tech Green
    pub const AMBER: Self = Self::new(255, 171, 0);           // #FFAB00 Warning/Highlight
    pub const CRIMSON: Self = Self::new(255, 61, 0);          // #FF3D00 Error/Fault
    pub const WHITE: Self = Self::new(245, 248, 255);         // Crisp White
    pub const SLATE: Self = Self::new(144, 164, 174);         // Subtle Text
    pub const DARK_SLATE: Self = Self::new(55, 71, 79);       // Borders & Dividers
}

pub struct Framebuffer {
    base: *mut u32,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub is_bgr: bool,
}

// Framebuffer raw pointer is statically safe inside the bare-metal single-core UEFI environment
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
        let packed = self.pack_color(color);
        for y in 0..self.height {
            for x in 0..self.width {
                let offset = y * self.stride + x;
                unsafe {
                    self.base.add(offset).write_volatile(packed);
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let packed = self.pack_color(color);

        for cy in y..y_end {
            for cx in x..x_end {
                let offset = cy * self.stride + cx;
                unsafe {
                    self.base.add(offset).write_volatile(packed);
                }
            }
        }
    }

    pub fn draw_border(&mut self, x: usize, y: usize, w: usize, h: usize, thickness: usize, color: Color) {
        self.fill_rect(x, y, w, thickness, color); // Top
        self.fill_rect(x, y + h.saturating_sub(thickness), w, thickness, color); // Bottom
        self.fill_rect(x, y, thickness, h, color); // Left
        self.fill_rect(x + w.saturating_sub(thickness), y, thickness, h, color); // Right
    }

    pub fn draw_char(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Option<Color>, scale: usize) {
        let glyph = get_glyph(ch);
        let packed_fg = self.pack_color(fg);
        let packed_bg = bg.map(|c| self.pack_color(c));

        for row in 0..FONT_HEIGHT {
            let bits = glyph[row];
            for col in 0..FONT_WIDTH {
                let is_fg = (bits & (0x80 >> col)) != 0;
                let px_color = if is_fg {
                    Some(packed_fg)
                } else {
                    packed_bg
                };

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

    /// Scrolls the screen vertically by `pixel_rows` upward, filling the exposed bottom with `fill_color`.
    pub fn scroll_up(&mut self, pixel_rows: usize, fill_color: Color) {
        if pixel_rows >= self.height {
            self.clear(fill_color);
            return;
        }

        // Copy scanlines up
        let rows_to_copy = self.height - pixel_rows;
        for y in 0..rows_to_copy {
            let src_y = y + pixel_rows;
            let dst_offset = y * self.stride;
            let src_offset = src_y * self.stride;
            unsafe {
                core::ptr::copy(
                    self.base.add(src_offset),
                    self.base.add(dst_offset),
                    self.width,
                );
            }
        }

        // Clear newly exposed bottom rows
        let packed_fill = self.pack_color(fill_color);
        for y in rows_to_copy..self.height {
            for x in 0..self.width {
                let offset = y * self.stride + x;
                unsafe {
                    self.base.add(offset).write_volatile(packed_fill);
                }
            }
        }
    }
}
