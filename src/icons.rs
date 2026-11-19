//! Zenkai OS - Vector Application & Filesystem Icon Renderer
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Renders crisp, tactile editorial macOS-style squircle icons.

use crate::font_aa::UI_SANS;
use crate::gop::{Color, Framebuffer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppIcon {
    FileSystem,
    HomeFolder,
    FileManager,
    Terminal,
    Software,
    Profiles,
    Network,
    Monitor,
    OsLab,
    Snake,
    Notes,
    Folder,
    FileText,
    FileCode,
    FileDesktop,
}

pub fn draw_zenkai_badge(fb: &mut Framebuffer, x: usize, y: usize) {
    fb.draw_rounded_rect(x, y, 18, 18, 5, Color::GLASS_TEXT, None);
    fb.fill_rect(x + 4, y + 4, 10, 2, Color::INK_SOLID);
    fb.fill_rect(x + 10, y + 6, 3, 2, Color::INK_SOLID);
    fb.fill_rect(x + 7, y + 8, 4, 2, Color::INK_SOLID);
    fb.fill_rect(x + 5, y + 10, 3, 2, Color::INK_SOLID);
    fb.fill_rect(x + 4, y + 12, 10, 2, Color::INK_SOLID);
}

pub fn draw_icon(fb: &mut Framebuffer, x: usize, y: usize, size: usize, icon: AppIcon) {
    let s = size.max(16);
    let r = (s / 4).max(4);
    let tile_bg = Color::new(252, 252, 250);
    let tile_brd = Color::new(212, 210, 203);

    match icon {
        AppIcon::FileSystem => {
            // Brushed aluminum macOS-style drive icon
            let drive_top = Color::new(236, 239, 244);
            let drive_face = Color::new(206, 212, 222);
            let drive_brd = Color::new(164, 173, 188);
            fb.draw_rounded_rect(x, y + s / 6, s, (s * 4) / 6, r, drive_top, Some(drive_brd));
            fb.draw_rounded_rect(x + 2, y + (s * 4) / 9, s.saturating_sub(4), s / 3, 4, drive_face, None);
            fb.draw_rounded_rect(x + 6, y + (s * 5) / 8, s / 4, 3, 1, Color::new(110, 120, 138), None);
            fb.draw_rounded_rect(x + s.saturating_sub(12), y + (s * 5) / 8, 5, 4, 2, Color::new(34, 197, 94), None);
        }
        AppIcon::HomeFolder | AppIcon::Folder => {
            let tab_w = (s * 44) / 100;
            let body_col = if icon == AppIcon::HomeFolder {
                Color::new(88, 152, 240)
            } else {
                Color::new(100, 164, 246)
            };
            let tab_col = Color::new(68, 126, 210);
            fb.draw_rounded_rect(x + 1, y + s / 8, tab_w, s / 3, 4, tab_col, None);
            fb.draw_rounded_rect(x, y + s / 4, s, (s * 3) / 4 - 2, r.min(9), body_col, Some(tab_col));
            fb.fill_rect(x + 4, y + s / 4 + 3, s.saturating_sub(8), 2, Color::new(166, 208, 255));
            if icon == AppIcon::HomeFolder && s >= 32 {
                let cx = x + s / 2;
                let cy = y + (s * 58) / 100;
                let h_col = Color::new(240, 247, 255);
                fb.fill_rect(cx - 2, cy - 6, 4, 2, h_col);
                fb.fill_rect(cx - 5, cy - 4, 10, 2, h_col);
                fb.fill_rect(cx - 7, cy - 2, 14, 2, h_col);
                fb.fill_rect(cx - 5, cy, 10, 7, h_col);
                fb.fill_rect(cx - 2, cy + 3, 4, 4, body_col);
            }
        }
        AppIcon::FileManager => {
            // Warm honey-sand archive folder
            let tab_w = (s * 44) / 100;
            let body_col = Color::new(236, 184, 72);
            let tab_col = Color::new(204, 150, 44);
            fb.draw_rounded_rect(x + 1, y + s / 8, tab_w, s / 3, 4, tab_col, None);
            fb.draw_rounded_rect(x, y + s / 4, s, (s * 3) / 4 - 2, r.min(9), body_col, Some(tab_col));
            fb.fill_rect(x + 4, y + s / 4 + 3, s.saturating_sub(8), 2, Color::new(252, 226, 158));
            if s >= 32 {
                fb.draw_rounded_rect(x + s / 3, y + (s * 56) / 100, s / 3, 4, 2, Color::new(138, 96, 22), None);
            }
        }
        AppIcon::Terminal => {
            // Matte obsidian terminal squircle
            fb.draw_rounded_rect(x, y, s, s, r, Color::new(26, 28, 34), Some(Color::new(62, 66, 78)));
            fb.fill_rect(x + 5, y + 5, s.saturating_sub(10), 2, Color::new(56, 60, 72));
            if s >= 34 {
                fb.draw_aa_text(x + 7, y + s / 4, &UI_SANS, ">_", Color::GLASS_TEXT, None);
            } else {
                fb.fill_rect(x + 4, y + s / 2 - 2, 5, 2, Color::GLASS_TEXT);
                fb.fill_rect(x + 10, y + s / 2 + 1, 5, 2, Color::GLASS_TEXT);
            }
        }
        AppIcon::Software => {
            // Crisp white paper squircle with 4 minimal cobalt app blocks
            fb.draw_rounded_rect(x, y, s, s, r, tile_bg, Some(tile_brd));
            let c = (s / 4).max(4);
            let gx = x + s / 4 - 1;
            let gy = y + s / 4 - 1;
            fb.draw_rounded_rect(gx, gy, c, c, 3, Color::ACCENT, None);
            fb.draw_rounded_rect(gx + c + 3, gy, c, c, 3, Color::new(96, 148, 245), None);
            fb.draw_rounded_rect(gx, gy + c + 3, c, c, 3, Color::new(96, 148, 245), None);
            fb.draw_rounded_rect(gx + c + 3, gy + c + 3, c, c, 3, Color::INK_SOLID, None);
        }
        AppIcon::Profiles => {
            // Crisp white squircle with minimal indigo environment nodes
            fb.draw_rounded_rect(x, y, s, s, r, tile_bg, Some(tile_brd));
            let mid_y = y + (s * 58) / 100;
            fb.fill_rect(x + s / 4, mid_y, s / 2, 2, Color::new(165, 170, 184));
            fb.fill_rect(x + s / 2 - 1, y + s / 3, 2, s / 4, Color::new(165, 170, 184));
            fb.draw_rounded_rect(x + s / 4 - 3, mid_y - 3, 7, 7, 3, Color::INK_SOLID, None);
            fb.draw_rounded_rect(x + (s * 3) / 4 - 4, mid_y - 3, 7, 7, 3, Color::ACCENT, None);
            fb.draw_rounded_rect(x + s / 2 - 4, y + s / 4 - 1, 8, 8, 4, Color::VIOLET, None);
        }
        AppIcon::Network => {
            // Crisp white squircle with clean ascending signal bars
            fb.draw_rounded_rect(x, y, s, s, r, tile_bg, Some(tile_brd));
            let bw = (s / 7).max(3);
            let base_y = y + (s * 3) / 4;
            let gap = bw + 3;
            let start_x = x + s.saturating_sub(bw * 3 + 6) / 2;
            fb.draw_rounded_rect(start_x, base_y - s / 5, bw, s / 5, 1, Color::INK_SOLID, None);
            fb.draw_rounded_rect(start_x + gap, base_y - (s * 3) / 8, bw, (s * 3) / 8, 1, Color::INK_SOLID, None);
            fb.draw_rounded_rect(start_x + gap * 2, base_y - (s * 5) / 9, bw, (s * 5) / 9, 1, Color::ACCENT, None);
        }
        AppIcon::Monitor => {
            // Crisp white squircle with editorial telemetry bars
            fb.draw_rounded_rect(x, y, s, s, r, tile_bg, Some(tile_brd));
            let base_y = y + (s * 4) / 5;
            let bw = (s / 6).max(3);
            let bx = x + s / 4;
            fb.draw_rounded_rect(bx, base_y - s / 4, bw, s / 4, 2, Color::new(180, 184, 194), None);
            fb.draw_rounded_rect(bx + bw + 3, base_y - (s * 4) / 9, bw, (s * 4) / 9, 2, Color::ACCENT, None);
            fb.draw_rounded_rect(bx + (bw + 3) * 2, base_y - s / 2, bw, s / 2, 2, Color::INK_SOLID, None);
        }
        AppIcon::OsLab => {
            // Crisp white squircle with minimal 2x2 ring-buffer cells
            fb.draw_rounded_rect(x, y, s, s, r, tile_bg, Some(tile_brd));
            let c = (s / 4).max(4);
            let gx = x + s / 4 - 2;
            let gy = y + s / 4 - 2;
            fb.draw_rounded_rect(gx, gy, c, c, 2, Color::INK_SOLID, None);
            fb.draw_rounded_rect(gx + c + 4, gy, c, c, 2, Color::SAGE, None);
            fb.draw_rounded_rect(gx, gy + c + 4, c, c, 2, Color::ACCENT, None);
            fb.draw_rounded_rect(gx + c + 4, gy + c + 4, c, c, 2, Color::SURFACE_CARD, Some(tile_brd));
        }
        AppIcon::Snake => {
            // Matte forest-slate arcade tile
            fb.draw_rounded_rect(x, y, s, s, r, Color::new(28, 46, 38), Some(Color::new(56, 86, 72)));
            let seg = (s / 6).max(4);
            fb.draw_rounded_rect(x + s / 5, y + (s * 3) / 5, seg, seg, 2, Color::new(74, 222, 128), None);
            fb.draw_rounded_rect(x + s / 5 + seg + 1, y + (s * 3) / 5, seg, seg, 2, Color::new(74, 222, 128), None);
            fb.draw_rounded_rect(x + s / 5 + seg + 1, y + (s * 3) / 5 - seg - 1, seg, seg, 2, Color::new(134, 239, 172), None);
            fb.draw_rounded_rect(x + (s * 2) / 3, y + s / 4, seg, seg, seg / 2, Color::TRAFFIC_RED, None);
        }
        AppIcon::Notes => {
            // Classic warm paper sticky note icon (inspired by Sticky Notes in reference)
            fb.draw_rounded_rect(x, y, s, s, r, Color::new(255, 253, 245), Some(tile_brd));
            fb.draw_rounded_rect(x + 2, y + 2, s.saturating_sub(4), (s / 4).max(5), 4, Color::new(245, 197, 66), None);
            let lx = x + 7;
            let lw = s.saturating_sub(14);
            fb.fill_rect(lx, y + s / 2 - 2, lw, 2, Color::new(82, 82, 91));
            fb.fill_rect(lx, y + s / 2 + 4, (lw * 3) / 4, 2, Color::new(161, 161, 170));
            fb.fill_rect(lx, y + s / 2 + 10, lw / 2, 2, Color::new(196, 196, 202));
        }
        AppIcon::FileText | AppIcon::FileCode | AppIcon::FileDesktop => {
            let pad_x = s / 8;
            let w = s.saturating_sub(pad_x * 2);
            let accent = match icon {
                AppIcon::FileCode => Color::VIOLET,
                AppIcon::FileDesktop => Color::SAGE,
                _ => Color::ACCENT,
            };
            fb.draw_rounded_rect(x + pad_x, y + 2, w, s.saturating_sub(4), 5, tile_bg, Some(tile_brd));
            fb.fill_rect(x + pad_x + 4, y + 8, w.saturating_sub(10), 3, accent);
            fb.fill_rect(x + pad_x + 4, y + s / 2 - 2, w.saturating_sub(8), 2, Color::TEXT_MUTED);
            fb.fill_rect(x + pad_x + 4, y + s / 2 + 4, (w * 2) / 3, 2, Color::TEXT_DIM);
        }
    }
}
