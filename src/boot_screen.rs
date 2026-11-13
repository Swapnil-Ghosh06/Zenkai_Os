//! Zenkai OS - Minimalist Boot Screen
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Clean Apple/Linear-inspired boot sequence with anti-aliased wordmark and pill progress bar.

use crate::font_aa::{UI_SANS, WORDMARK_H};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::kok;
use crate::timer::{read_tsc, sleep_ms};

pub fn run_visual_boot_sequence(fb: &mut Framebuffer, hw: &mut HardwareInfo) {
    // 1. Soft atmospheric gradient background
    fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);

    // 2. Centered anti-aliased "Zenkai" wordmark
    let wm_y = (fb.height / 2).saturating_sub(WORDMARK_H / 2 + 28);
    fb.draw_wordmark(wm_y, Color::TEXT_PRIMARY);

    // 3. Subtle muted caption
    let sub = "Swapnil Ghosh  \x7f  25BTRCL267";
    let clean_sub = "Swapnil Ghosh  -  25BTRCL267 AIML-E";
    let _ = sub;
    let sub_w = UI_SANS.text_width(clean_sub);
    let sub_x = fb.width.saturating_sub(sub_w) / 2;
    fb.draw_aa_text(sub_x, wm_y + WORDMARK_H + 8, &UI_SANS, clean_sub, Color::TEXT_DIM, None);

    // 4. Sleek thin pill progress bar (240px x 4px)
    let bar_w = 240usize;
    let bar_h = 4usize;
    let bar_x = fb.width.saturating_sub(bar_w) / 2;
    let bar_y = wm_y + WORDMARK_H + 46;

    fb.draw_rounded_rect(bar_x, bar_y, bar_w, bar_h, 2, Color::SURFACE_ALT, None);

    let stages = [
        (20, "GOP Video & Laptop Probe", 180u64),
        (40, "Memory & SpinLock Init", 160),
        (60, "Network (WiFi & LAN)", 190),
        (80, "Aptitude & Snapstore", 170),
        (100, "R19 AI/ML Research Engine", 200),
    ];

    let mut prev_fill = 0usize;
    for &(pct, stage_name, stall_ms) in &stages {
        let stage_tsc = read_tsc();
        let target_fill = (bar_w * pct) / 100;

        // Smooth animation steps between prev_fill and target_fill
        let steps = 6usize;
        for s in 1..=steps {
            let cur_fill = prev_fill + ((target_fill - prev_fill) * s) / steps;
            if cur_fill >= 4 {
                fb.draw_rounded_rect(bar_x, bar_y, cur_fill, bar_h, 2, Color::TEXT_PRIMARY, None);
            }
            sleep_ms((stall_ms / steps as u64) as usize);
        }
        prev_fill = target_fill;

        kok!("STAGE", "[{:>3}%] {}", pct, stage_name);
        hw.record_stage(stage_name, pct, stage_tsc, stall_ms);
    }

    sleep_ms(180);
}
