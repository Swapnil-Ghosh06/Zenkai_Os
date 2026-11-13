//! Zenkai OS - UEFI Graphics Output Protocol (GOP) & Anti-Aliased Renderer
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Smooth 32-bit linear framebuffer with alpha-blended typography, rounded surfaces, and gradients.

use crate::font_aa::{AaFace, WORDMARK_COV, WORDMARK_H, WORDMARK_W};

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

    // Modern Linux Distro Palette (Catppuccin / Pop!_OS / Twilight Indigo)
    pub const BG_TOP: Self = Self::new(15, 20, 35);         // #0F1423 Deep Midnight Indigo
    pub const BG_BOTTOM: Self = Self::new(26, 34, 56);      // #1A2238 Twilight Slate
    pub const SURFACE: Self = Self::new(22, 27, 40);        // #161B28 Window Body
    pub const SURFACE_ALT: Self = Self::new(29, 36, 54);    // #1D2436 Titlebar / Dock
    pub const SURFACE_HOVER: Self = Self::new(39, 48, 71);  // #273047 Selected Row
    pub const BORDER: Self = Self::new(46, 56, 82);         // #2E3852 Hairline Border
    pub const SHADOW: Self = Self::new(7, 10, 18);          // #070A12 Soft Shadow

    pub const TEXT_PRIMARY: Self = Self::new(241, 245, 249);// #F1F5F9 Crisp White
    pub const TEXT_MUTED: Self = Self::new(148, 163, 184);  // #94A3B8 Cool Slate
    pub const TEXT_DIM: Self = Self::new(100, 116, 139);    // #64748B Subtle Caption

    pub const ACCENT: Self = Self::new(96, 165, 250);       // #60A5FA Sky Blue
    pub const VIOLET: Self = Self::new(167, 139, 250);      // #A78BFA Soft Violet
    pub const SAGE: Self = Self::new(52, 211, 153);         // #34D399 Mint Green
    pub const AMBER: Self = Self::new(251, 191, 36);        // #FBBF24 Soft Gold
    pub const CORAL: Self = Self::new(248, 113, 113);       // #F87171 Soft Coral

    // Compatibility aliases for console macros
    pub const BG_OBSIDIAN: Self = Self::BG_TOP;
    pub const BG_PANEL: Self = Self::SURFACE;
    pub const WHITE: Self = Self::TEXT_PRIMARY;
    pub const CYAN: Self = Self::ACCENT;
    pub const EMERALD: Self = Self::SAGE;
    pub const CRIMSON: Self = Self::CORAL;
    pub const SLATE: Self = Self::TEXT_MUTED;
    pub const DARK_SLATE: Self = Self::BORDER;
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
    pub fn pack_color(&self, c: Color) -> u32 {
        if self.is_bgr {
            // Little-endian memory [B, G, R, X] => (R << 16) | (G << 8) | B
            ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32)
        } else {
            // Little-endian memory [R, G, B, X] => (B << 16) | (G << 8) | R
            ((c.b as u32) << 16) | ((c.g as u32) << 8) | (c.r as u32)
        }
    }

    #[inline(always)]
    pub fn unpack_color(&self, raw: u32) -> Color {
        if self.is_bgr {
            Color::new(((raw >> 16) & 0xFF) as u8, ((raw >> 8) & 0xFF) as u8, (raw & 0xFF) as u8)
        } else {
            Color::new((raw & 0xFF) as u8, ((raw >> 8) & 0xFF) as u8, ((raw >> 16) & 0xFF) as u8)
        }
    }

    #[inline(always)]
    pub fn blend(bg: Color, fg: Color, alpha: u8) -> Color {
        let a = alpha as u16;
        let inv = 255 - a;
        Color::new(
            ((bg.r as u16 * inv + fg.r as u16 * a) / 255) as u8,
            ((bg.g as u16 * inv + fg.g as u16 * a) / 255) as u8,
            ((bg.b as u16 * inv + fg.b as u16 * a) / 255) as u8,
        )
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

    #[inline(always)]
    pub fn blend_pixel(&mut self, x: usize, y: usize, fg: Color, alpha: u8) {
        if alpha == 0 || x >= self.width || y >= self.height {
            return;
        }
        if alpha == 255 {
            self.set_pixel(x, y, fg);
            return;
        }
        let offset = y * self.stride + x;
        let raw_bg = unsafe { self.base.add(offset).read_volatile() };
        let blended = Self::blend(self.unpack_color(raw_bg), fg, alpha);
        unsafe {
            self.base.add(offset).write_volatile(self.pack_color(blended));
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }

    /// Draws a smooth vertical atmospheric gradient across the whole screen.
    pub fn fill_gradient(&mut self, top: Color, bottom: Color) {
        let h = self.height.max(1);
        for y in 0..self.height {
            let alpha = ((y * 255) / h) as u8;
            let row_col = Self::blend(top, bottom, alpha);
            let packed = self.pack_color(row_col);
            let row_start = y * self.stride;
            unsafe {
                let ptr = self.base.add(row_start);
                for x in 0..self.width {
                    ptr.add(x).write_volatile(packed);
                }
            }
        }
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

    /// Draws a smooth rounded rectangle with circular corners and optional 1px hairline border.
    pub fn draw_rounded_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, fill: Color, border: Option<Color>) {
        if w < r * 2 || h < r * 2 {
            self.fill_rect(x, y, w, h, fill);
            return;
        }
        let r_sq = (r * r) as isize;
        let r_inner_sq = ((r.saturating_sub(1)) * (r.saturating_sub(1))) as isize;

        // Fill middle horizontal band
        self.fill_rect(x, y + r, w, h - 2 * r, fill);
        // Fill top and bottom center strips
        self.fill_rect(x + r, y, w - 2 * r, r, fill);
        self.fill_rect(x + r, y + h - r, w - 2 * r, r, fill);

        if let Some(b) = border {
            self.fill_rect(x + r, y, w - 2 * r, 1, b);
            self.fill_rect(x + r, y + h - 1, w - 2 * r, 1, b);
            self.fill_rect(x, y + r, 1, h - 2 * r, b);
            self.fill_rect(x + w - 1, y + r, 1, h - 2 * r, b);
        }

        // Fill 4 rounded corners
        for dy in 0..r {
            for dx in 0..r {
                let dist_sq = ((r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy)) as isize;
                if dist_sq <= r_sq {
                    let col = if let Some(b) = border {
                        if dist_sq >= r_inner_sq { b } else { fill }
                    } else {
                        fill
                    };
                    self.set_pixel(x + dx, y + dy, col);
                    self.set_pixel(x + w - 1 - dx, y + dy, col);
                    self.set_pixel(x + dx, y + h - 1 - dy, col);
                    self.set_pixel(x + w - 1 - dx, y + h - 1 - dy, col);
                }
            }
        }
    }

    /// Draws a soft drop shadow behind a floating rounded window.
    pub fn draw_shadow(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize) {
        for layer in (1..=6).rev() {
            let alpha = (7 - layer) as u8 * 14;
            let sx = x.saturating_sub(layer / 2);
            let sy = y + layer * 2;
            let sw = w + layer;
            let sh = h + layer;
            let x_end = (sx + sw).min(self.width);
            let y_end = (sy + sh).min(self.height);
            // Fast soft shadow on bottom and right edges
            for py in (y + h.saturating_sub(r))..y_end {
                for px in sx..x_end {
                    self.blend_pixel(px, py, Color::SHADOW, alpha);
                }
            }
        }
    }

    /// Renders a single anti-aliased glyph from an AaFace.
    pub fn draw_aa_char(&mut self, x: usize, y: usize, face: &AaFace, ch: char, fg: Color, bg: Option<Color>) -> usize {
        let g = face.glyph(ch);
        let gw = g.width as usize;
        let gh = g.height as usize;
        let base = g.offset as usize;

        for row in 0..gh {
            let cy = y + row;
            if cy >= self.height {
                break;
            }
            for col in 0..gw {
                let cx = x + col;
                if cx >= self.width {
                    break;
                }
                let alpha = face.cov[base + row * gw + col];
                if let Some(bg_col) = bg {
                    let blended = Self::blend(bg_col, fg, alpha);
                    self.set_pixel(cx, cy, blended);
                } else if alpha > 0 {
                    self.blend_pixel(cx, cy, fg, alpha);
                }
            }
        }
        g.advance as usize
    }

    /// Renders an anti-aliased string using Helvetica Neue / Menlo.
    pub fn draw_aa_text(&mut self, x: usize, y: usize, face: &AaFace, text: &str, fg: Color, bg: Option<Color>) {
        let mut cur_x = x;
        let mut cur_y = y;
        for ch in text.chars() {
            if ch == '\n' {
                cur_x = x;
                cur_y += face.line_height;
                continue;
            }
            cur_x += self.draw_aa_char(cur_x, cur_y, face, ch, fg, bg);
        }
    }

    /// Renders the crisp anti-aliased "Zenkai" wordmark centered horizontally at `top_y`.
    pub fn draw_wordmark(&mut self, top_y: usize, fg: Color) {
        let start_x = self.width.saturating_sub(WORDMARK_W) / 2;
        for row in 0..WORDMARK_H {
            for col in 0..WORDMARK_W {
                let alpha = WORDMARK_COV[row * WORDMARK_W + col];
                if alpha > 0 {
                    self.blend_pixel(start_x + col, top_y + row, fg, alpha);
                }
            }
        }
    }

    /// Scrolls a bounded rectangular region upward by `pixel_rows`.
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
