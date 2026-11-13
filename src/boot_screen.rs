//! Zenkai OS - Visual Bootloader Sequence & Progress Animator
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Graphical boot progression, title banner rendering, and interactive countdown timer.

use crate::draw::{draw_author_badge, draw_progress_card, draw_zenkai_banner};
use crate::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::gop::{Color, Framebuffer};
use crate::timer::sleep_ms;
use crate::kok;
use uefi::proto::console::text::{Key, ScanCode};

pub fn run_visual_boot_sequence(fb: &mut Framebuffer) {
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

    // 5-Stage Hardware Initialization Sequence
    let stages = [
        (20, "Stage 1/5: GOP Video Handshake", "1024x768 32-bit linear framebuffer active"),
        (40, "Stage 2/5: Physical Memory & Heap", "4KB page frames & FreeList coalescing online"),
        (60, "Stage 3/5: Concurrency SpinLocks", "Atomic acquire/release & rdtsc cycle counter online"),
        (80, "Stage 4/5: Process Scheduler & PCB", "Priority ReadyQueue & Round-Robin quantum ready"),
        (100, "Stage 5/5: R19 AI/ML Research Engine", "FIFO/LRU/OPT benchmarks & H1-H5 validation ready"),
    ];

    for &(pct, stage_name, detail) in &stages {
        draw_progress_card(fb, card_x, card_y, card_w, card_h, pct, stage_name, detail);
        kok!("STAGE", "[{:>3}%] {} ({})", pct, stage_name, detail);
        sleep_ms(200);
    }

    // 5. Animated Countdown Prompt with Enter Key Bypass
    let prompt_y = card_y + card_h + 30;
    for countdown in (1..=2).rev() {
        let msg = match countdown {
            2 => "Boot complete! Entering Graphical Console in 2s... [Press ENTER to launch now]",
            1 => "Boot complete! Entering Graphical Console in 1s... [Press ENTER to launch now]",
            _ => "Launching Zenkai Graphical Console...",
        };

        let msg_w = msg.len() * FONT_WIDTH;
        let msg_x = (fb.width.saturating_sub(msg_w)) / 2;

        // Clear prompt line
        fb.fill_rect(0, prompt_y, fb.width, FONT_HEIGHT + 4, Color::BG_OBSIDIAN);
        fb.draw_text(msg_x, prompt_y, msg, Color::CYAN, None, 1);

        // Check for Enter key during this 1-second interval (10 checks of 100ms)
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
            sleep_ms(100);
        }
    }
}
