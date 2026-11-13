//! Zenkai OS - 2D Vector Primitives, Banners & Visual Progress Bar
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Custom geometric glyph generator and modern cybernetic UI elements.

use crate::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::gop::{Color, Framebuffer};

/// 5x7 geometric block definitions for "ZENKAI OS"
/// 1 = filled block, 0 = empty
const GLYPH_Z: [u8; 7] = [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f]; // Z
const GLYPH_E: [u8; 7] = [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f]; // E
const GLYPH_N: [u8; 7] = [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]; // N
const GLYPH_K: [u8; 7] = [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]; // K
const GLYPH_A: [u8; 7] = [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11]; // A
const GLYPH_I: [u8; 7] = [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f]; // I
const GLYPH_SPACE: [u8; 7] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // space
const GLYPH_O: [u8; 7] = [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e]; // O
const GLYPH_S: [u8; 7] = [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e]; // S

pub fn get_block_glyph(ch: char) -> &'static [u8; 7] {
    match ch {
        'Z' => &GLYPH_Z,
        'E' => &GLYPH_E,
        'N' => &GLYPH_N,
        'K' => &GLYPH_K,
        'A' => &GLYPH_A,
        'I' => &GLYPH_I,
        ' ' => &GLYPH_SPACE,
        'O' => &GLYPH_O,
        'S' => &GLYPH_S,
        _ => &GLYPH_SPACE,
    }
}

/// Renders a large, sharp geometric title banner for "ZENKAI OS" centered horizontally.
pub fn draw_zenkai_banner(fb: &mut Framebuffer, top_y: usize) {
    let text = "ZENKAI OS";
    let block_size = 7usize;
    let glyph_w = 5 * block_size;
    let _glyph_h = 7 * block_size;
    let tracking = 14usize;

    let total_width = text.len() * glyph_w + (text.len() - 1) * tracking;
    let start_x = fb.width.saturating_sub(total_width) / 2;

    let mut cur_x = start_x;
    for ch in text.chars() {
        let pattern = get_block_glyph(ch);
        for row in 0..7 {
            let row_bits = pattern[row];
            for col in 0..5 {
                if (row_bits & (0x10 >> col)) != 0 {
                    let bx = cur_x + col * block_size;
                    let by = top_y + row * block_size;
                    // Render main cyan block with white core highlight
                    fb.fill_rect(bx, by, block_size, block_size, Color::CYAN);
                    if block_size >= 4 {
                        fb.fill_rect(bx + 1, by + 1, block_size - 2, block_size - 2, Color::WHITE);
                    }
                }
            }
        }
        cur_x += glyph_w + tracking;
    }
}

/// Draws author attribution badge centered on the screen.
pub fn draw_author_badge(fb: &mut Framebuffer, top_y: usize) {
    let author_text = "Author: Swapnil Ghosh | Roll: 25BTRCL267 | Class: AIML-E";
    let sub_text = "Autonomous AI/ML Micro-Kernel & R19 Paging Engine";

    let text_w = author_text.len() * FONT_WIDTH;
    let x = fb.width.saturating_sub(text_w) / 2;

    // Draw dark pill box background
    let pad_x = 24;
    let pad_y = 6;
    let box_x = x.saturating_sub(pad_x);
    let box_y = top_y.saturating_sub(pad_y);
    let box_w = text_w + pad_x * 2;
    let box_h = FONT_HEIGHT * 2 + pad_y * 2 + 6;

    fb.fill_rect(box_x, box_y, box_w, box_h, Color::BG_PANEL);
    fb.draw_border(box_x, box_y, box_w, box_h, 1, Color::DARK_SLATE);

    fb.draw_text(x, top_y, author_text, Color::EMERALD, None, 1);

    let sub_w = sub_text.len() * FONT_WIDTH;
    let sub_x = fb.width.saturating_sub(sub_w) / 2;
    fb.draw_text(sub_x, top_y + FONT_HEIGHT + 4, sub_text, Color::SLATE, None, 1);
}

/// Renders a modern, animated progress bar with card container and diagnostic labels.
pub fn draw_progress_card(
    fb: &mut Framebuffer,
    card_x: usize,
    card_y: usize,
    card_w: usize,
    card_h: usize,
    pct: usize,
    stage_name: &str,
    status_detail: &str,
) {
    // 1. Draw outer card
    fb.fill_rect(card_x, card_y, card_w, card_h, Color::BG_PANEL);
    fb.draw_border(card_x, card_y, card_w, card_h, 1, Color::DARK_SLATE);

    // 2. Stage header text
    let text_x = card_x + 20;
    let text_y = card_y + 16;
    fb.draw_text(text_x, text_y, "BOOT INITIALIZATION SEQUENCE", Color::SLATE, None, 1);

    // Right-aligned percentage string
    let pct_clamped = pct.min(100);
    let mut pct_buf = [0u8; 8];
    let pct_str = format_pct(pct_clamped, &mut pct_buf);
    let pct_x = card_x + card_w - 20 - (pct_str.len() * FONT_WIDTH);
    fb.draw_text(pct_x, text_y, pct_str, Color::CYAN, None, 1);

    // 3. Progress bar track
    let track_x = card_x + 20;
    let track_y = card_y + 42;
    let track_w = card_w - 40;
    let track_h = 16;

    fb.fill_rect(track_x, track_y, track_w, track_h, Color::BG_CARD);
    fb.draw_border(track_x, track_y, track_w, track_h, 1, Color::DARK_SLATE);

    // 4. Progress bar fill
    let fill_w = (track_w * pct_clamped) / 100;
    if fill_w > 0 {
        fb.fill_rect(track_x + 1, track_y + 1, fill_w.saturating_sub(2), track_h - 2, Color::EMERALD);
        // Highlight line across top of fill
        fb.fill_rect(track_x + 1, track_y + 1, fill_w.saturating_sub(2), 2, Color::WHITE);
    }

    // 5. Active Subsystem Stage label
    let stage_y = card_y + 70;
    fb.draw_text(text_x, stage_y, "Active Stage:", Color::SLATE, None, 1);
    fb.draw_text(text_x + 14 * FONT_WIDTH, stage_y, stage_name, Color::WHITE, None, 1);

    // 6. Subsystem status details
    let detail_y = card_y + 92;
    fb.draw_text(text_x, detail_y, "Diagnostics :", Color::SLATE, None, 1);
    fb.draw_text(text_x + 14 * FONT_WIDTH, detail_y, status_detail, Color::CYAN, None, 1);
}

fn format_pct(pct: usize, buf: &mut [u8; 8]) -> &str {
    if pct >= 100 {
        buf[0] = b'1';
        buf[1] = b'0';
        buf[2] = b'0';
        buf[3] = b'%';
        core::str::from_utf8(&buf[..4]).unwrap_or("100%")
    } else if pct >= 10 {
        buf[0] = b'0' + (pct / 10) as u8;
        buf[1] = b'0' + (pct % 10) as u8;
        buf[2] = b'%';
        core::str::from_utf8(&buf[..3]).unwrap_or("0%")
    } else {
        buf[0] = b'0' + pct as u8;
        buf[1] = b'%';
        core::str::from_utf8(&buf[..2]).unwrap_or("0%")
    }
}
