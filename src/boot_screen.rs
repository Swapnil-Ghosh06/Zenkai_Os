//! Zenkai OS - Visual Bootloader Sequence & Progress Animator
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Graphical boot progression, title banner rendering, and component boot-time measurement.

use crate::draw::{draw_author_badge, draw_progress_card, draw_zenkai_banner};
use crate::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::kok;
use crate::timer::{read_tsc, sleep_ms};
use uefi::proto::console::text::{Key, ScanCode};

pub fn run_visual_boot_sequence(fb: &mut Framebuffer, hw: &mut HardwareInfo) {
    // 1. Clear to Obsidian background
    fb.clear(Color::BG_OBSIDIAN);

    // 2. Draw cybernetic title banner
    let banner_y = (fb.height / 7).max(40);
    draw_zenkai_banner(fb, banner_y);

    // 3. Draw author badge
    let badge_y = banner_y + 80;
    draw_author_badge(fb, badge_y);

    // 4. Progress bar coordinates
    let card_w = 680.min(fb.width.saturating_sub(80));
    let card_h = 130;
    let card_x = (fb.width.saturating_sub(card_w)) / 2;
    let card_y = badge_y + 80;

    // 5-Stage Hardware Initialization Sequence with real TSC profiling
    let stages = [
        (20, "Stage 1/5: GOP Video & Laptop Probe", "1280x800 linear framebuffer & CPUID/PCI scan", 180u64),
        (40, "Stage 2/5: Memory & SpinLock Init", "4KB page frames, FreeList heap & SpinLocks", 160),
        (60, "Stage 3/5: Network (WiFi & LAN)", "eth0 LAN + wlan0 wpasupplicant initialized", 190),
        (80, "Stage 4/5: Aptitude & Snapstore", "apt alias & snapd package catalog mounted", 170),
        (100, "Stage 5/5: R19 AI/ML Research Engine", "FIFO/LRU/OPT benchmarks & H1-H5 ready", 200),
    ];

    for &(pct, stage_name, detail, stall_ms) in &stages {
        let stage_tsc = read_tsc();
        draw_progress_card(fb, card_x, card_y, card_w, card_h, pct, stage_name, detail);
        kok!("STAGE", "[{:>3}%] {} ({})", pct, stage_name, detail);
        sleep_ms(stall_ms as usize);
        hw.record_stage(stage_name, pct, stage_tsc, stall_ms);
    }

    // 5. Animated Countdown Prompt with Enter Key Bypass
    let prompt_y = card_y + card_h + 30;
    for countdown in (1..=2).rev() {
        let msg = match countdown {
            2 => "Boot complete! Launching Zenkai Desktop in 2s... [Press ENTER to launch now]",
            1 => "Boot complete! Launching Zenkai Desktop in 1s... [Press ENTER to launch now]",
            _ => "Launching Zenkai Desktop...",
        };

        let msg_w = msg.len() * FONT_WIDTH;
        let msg_x = (fb.width.saturating_sub(msg_w)) / 2;

        fb.fill_rect(0, prompt_y, fb.width, FONT_HEIGHT + 4, Color::BG_OBSIDIAN);
        fb.draw_text(msg_x, prompt_y, msg, Color::CYAN, None, 1);

        for _ in 0..10 {
            let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key().unwrap_or(None));
            if let Some(key) = key_opt {
                match key {
                    Key::Printable(ch) => {
                        let c = char::from(ch);
                        if c == '\r' || c == '\n' {
                            return;
                        }
                    }
                    Key::Special(scan) if scan == ScanCode::NULL => return,
                    _ => {}
                }
            }
            sleep_ms(80);
        }
    }
}
