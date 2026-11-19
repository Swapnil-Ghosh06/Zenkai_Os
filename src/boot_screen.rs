//! Zenkai OS - Interactive Glass-Dark Boot Menu & Visual Boot Sequence
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Controllable via Mouse Cursor (click/hover) and Keyboard (Up/Down/Space/Enter).

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};
use uefi::runtime::{self, ResetType};
use uefi::Status;

use crate::font_aa::{UI_SANS, WORDMARK_H};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::kok;
use crate::mouse::MouseDriver;
use crate::timer::{read_tsc, sleep_ms};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BootAction {
    Normal,
    Recovery,
    FirmwareShell,
}

const MENU_ITEMS: [&str; 6] = [
    "Boot Zenkai OS",
    "Recovery Mode",
    "Firmware Shell",
    "Debug Logging",
    "Restart System",
    "Power Off",
];

const ITEM_H: usize = 40;
const CARD_W: usize = 460;
const CARD_PAD: usize = 16;

fn draw_boot_menu(
    fb: &mut Framebuffer,
    selected: usize,
    debug_on: bool,
    countdown_sec: Option<usize>,
    wm_y: usize,
    card_x: usize,
    card_y: usize,
) {
    fb.draw_sky_clouds_wallpaper();
    fb.draw_wordmark(wm_y, Color::GLASS_TEXT);

    let sub = "Zenkai OS v1.0 Desktop Edition  |  Swapnil Ghosh (25BTRCL267 AIML-E)";
    let sub_w = UI_SANS.text_width(sub);
    let sub_x = fb.width.saturating_sub(sub_w) / 2;
    fb.draw_aa_text(sub_x, wm_y + WORDMARK_H + 6, &UI_SANS, sub, Color::GLASS_TEXT, None);

    let card_h = CARD_PAD * 2 + MENU_ITEMS.len() * ITEM_H + 12;
    fb.draw_shadow(card_x, card_y, CARD_W, card_h, 14);
    fb.draw_glass_panel(card_x, card_y, CARD_W, card_h, 14, Color::SURFACE, Color::BORDER);

    // Subtle macOS traffic-light dots on boot card header
    fb.draw_rounded_rect(card_x + 16, card_y + 12, 10, 10, 5, Color::TRAFFIC_RED, None);
    fb.draw_rounded_rect(card_x + 32, card_y + 12, 10, 10, 5, Color::TRAFFIC_YELLOW, None);
    fb.draw_rounded_rect(card_x + 48, card_y + 12, 10, 10, 5, Color::TRAFFIC_GREEN, None);

    let list_y = card_y + CARD_PAD + 14;
    for (idx, &label) in MENU_ITEMS.iter().enumerate() {
        let ry = list_y + idx * ITEM_H;
        let rx = card_x + 14;
        let rw = CARD_W - 28;
        let is_sel = idx == selected;

        if is_sel {
            fb.draw_rounded_rect(rx, ry, rw, ITEM_H - 4, 9, Color::INK_SOLID, None);
        }

        let text_col = if is_sel { Color::GLASS_TEXT } else { Color::TEXT_PRIMARY };
        fb.draw_aa_text(rx + 16, ry + 7, &UI_SANS, label, text_col, None);

        if idx == 0 {
            let tag = "Default";
            let tw = UI_SANS.text_width(tag);
            let tag_col = if is_sel { Color::GLASS_MUTED } else { Color::TEXT_MUTED };
            fb.draw_aa_text(rx + rw - tw - 16, ry + 7, &UI_SANS, tag, tag_col, None);
        } else if idx == 3 {
            fb.draw_toggle_switch(rx + rw - 56, ry + 7, debug_on, Color::SAGE);
        }
    }

    let hint = match countdown_sec {
        Some(s) => format!("Auto-booting in {}s  (Move mouse or press any key to pause)", s),
        None => String::from("Click or use Up/Down + Enter to select  |  Space toggles Debug"),
    };
    let hw_txt = UI_SANS.text_width(&hint);
    let hx = fb.width.saturating_sub(hw_txt) / 2;
    fb.draw_frosted_glass(hx.saturating_sub(14), card_y + card_h + 14, hw_txt + 28, 28, 9, Color::GLASS_DARK, 165, None);
    fb.draw_aa_text(hx, card_y + card_h + 16, &UI_SANS, &hint, Color::GLASS_TEXT, None);
}

pub fn run_visual_boot_sequence(fb: &mut Framebuffer, hw: &mut HardwareInfo) -> BootAction {
    let wm_y = (fb.height / 2).saturating_sub(WORDMARK_H + 135);
    let card_x = fb.width.saturating_sub(CARD_W) / 2;
    let card_y = wm_y + WORDMARK_H + 46;
    let list_y = card_y + CARD_PAD + 14;

    let mut mouse = MouseDriver::init(fb.width, fb.height);
    let mut selected: usize = 0;
    let mut countdown_ms: Option<usize> = Some(3000);
    let mut last_shown_sec: Option<usize> = Some(3);
    let mut needs_redraw = true;
    let mut action = BootAction::Normal;

    loop {
        if needs_redraw {
            mouse.hide(fb);
            draw_boot_menu(fb, selected, hw.debug_logging, last_shown_sec, wm_y, card_x, card_y);
            mouse.draw(fb);
            needs_redraw = false;
        }

        // Poll keyboard
        let key_evt = uefi::system::with_stdin(|stdin| stdin.read_key().ok().flatten());
        if let Some(key) = key_evt {
            countdown_ms = None;
            last_shown_sec = None;
            match key {
                Key::Special(ScanCode::UP) => {
                    selected = if selected == 0 { MENU_ITEMS.len() - 1 } else { selected - 1 };
                    needs_redraw = true;
                }
                Key::Special(ScanCode::DOWN) => {
                    selected = (selected + 1) % MENU_ITEMS.len();
                    needs_redraw = true;
                }
                Key::Printable(c) if char::from(c) == ' ' => {
                    if selected == 3 {
                        hw.debug_logging = !hw.debug_logging;
                        needs_redraw = true;
                    }
                }
                Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' => {
                    match selected {
                        0 => { action = BootAction::Normal; break; }
                        1 => { action = BootAction::Recovery; break; }
                        2 => { action = BootAction::FirmwareShell; break; }
                        3 => {
                            hw.debug_logging = !hw.debug_logging;
                            needs_redraw = true;
                        }
                        4 => runtime::reset(ResetType::COLD, Status::SUCCESS, None),
                        5 => runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None),
                        _ => {}
                    }
                }
                _ => {
                    needs_redraw = true;
                }
            }
        }

        // Poll mouse
        if mouse.poll() {
            countdown_ms = None;
            last_shown_sec = None;
            mouse.hide(fb);

            let rx = card_x + 14;
            let rw = CARD_W - 28;
            let mut chosen = false;
            for idx in 0..MENU_ITEMS.len() {
                let ry = list_y + idx * ITEM_H;
                if mouse.in_rect(rx, ry, rw, ITEM_H - 4) {
                    if selected != idx {
                        selected = idx;
                        needs_redraw = true;
                    }
                    if mouse.just_clicked() {
                        match idx {
                            0 => { action = BootAction::Normal; chosen = true; break; }
                            1 => { action = BootAction::Recovery; chosen = true; break; }
                            2 => { action = BootAction::FirmwareShell; chosen = true; break; }
                            3 => {
                                hw.debug_logging = !hw.debug_logging;
                                needs_redraw = true;
                            }
                            4 => runtime::reset(ResetType::COLD, Status::SUCCESS, None),
                            5 => runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None),
                            _ => {}
                        }
                    }
                }
            }

            if chosen {
                break;
            }
            if !needs_redraw {
                mouse.draw(fb);
            }
        }

        if let Some(rem) = countdown_ms {
            if rem <= 25 {
                action = BootAction::Normal;
                break;
            }
            let next_rem = rem - 25;
            countdown_ms = Some(next_rem);
            let sec = (next_rem + 999) / 1000;
            if Some(sec) != last_shown_sec {
                last_shown_sec = Some(sec);
                needs_redraw = true;
            }
        }

        sleep_ms(25);
    }

    // Boot progress splash
    mouse.hide(fb);
    fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);
    let splash_wm_y = (fb.height / 2).saturating_sub(WORDMARK_H / 2 + 28);
    fb.draw_wordmark(splash_wm_y, Color::TEXT_PRIMARY);

    let mode_label = match action {
        BootAction::Normal => "Zenkai OS v1.0 Desktop Edition  -  Swapnil Ghosh (25BTRCL267 AIML-E)",
        BootAction::Recovery => "Starting Recovery Diagnostics  -  Swapnil Ghosh",
        BootAction::FirmwareShell => "Opening Zenkai Firmware Shell  -  Swapnil Ghosh",
    };
    let mw = UI_SANS.text_width(mode_label);
    fb.draw_aa_text(fb.width.saturating_sub(mw) / 2, splash_wm_y + WORDMARK_H + 8, &UI_SANS, mode_label, Color::TEXT_MUTED, None);

    let bar_w = 320usize;
    let bar_h = 6usize;
    let bar_x = fb.width.saturating_sub(bar_w) / 2;
    let bar_y = splash_wm_y + WORDMARK_H + 46;
    fb.draw_rounded_rect(bar_x, bar_y, bar_w, bar_h, 3, Color::SURFACE_ALT, None);

    let stages = [
        (20, "GOP Video & Hardware Probe", 120u64),
        (40, "Memory Allocator & SpinLock Init", 120),
        (60, "Live Wi-Fi & Ethernet Network Stack", 120),
        (80, "APT/Snapstore & Virtual Filesystem", 120),
        (100, "R19 AI/ML Engine & Zenkai Desktop", 140),
    ];

    let pill_w = 400usize.min(fb.width.saturating_sub(40));
    let pill_x = fb.width.saturating_sub(pill_w) / 2;
    let pill_y = bar_y + 18;

    let mut prev_fill = 0usize;
    for &(pct, stage_name, stall_ms) in &stages {
        fb.draw_rounded_rect(pill_x, pill_y, pill_w, 28, 8, Color::SURFACE, Some(Color::BORDER));
        let st_txt = format!("[{:>3}%]  Loading {}...", pct, stage_name);
        let st_w = UI_SANS.text_width(&st_txt);
        fb.draw_aa_text(
            pill_x + pill_w.saturating_sub(st_w) / 2,
            pill_y + 2,
            &UI_SANS,
            &st_txt,
            Color::ACCENT,
            None,
        );

        let stage_tsc = read_tsc();
        let target_fill = (bar_w * pct) / 100;
        let steps = 4usize;
        for s in 1..=steps {
            let cur_fill = prev_fill + ((target_fill - prev_fill) * s) / steps;
            if cur_fill >= 4 {
                fb.draw_rounded_rect(bar_x, bar_y, cur_fill, bar_h, 3, Color::ACCENT, None);
            }
            sleep_ms((stall_ms / steps as u64) as usize);
        }
        prev_fill = target_fill;
        kok!("STAGE", "[{:>3}%] {}", pct, stage_name);
        hw.record_stage(stage_name, pct, stage_tsc, stall_ms);
    }

    sleep_ms(100);
    action
}
