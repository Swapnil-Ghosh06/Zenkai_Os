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

    // Editorial Paper & Sky Palette (Inspired by Cloud Desktop / Light Paper UI)
    // Sky & Atmospheric Backdrop
    pub const BG_TOP: Self = Self::new(58, 90, 140);         // #3A5A8C Deep Cerulean Sky
    pub const BG_MID: Self = Self::new(92, 128, 178);        // #5C80B2 Soft Mid Sky
    pub const BG_BOTTOM: Self = Self::new(150, 178, 208);    // #96B2D0 Horizon Mist

    // Light Editorial Paper Window Surfaces (matching crisp white + warm linen rail)
    pub const SURFACE: Self = Self::new(255, 255, 255);      // #FFFFFF Pure Paper Window Body
    pub const SURFACE_ALT: Self = Self::new(246, 245, 241);  // #F6F5F1 Warm Linen Titlebar / Rail
    pub const SURFACE_CARD: Self = Self::new(241, 240, 235); // #F1F0EB Soft Stone Card
    pub const SURFACE_HOVER: Self = Self::new(230, 228, 221);// #E6E4DD Hover / Selected Row
    pub const BORDER: Self = Self::new(222, 220, 213);       // #DEDCD5 Crisp Hairline
    pub const SHADOW: Self = Self::new(15, 23, 42);          // #0F172A Soft Slate Shadow

    // High-Contrast Editorial Ink on Paper
    pub const TEXT_PRIMARY: Self = Self::new(24, 24, 27);    // #18181B Matte Charcoal Ink
    pub const TEXT_MUTED: Self = Self::new(92, 96, 108);     // #5C606C Neutral Slate
    pub const TEXT_DIM: Self = Self::new(146, 150, 160);     // #9296A0 Subtle Caption

    // Primary Obsidian CTA & Frosted Dark Glass Tokens (for Desktop Chrome over Sky)
    pub const INK_SOLID: Self = Self::new(24, 24, 27);       // #18181B Solid Matte Black Button
    pub const GLASS_DARK: Self = Self::new(25, 38, 58);      // #19263A Frosted Slate Glass
    pub const GLASS_ELEVATED: Self = Self::new(40, 56, 82);  // #283852 Elevated Frosted Glass
    pub const GLASS_TEXT: Self = Self::new(248, 250, 252);   // #F8FAFC Crisp White on Glass
    pub const GLASS_MUTED: Self = Self::new(203, 213, 225);  // #CBD5E1 Soft Silver on Glass
    pub const GLASS_BORDER: Self = Self::new(255, 255, 255); // Luminous Hairline (alpha-blended)

    // Tasteful Muted Editorial Accents (high contrast on white paper, never neon)
    pub const ACCENT: Self = Self::new(37, 99, 235);         // #2563EB Editorial Cobalt
    pub const VIOLET: Self = Self::new(109, 40, 217);        // #6D28D9 Deep Indigo-Violet
    pub const SAGE: Self = Self::new(21, 128, 61);           // #15803D Forest Emerald
    pub const AMBER: Self = Self::new(180, 83, 9);           // #B45309 Warm Ochre
    pub const CORAL: Self = Self::new(220, 38, 38);          // #DC2626 Editorial Crimson

    // macOS Traffic-Light Window Controls
    pub const TRAFFIC_RED: Self = Self::new(255, 95, 86);
    pub const TRAFFIC_YELLOW: Self = Self::new(255, 189, 46);
    pub const TRAFFIC_GREEN: Self = Self::new(39, 201, 63);

    // Compatibility aliases for console macros
    pub const BG_OBSIDIAN: Self = Self::SURFACE;
    pub const BG_PANEL: Self = Self::SURFACE_ALT;
    pub const WHITE: Self = Self::GLASS_TEXT;
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
    pub fn read_raw_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            let offset = y * self.stride + x;
            unsafe { self.base.add(offset).read_volatile() }
        } else {
            0
        }
    }

    #[inline(always)]
    pub fn write_raw_pixel(&mut self, x: usize, y: usize, raw: u32) {
        if x < self.width && y < self.height {
            let offset = y * self.stride + x;
            unsafe { self.base.add(offset).write_volatile(raw) }
        }
    }

    /// Draws a macOS-style crisp card with rounded corners and subtle 1px hairline border.
    pub fn draw_glass_panel(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, fill: Color, border: Color) {
        self.draw_rounded_rect(x, y, w, h, r, fill, Some(border));
    }

    /// Alpha-blends a filled rectangle over existing framebuffer pixels.
    pub fn fill_rect_alpha(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color, alpha: u8) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if x >= x_end || y >= y_end || alpha == 0 {
            return;
        }
        if alpha == 255 {
            self.fill_rect(x, y, x_end - x, y_end - y, color);
            return;
        }
        let a = alpha as u16;
        let inv = 255 - a;
        let fg_r = color.r as u16 * a;
        let fg_g = color.g as u16 * a;
        let fg_b = color.b as u16 * a;

        for cy in y..y_end {
            let row_start = cy * self.stride + x;
            let count = x_end - x;
            unsafe {
                let ptr = self.base.add(row_start);
                for i in 0..count {
                    let raw = ptr.add(i).read_volatile();
                    let bg = self.unpack_color(raw);
                    let blended = Color::new(
                        ((bg.r as u16 * inv + fg_r) / 255) as u8,
                        ((bg.g as u16 * inv + fg_g) / 255) as u8,
                        ((bg.b as u16 * inv + fg_b) / 255) as u8,
                    );
                    ptr.add(i).write_volatile(self.pack_color(blended));
                }
            }
        }
    }

    /// Draws a true translucent frosted-glass rounded panel alpha-blended over the sky/clouds.
    pub fn draw_frosted_glass(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        r: usize,
        tint: Color,
        fill_alpha: u8,
        border: Option<(Color, u8)>,
    ) {
        if w < r * 2 || h < r * 2 {
            self.fill_rect_alpha(x, y, w, h, tint, fill_alpha);
            return;
        }
        let r_sq = (r * r) as isize;
        let r_inner_sq = ((r.saturating_sub(1)) * (r.saturating_sub(1))) as isize;

        // Middle horizontal band + top/bottom center strips
        self.fill_rect_alpha(x + 1, y + r, w.saturating_sub(2), h - 2 * r, tint, fill_alpha);
        self.fill_rect_alpha(x + r, y + 1, w - 2 * r, r.saturating_sub(1), tint, fill_alpha);
        self.fill_rect_alpha(x + r, y + h - r, w - 2 * r, r.saturating_sub(1), tint, fill_alpha);

        // Straight border edges
        if let Some((b_col, b_alpha)) = border {
            self.fill_rect_alpha(x + r, y, w - 2 * r, 1, b_col, b_alpha);
            self.fill_rect_alpha(x + r, y + h - 1, w - 2 * r, 1, b_col, b_alpha);
            self.fill_rect_alpha(x, y + r, 1, h - 2 * r, b_col, b_alpha);
            self.fill_rect_alpha(x + w - 1, y + r, 1, h - 2 * r, b_col, b_alpha);
        } else {
            self.fill_rect_alpha(x + r, y, w - 2 * r, 1, tint, fill_alpha);
            self.fill_rect_alpha(x + r, y + h - 1, w - 2 * r, 1, tint, fill_alpha);
            self.fill_rect_alpha(x, y + r, 1, h - 2 * r, tint, fill_alpha);
            self.fill_rect_alpha(x + w - 1, y + r, 1, h - 2 * r, tint, fill_alpha);
        }

        // 4 Rounded corners
        for dy in 0..r {
            for dx in 0..r {
                let dist_sq = ((r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy)) as isize;
                if dist_sq <= r_sq {
                    let is_edge = dist_sq >= r_inner_sq;
                    let (col, a) = if is_edge {
                        if let Some((b_col, b_alpha)) = border {
                            (b_col, b_alpha)
                        } else {
                            (tint, fill_alpha)
                        }
                    } else {
                        (tint, fill_alpha)
                    };
                    self.blend_pixel(x + dx, y + dy, col, a);
                    self.blend_pixel(x + w - 1 - dx, y + dy, col, a);
                    self.blend_pixel(x + dx, y + h - 1 - dy, col, a);
                    self.blend_pixel(x + w - 1 - dx, y + h - 1 - dy, col, a);
                }
            }
        }
    }

    /// Draws a soft feathered cloud puff (radial quadratic alpha falloff) for sky wallpaper.
    fn draw_cloud_puff(&mut self, cx: isize, cy: isize, rx: isize, ry: isize, color: Color, peak_alpha: u8) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let x0 = (cx - rx).max(0) as usize;
        let x1 = (cx + rx).min(self.width as isize) as usize;
        let y0 = (cy - ry).max(0) as usize;
        let y1 = (cy + ry).min(self.height as isize) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let rx_sq = (rx * rx) as i64;
        let ry_sq = (ry * ry) as i64;
        let prod = rx_sq * ry_sq;

        for y in y0..y1 {
            let dy = (y as isize - cy) as i64;
            let dy_term = dy * dy * rx_sq;
            let row_start = y * self.stride;
            for x in x0..x1 {
                let dx = (x as isize - cx) as i64;
                let norm = dx * dx * ry_sq + dy_term;
                if norm < prod {
                    // Smooth quadratic falloff: (1 - r^2)^2 gives soft organic cloud edges
                    let rem = ((prod - norm) * 255) / prod;
                    let soft = (rem * rem) / 255;
                    let alpha = ((soft * peak_alpha as i64) / 255) as u8;
                    if alpha > 2 {
                        let offset = row_start + x;
                        let raw_bg = unsafe { self.base.add(offset).read_volatile() };
                        let blended = Self::blend(self.unpack_color(raw_bg), color, alpha);
                        unsafe {
                            self.base.add(offset).write_volatile(self.pack_color(blended));
                        }
                    }
                }
            }
        }
    }

    /// Renders the signature atmospheric cerulean sky + sunlit volumetric cumulus clouds wallpaper.
    pub fn draw_sky_clouds_wallpaper(&mut self) {
        let w = self.width as isize;
        let h = self.height as isize;
        let mid_y = (self.height * 58) / 100;

        // 1. Two-stop smooth cerulean-to-horizon sky gradient
        for y in 0..self.height {
            let row_col = if y < mid_y {
                let a = ((y * 255) / mid_y.max(1)) as u8;
                Self::blend(Color::BG_TOP, Color::BG_MID, a)
            } else {
                let a = (((y - mid_y) * 255) / (self.height - mid_y).max(1)) as u8;
                Self::blend(Color::BG_MID, Color::BG_BOTTOM, a)
            };
            let packed = self.pack_color(row_col);
            let row_start = y * self.stride;
            unsafe {
                let ptr = self.base.add(row_start);
                for x in 0..self.width {
                    ptr.add(x).write_volatile(packed);
                }
            }
        }

        // 2. Back shadow cumulus layer (cool atmospheric silver-blue depth)
        let shadow_col = Color::new(152, 168, 192);
        let back_puffs: [(isize, isize, isize, isize, u8); 9] = [
            (w * 4 / 100, h * 76 / 100, w * 18 / 100, h * 26 / 100, 170),
            (w * 16 / 100, h * 84 / 100, w * 20 / 100, h * 24 / 100, 180),
            (w * 32 / 100, h * 92 / 100, w * 22 / 100, h * 22 / 100, 165),
            (w * 52 / 100, h * 94 / 100, w * 24 / 100, h * 20 / 100, 155),
            (w * 72 / 100, h * 88 / 100, w * 22 / 100, h * 24 / 100, 175),
            (w * 86 / 100, h * 72 / 100, w * 18 / 100, h * 28 / 100, 185),
            (w * 96 / 100, h * 64 / 100, w * 16 / 100, h * 30 / 100, 190),
            (w * 9 / 100, h * 58 / 100, w * 14 / 100, h * 20 / 100, 130),
            (w * 89 / 100, h * 54 / 100, w * 15 / 100, h * 22 / 100, 140),
        ];
        for &(cx, cy, rx, ry, a) in &back_puffs {
            self.draw_cloud_puff(cx, cy, rx, ry, shadow_col, a);
        }

        // 3. Mid volumetric cloud body (soft alabaster-silver)
        let mid_col = Color::new(220, 227, 236);
        let mid_puffs: [(isize, isize, isize, isize, u8); 12] = [
            (w * 2 / 100, h * 66 / 100, w * 13 / 100, h * 19 / 100, 210),
            (w * 11 / 100, h * 76 / 100, w * 15 / 100, h * 22 / 100, 220),
            (w * 22 / 100, h * 86 / 100, w * 16 / 100, h * 20 / 100, 215),
            (w * 37 / 100, h * 93 / 100, w * 18 / 100, h * 18 / 100, 200),
            (w * 54 / 100, h * 95 / 100, w * 19 / 100, h * 17 / 100, 195),
            (w * 69 / 100, h * 90 / 100, w * 17 / 100, h * 20 / 100, 210),
            (w * 80 / 100, h * 80 / 100, w * 16 / 100, h * 22 / 100, 220),
            (w * 90 / 100, h * 68 / 100, w * 15 / 100, h * 23 / 100, 225),
            (w * 97 / 100, h * 58 / 100, w * 13 / 100, h * 21 / 100, 215),
            (w * 6 / 100, h * 56 / 100, w * 10 / 100, h * 15 / 100, 185),
            (w * 86 / 100, h * 56 / 100, w * 11 / 100, h * 16 / 100, 190),
            (w * 94 / 100, h * 84 / 100, w * 16 / 100, h * 22 / 100, 220),
        ];
        for &(cx, cy, rx, ry, a) in &mid_puffs {
            self.draw_cloud_puff(cx, cy, rx, ry, mid_col, a);
        }

        // 4. Bright sunlit cloud crowns (crisp warm white highlights on top-left rims)
        let sun_col = Color::new(247, 249, 252);
        let sun_puffs: [(isize, isize, isize, isize, u8); 9] = [
            (w * 4 / 100, h * 56 / 100, w * 8 / 100, h * 12 / 100, 210),
            (w * 11 / 100, h * 68 / 100, w * 10 / 100, h * 14 / 100, 220),
            (w * 20 / 100, h * 80 / 100, w * 11 / 100, h * 13 / 100, 200),
            (w * 35 / 100, h * 89 / 100, w * 12 / 100, h * 11 / 100, 175),
            (w * 68 / 100, h * 86 / 100, w * 11 / 100, h * 12 / 100, 190),
            (w * 80 / 100, h * 72 / 100, w * 11 / 100, h * 15 / 100, 215),
            (w * 88 / 100, h * 57 / 100, w * 10 / 100, h * 14 / 100, 225),
            (w * 95 / 100, h * 50 / 100, w * 9 / 100, h * 13 / 100, 205),
            (w * 91 / 100, h * 76 / 100, w * 12 / 100, h * 15 / 100, 210),
        ];
        for &(cx, cy, rx, ry, a) in &sun_puffs {
            self.draw_cloud_puff(cx, cy, rx, ry, sun_col, a);
        }
    }

    /// Draws a macOS/iOS pill toggle switch (40x22) at `(x, y)`.
    pub fn draw_toggle_switch(&mut self, x: usize, y: usize, on: bool, active_col: Color) {
        let track_col = if on { active_col } else { Color::BORDER };
        self.draw_rounded_rect(x, y, 40, 22, 11, track_col, None);
        let knob_x = if on { x + 20 } else { x + 2 };
        self.draw_rounded_rect(knob_x, y + 2, 18, 18, 9, Color::SURFACE, None);
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

    /// Draws a soft, diffused macOS-style elevation shadow behind a floating rounded window.
    pub fn draw_shadow(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize) {
        for layer in (1..=6).rev() {
            let alpha = (7 - layer) as u8 * 7;
            let spread = layer * 2;
            let sx = x.saturating_sub(spread);
            let sy = y + layer;
            let sw = w + spread * 2;
            let sh = h + spread;
            let x_end = (sx + sw).min(self.width);
            let y_end = (sy + sh).min(self.height);
            // Bottom diffused shadow strip
            let bot_y0 = (y + h.saturating_sub(r / 2)).min(y_end);
            self.fill_rect_alpha(sx + r / 2, bot_y0, x_end.saturating_sub(sx + r), y_end.saturating_sub(bot_y0), Color::SHADOW, alpha);
            // Subtle side halo strips
            let side_y0 = (y + r).min(bot_y0);
            let side_h = bot_y0.saturating_sub(side_y0);
            if side_h > 0 && x > sx {
                self.fill_rect_alpha(sx, side_y0, x - sx, side_h, Color::SHADOW, alpha / 2);
            }
            if side_h > 0 && x_end > x + w {
                self.fill_rect_alpha(x + w, side_y0, x_end - (x + w), side_h, Color::SHADOW, alpha / 2);
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
                let alpha = face.cov[base + (gh - 1 - row) * gw + col];
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
                let alpha = WORDMARK_COV[(WORDMARK_H - 1 - row) * WORDMARK_W + col];
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
