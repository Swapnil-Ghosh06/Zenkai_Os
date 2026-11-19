//! Zenkai OS - UEFI Graphics Output Protocol (GOP) & Double-Buffered Compositor
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Smooth 32-bit RAM-backed double-buffered framebuffer with cached atmospheric sky/clouds
//! wallpaper, real alpha-blended frosted glass, anti-aliased typography, and tear-free present().

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};

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

    // Light Editorial Paper Window Surfaces
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

    // Compatibility aliases for console & shell macros (high-contrast charcoal on paper surface!)
    pub const BG_OBSIDIAN: Self = Self::SURFACE;
    pub const BG_PANEL: Self = Self::SURFACE_ALT;
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
    gop_handle: Option<uefi::Handle>,
    back: Vec<u32>,
    wallpaper: Vec<u32>,
    buffered: bool,
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    pub unsafe fn from_raw(
        base: *mut u32,
        width: usize,
        height: usize,
        stride: usize,
        is_bgr: bool,
        gop_handle: Option<uefi::Handle>,
    ) -> Self {
        let pixel_count = width.saturating_mul(height);
        Self {
            base,
            width,
            height,
            stride,
            is_bgr,
            gop_handle,
            back: vec![0u32; pixel_count],
            wallpaper: Vec::new(),
            buffered: false,
        }
    }

    /// Starts a tear-free double-buffered frame: all drawing stays in fast RAM until `present()`.
    #[inline]
    pub fn begin_frame(&mut self) {
        self.buffered = true;
    }

    #[inline]
    pub fn is_buffered(&self) -> bool {
        self.buffered
    }

    /// Blits the entire completed RAM backbuffer to UEFI GOP using `GraphicsOutput::blt`
    /// (firmware-native, pitch-safe on VirtualBox ARM64/VMSVGA) with row-by-row MMIO fallback.
    pub fn present(&mut self) {
        self.buffered = false;
        let w = self.width;
        let h = self.height;
        if w == 0 || h == 0 || self.back.len() < w * h {
            return;
        }
        if self.is_bgr {
            if let Some(handle) = self.gop_handle {
                let gop_opt = unsafe {
                    uefi::boot::open_protocol::<GraphicsOutput>(
                        OpenProtocolParams {
                            handle,
                            agent: uefi::boot::image_handle(),
                            controller: None,
                        },
                        OpenProtocolAttributes::GetProtocol,
                    )
                    .ok()
                };
                if let Some(mut gop) = gop_opt {
                    let blt_slice = unsafe {
                        core::slice::from_raw_parts(self.back.as_ptr() as *const BltPixel, w * h)
                    };
                    if gop
                        .blt(BltOp::BufferToVideo {
                            buffer: blt_slice,
                            src: BltRegion::Full,
                            dest: (0, 0),
                            dims: (w, h),
                        })
                        .is_ok()
                    {
                        return;
                    }
                }
            }
        }
        unsafe {
            for y in 0..h {
                let src = self.back.as_ptr().add(y * w);
                let dst = self.base.add(y * self.stride);
                for x in 0..w {
                    dst.add(x).write_volatile(*src.add(x));
                }
            }
        }
    }

    /// Blits only a dirty rectangle `(x, y, rw, rh)` from the RAM backbuffer to VRAM.
    pub fn present_rect(&mut self, x: usize, y: usize, rw: usize, rh: usize) {
        self.buffered = false;
        let x0 = x.min(self.width);
        let y0 = y.min(self.height);
        let x1 = (x + rw).min(self.width);
        let y1 = (y + rh).min(self.height);
        if x0 >= x1 || y0 >= y1 || self.back.len() < self.width * self.height {
            return;
        }
        let copy_w = x1 - x0;
        let copy_h = y1 - y0;
        let w = self.width;
        if self.is_bgr {
            if let Some(handle) = self.gop_handle {
                let gop_opt = unsafe {
                    uefi::boot::open_protocol::<GraphicsOutput>(
                        OpenProtocolParams {
                            handle,
                            agent: uefi::boot::image_handle(),
                            controller: None,
                        },
                        OpenProtocolAttributes::GetProtocol,
                    )
                    .ok()
                };
                if let Some(mut gop) = gop_opt {
                    let blt_slice = unsafe {
                        core::slice::from_raw_parts(self.back.as_ptr() as *const BltPixel, w * self.height)
                    };
                    if gop
                        .blt(BltOp::BufferToVideo {
                            buffer: blt_slice,
                            src: BltRegion::SubRectangle {
                                coords: (x0, y0),
                                px_stride: w,
                            },
                            dest: (x0, y0),
                            dims: (copy_w, copy_h),
                        })
                        .is_ok()
                    {
                        return;
                    }
                }
            }
        }
        unsafe {
            for cy in y0..y1 {
                let src = self.back.as_ptr().add(cy * w + x0);
                let dst = self.base.add(cy * self.stride + x0);
                for cx in 0..copy_w {
                    dst.add(cx).write_volatile(*src.add(cx));
                }
            }
        }
    }

    #[inline(always)]
    pub fn pack_color(&self, c: Color) -> u32 {
        if self.is_bgr {
            ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32)
        } else {
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
            let idx = y * self.width + x;
            unsafe { *self.back.get_unchecked(idx) }
        } else {
            0
        }
    }

    #[inline(always)]
    pub fn write_raw_pixel(&mut self, x: usize, y: usize, raw: u32) {
        if x < self.width && y < self.height {
            let idx = y * self.width + x;
            unsafe {
                *self.back.get_unchecked_mut(idx) = raw;
                if !self.buffered {
                    self.base.add(y * self.stride + x).write_volatile(raw);
                }
            }
        }
    }

    /// Draws a macOS-style crisp card with rounded corners and subtle 1px hairline border.
    pub fn draw_glass_panel(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, fill: Color, border: Color) {
        self.draw_rounded_rect(x, y, w, h, r, fill, Some(border));
    }

    /// Alpha-blends a filled rectangle over existing RAM backbuffer pixels (blazing fast).
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
        let width = self.width;
        let stride = self.stride;
        let is_bgr = self.is_bgr;
        let buffered = self.buffered;

        for cy in y..y_end {
            let back_row = cy * width + x;
            let count = x_end - x;
            for i in 0..count {
                let raw = unsafe { *self.back.get_unchecked(back_row + i) };
                let (br, bg, bb) = if is_bgr {
                    (((raw >> 16) & 0xFF) as u16, ((raw >> 8) & 0xFF) as u16, (raw & 0xFF) as u16)
                } else {
                    ((raw & 0xFF) as u16, ((raw >> 8) & 0xFF) as u16, ((raw >> 16) & 0xFF) as u16)
                };
                let nr = ((br * inv + fg_r) / 255) as u32;
                let ng = ((bg * inv + fg_g) / 255) as u32;
                let nb = ((bb * inv + fg_b) / 255) as u32;
                let packed = if is_bgr {
                    (nr << 16) | (ng << 8) | nb
                } else {
                    (nb << 16) | (ng << 8) | nr
                };
                unsafe {
                    *self.back.get_unchecked_mut(back_row + i) = packed;
                }
            }
            if !buffered {
                unsafe {
                    let src = self.back.as_ptr().add(back_row);
                    let dst = self.base.add(cy * stride + x);
                    for i in 0..count {
                        dst.add(i).write_volatile(*src.add(i));
                    }
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

        self.fill_rect_alpha(x + 1, y + r, w.saturating_sub(2), h - 2 * r, tint, fill_alpha);
        self.fill_rect_alpha(x + r, y + 1, w - 2 * r, r.saturating_sub(1), tint, fill_alpha);
        self.fill_rect_alpha(x + r, y + h - r, w - 2 * r, r.saturating_sub(1), tint, fill_alpha);

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

    /// Draws a soft feathered cloud puff into a target RAM pixel buffer.
    fn render_cloud_puff_into(
        buf: &mut [u32],
        width: usize,
        height: usize,
        is_bgr: bool,
        cx: isize,
        cy: isize,
        rx: isize,
        ry: isize,
        color: Color,
        peak_alpha: u8,
    ) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let x0 = (cx - rx).max(0) as usize;
        let x1 = (cx + rx).min(width as isize) as usize;
        let y0 = (cy - ry).max(0) as usize;
        let y1 = (cy + ry).min(height as isize) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let rx_sq = (rx * rx) as i64;
        let ry_sq = (ry * ry) as i64;
        let prod = rx_sq * ry_sq;

        for y in y0..y1 {
            let dy = (y as isize - cy) as i64;
            let dy_term = dy * dy * rx_sq;
            let row_start = y * width;
            for x in x0..x1 {
                let dx = (x as isize - cx) as i64;
                let norm = dx * dx * ry_sq + dy_term;
                if norm < prod {
                    let rem = ((prod - norm) * 255) / prod;
                    let soft = (rem * rem) / 255;
                    let alpha = ((soft * peak_alpha as i64) / 255) as u16;
                    if alpha > 2 {
                        let inv = 255 - alpha;
                        let idx = row_start + x;
                        let raw = unsafe { *buf.get_unchecked(idx) };
                        let (br, bg, bb) = if is_bgr {
                            (((raw >> 16) & 0xFF) as u16, ((raw >> 8) & 0xFF) as u16, (raw & 0xFF) as u16)
                        } else {
                            ((raw & 0xFF) as u16, ((raw >> 8) & 0xFF) as u16, ((raw >> 16) & 0xFF) as u16)
                        };
                        let nr = ((br * inv + color.r as u16 * alpha) / 255) as u32;
                        let ng = ((bg * inv + color.g as u16 * alpha) / 255) as u32;
                        let nb = ((bb * inv + color.b as u16 * alpha) / 255) as u32;
                        let packed = if is_bgr {
                            (nr << 16) | (ng << 8) | nb
                        } else {
                            (nb << 16) | (ng << 8) | nr
                        };
                        unsafe {
                            *buf.get_unchecked_mut(idx) = packed;
                        }
                    }
                }
            }
        }
    }

    /// Pre-renders the sky & cumulus clouds wallpaper ONCE into fast RAM, then blits in ~0.3ms.
    pub fn draw_sky_clouds_wallpaper(&mut self) {
        let pixel_count = self.width * self.height;
        if self.wallpaper.len() != pixel_count {
            let mut wp = vec![0u32; pixel_count];
            let w = self.width as isize;
            let h = self.height as isize;
            let mid_y = (self.height * 58) / 100;

            for y in 0..self.height {
                let row_col = if y < mid_y {
                    let a = ((y * 255) / mid_y.max(1)) as u8;
                    Self::blend(Color::BG_TOP, Color::BG_MID, a)
                } else {
                    let a = (((y - mid_y) * 255) / (self.height - mid_y).max(1)) as u8;
                    Self::blend(Color::BG_MID, Color::BG_BOTTOM, a)
                };
                let packed = self.pack_color(row_col);
                let row_start = y * self.width;
                wp[row_start..row_start + self.width].fill(packed);
            }

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
                Self::render_cloud_puff_into(&mut wp, self.width, self.height, self.is_bgr, cx, cy, rx, ry, shadow_col, a);
            }

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
                Self::render_cloud_puff_into(&mut wp, self.width, self.height, self.is_bgr, cx, cy, rx, ry, mid_col, a);
            }

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
                Self::render_cloud_puff_into(&mut wp, self.width, self.height, self.is_bgr, cx, cy, rx, ry, sun_col, a);
            }

            self.wallpaper = wp;
        }

        self.back.copy_from_slice(&self.wallpaper);
        if !self.buffered {
            self.present();
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
            let packed = self.pack_color(color);
            let idx = y * self.width + x;
            unsafe {
                *self.back.get_unchecked_mut(idx) = packed;
                if !self.buffered {
                    self.base.add(y * self.stride + x).write_volatile(packed);
                }
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
        let idx = y * self.width + x;
        let raw_bg = unsafe { *self.back.get_unchecked(idx) };
        let blended = Self::blend(self.unpack_color(raw_bg), fg, alpha);
        let packed = self.pack_color(blended);
        unsafe {
            *self.back.get_unchecked_mut(idx) = packed;
            if !self.buffered {
                self.base.add(y * self.stride + x).write_volatile(packed);
            }
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
            let row_start = y * self.width;
            self.back[row_start..row_start + self.width].fill(packed);
        }
        if !self.buffered {
            self.present();
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if x >= x_end || y >= y_end {
            return;
        }
        let packed = self.pack_color(color);
        let count = x_end - x;
        let width = self.width;
        let stride = self.stride;
        let buffered = self.buffered;

        for cy in y..y_end {
            let row_start = cy * width + x;
            self.back[row_start..row_start + count].fill(packed);
            if !buffered {
                unsafe {
                    let dst = self.base.add(cy * stride + x);
                    for i in 0..count {
                        dst.add(i).write_volatile(packed);
                    }
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

        self.fill_rect(x, y + r, w, h - 2 * r, fill);
        self.fill_rect(x + r, y, w - 2 * r, r, fill);
        self.fill_rect(x + r, y + h - r, w - 2 * r, r, fill);

        if let Some(b) = border {
            self.fill_rect(x + r, y, w - 2 * r, 1, b);
            self.fill_rect(x + r, y + h - 1, w - 2 * r, 1, b);
            self.fill_rect(x, y + r, 1, h - 2 * r, b);
            self.fill_rect(x + w - 1, y + r, 1, h - 2 * r, b);
        }

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
        for layer in (1..=5).rev() {
            let alpha = (6 - layer) as u8 * 8;
            let spread = layer * 2;
            let sx = x.saturating_sub(spread);
            let sy = y + layer;
            let sw = w + spread * 2;
            let sh = h + spread;
            let x_end = (sx + sw).min(self.width);
            let y_end = (sy + sh).min(self.height);
            let bot_y0 = (y + h.saturating_sub(r / 2)).min(y_end);
            self.fill_rect_alpha(sx + r / 2, bot_y0, x_end.saturating_sub(sx + r), y_end.saturating_sub(bot_y0), Color::SHADOW, alpha);
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

    /// Renders a single-line anti-aliased string strictly clipped to `max_w` pixels,
    /// appending `..` if truncated so text never overflows any card or window border.
    pub fn draw_aa_text_clipped(
        &mut self,
        x: usize,
        y: usize,
        max_w: usize,
        face: &AaFace,
        text: &str,
        fg: Color,
        bg: Option<Color>,
    ) {
        if max_w < 12 {
            return;
        }
        let full_w = face.text_width(text);
        if full_w <= max_w {
            self.draw_aa_text(x, y, face, text, fg, bg);
            return;
        }
        let ell_w = face.text_width("..");
        let limit = max_w.saturating_sub(ell_w);
        let mut cur_x = x;
        let mut used = 0usize;
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            let adv = face.glyph(ch).advance as usize;
            if used + adv > limit {
                break;
            }
            cur_x += self.draw_aa_char(cur_x, y, face, ch, fg, bg);
            used += adv;
        }
        self.draw_aa_text(cur_x, y, face, "..", fg, bg);
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

    /// Scrolls a bounded rectangular region upward by `pixel_rows` using fast RAM copy.
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
        let width = self.width;
        let packed_fill = self.pack_color(fill_color);

        for row in 0..rows_to_copy {
            let dst_off = (ry + row) * width + rx;
            let src_off = (ry + row + pixel_rows) * width + rx;
            unsafe {
                core::ptr::copy(self.back.as_ptr().add(src_off), self.back.as_mut_ptr().add(dst_off), actual_w);
            }
        }
        for row in rows_to_copy..actual_h {
            let dst_off = (ry + row) * width + rx;
            self.back[dst_off..dst_off + actual_w].fill(packed_fill);
        }

        if !self.buffered {
            self.present_rect(rx, ry, actual_w, actual_h);
        }
    }
}
