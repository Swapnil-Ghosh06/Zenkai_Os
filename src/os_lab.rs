//! Zenkai OS - Interactive OS Lab (Producer-Consumer & R19 AI/ML Visualizer)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Live semaphore/mutex bounded-buffer simulator + graphical R19 page-fault chart.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use uefi::proto::console::text::{Key, ScanCode};

use crate::font_aa::{TERM_MONO, UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::mouse::MouseDriver;
use crate::r19::{
    gen_bursty, gen_random, gen_sequential, gen_strided, simulate_fifo, simulate_lru, simulate_opt,
    REF_LENGTH,
};
use crate::timer::read_tsc;

const BUFFER_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct R19BarRow {
    pub name: &'static str,
    pub fifo: usize,
    pub lru: usize,
    pub opt: usize,
}

pub struct OsLabState {
    pub mode_r19: bool,
    // Producer-Consumer Bounded Buffer state
    pub slots: [Option<(u8, u32)>; BUFFER_CAP],
    pub in_idx: usize,
    pub out_idx: usize,
    pub empty_sem: usize,
    pub full_sem: usize,
    pub last_actor: &'static str,
    pub next_seq: u32,
    pub produced_total: u32,
    pub consumed_total: u32,
    pub blocked_events: u32,
    pub running: bool,
    pub last_step_tsc: u64,
    pub event_log: Vec<String>,
    // R19 Bar Chart state
    pub r19_frames: usize,
    pub r19_rows: [R19BarRow; 4],
}

impl OsLabState {
    pub fn new() -> Self {
        let mut s = Self {
            mode_r19: false,
            slots: [None; BUFFER_CAP],
            in_idx: 0,
            out_idx: 0,
            empty_sem: BUFFER_CAP,
            full_sem: 0,
            last_actor: "IDLE",
            next_seq: 1,
            produced_total: 0,
            consumed_total: 0,
            blocked_events: 0,
            running: true,
            last_step_tsc: read_tsc(),
            event_log: Vec::new(),
            r19_frames: 12,
            r19_rows: [
                R19BarRow { name: "Sequential", fifo: 0, lru: 0, opt: 0 },
                R19BarRow { name: "Strided", fifo: 0, lru: 0, opt: 0 },
                R19BarRow { name: "Bursty", fifo: 0, lru: 0, opt: 0 },
                R19BarRow { name: "Random", fifo: 0, lru: 0, opt: 0 },
            ],
        };
        s.push_log(String::from("[INIT] Bounded Buffer (cap=8), mutex=1, empty=8, full=0."));
        s.recompute_r19(12);
        s
    }

    pub fn reset_pc(&mut self) {
        self.slots = [None; BUFFER_CAP];
        self.in_idx = 0;
        self.out_idx = 0;
        self.empty_sem = BUFFER_CAP;
        self.full_sem = 0;
        self.last_actor = "RESET";
        self.next_seq = 1;
        self.produced_total = 0;
        self.consumed_total = 0;
        self.blocked_events = 0;
        self.event_log.clear();
        self.push_log(String::from("[RESET] Semaphores reset: empty=8, full=0, mutex=1."));
    }

    fn push_log(&mut self, msg: String) {
        self.event_log.push(msg);
        if self.event_log.len() > 6 {
            self.event_log.remove(0);
        }
    }

    pub fn produce_one(&mut self, producer_id: u8) {
        if self.empty_sem == 0 {
            self.blocked_events += 1;
            self.last_actor = "P-WAIT";
            self.push_log(format!(
                "[WAIT ] Producer P{} blocked on sem_wait(empty=0) -> buffer full!",
                producer_id
            ));
            return;
        }
        self.empty_sem -= 1;
        let item = self.next_seq;
        self.next_seq += 1;
        self.slots[self.in_idx] = Some((producer_id, item));
        let slot_used = self.in_idx;
        self.in_idx = (self.in_idx + 1) % BUFFER_CAP;
        self.full_sem += 1;
        self.produced_total += 1;
        self.last_actor = "PRODUCER";
        self.push_log(format!(
            "[PROD ] P{} wrote item #{} -> slot[{}] (empty={}, full={})",
            producer_id, item, slot_used, self.empty_sem, self.full_sem
        ));
    }

    pub fn consume_one(&mut self, consumer_id: u8) {
        if self.full_sem == 0 {
            self.blocked_events += 1;
            self.last_actor = "C-WAIT";
            self.push_log(format!(
                "[WAIT ] Consumer C{} blocked on sem_wait(full=0) -> buffer empty!",
                consumer_id
            ));
            return;
        }
        self.full_sem -= 1;
        let slot_used = self.out_idx;
        let popped = self.slots[self.out_idx].take();
        self.out_idx = (self.out_idx + 1) % BUFFER_CAP;
        self.empty_sem += 1;
        self.consumed_total += 1;
        self.last_actor = "CONSUMER";
        if let Some((pid, item)) = popped {
            self.push_log(format!(
                "[CONS ] C{} read P{}:#{} <- slot[{}] (empty={}, full={})",
                consumer_id, pid, item, slot_used, self.empty_sem, self.full_sem
            ));
        }
    }

    pub fn tick_if_ready(&mut self) -> bool {
        if self.mode_r19 || !self.running {
            return false;
        }
        let now = read_tsc();
        if now.saturating_sub(self.last_step_tsc) < 620 * 2_400_000 {
            return false;
        }
        self.last_step_tsc = now;
        let mix = (now >> 8) ^ (self.next_seq as u64 * 17);
        let prod_bias = if self.full_sem < 2 {
            75
        } else if self.empty_sem < 2 {
            25
        } else {
            52
        };
        if (mix % 100) < prod_bias {
            let pid = ((mix % 2) + 1) as u8;
            self.produce_one(pid);
        } else {
            let cid = ((mix % 2) + 1) as u8;
            self.consume_one(cid);
        }
        true
    }

    pub fn recompute_r19(&mut self, frames: usize) {
        self.r19_frames = frames;
        let mut seq = [0u8; REF_LENGTH];
        let mut strided = [0u8; REF_LENGTH];
        let mut burst = [0u8; REF_LENGTH];
        let mut rand = [0u8; REF_LENGTH];
        gen_sequential(&mut seq);
        gen_strided(&mut strided);
        gen_bursty(&mut burst, 0x1A2B_3C4D);
        gen_random(&mut rand, 0x5EED_9999);

        self.r19_rows = [
            R19BarRow {
                name: "Sequential",
                fifo: simulate_fifo(&seq, frames),
                lru: simulate_lru(&seq, frames),
                opt: simulate_opt(&seq, frames),
            },
            R19BarRow {
                name: "Strided",
                fifo: simulate_fifo(&strided, frames),
                lru: simulate_lru(&strided, frames),
                opt: simulate_opt(&strided, frames),
            },
            R19BarRow {
                name: "Bursty",
                fifo: simulate_fifo(&burst, frames),
                lru: simulate_lru(&burst, frames),
                opt: simulate_opt(&burst, frames),
            },
            R19BarRow {
                name: "Random",
                fifo: simulate_fifo(&rand, frames),
                lru: simulate_lru(&rand, frames),
                opt: simulate_opt(&rand, frames),
            },
        ];
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        match key {
            Key::Special(ScanCode::LEFT) | Key::Special(ScanCode::RIGHT) => {
                self.mode_r19 = !self.mode_r19;
                true
            }
            Key::Printable(c16) => {
                let ch = char::from(c16);
                match ch {
                    ' ' => {
                        if !self.mode_r19 {
                            self.running = !self.running;
                            return true;
                        }
                        false
                    }
                    'p' | 'P' | '+' => {
                        if !self.mode_r19 {
                            self.produce_one(1);
                            return true;
                        }
                        false
                    }
                    'c' | 'C' | '-' => {
                        if !self.mode_r19 {
                            self.consume_one(1);
                            return true;
                        }
                        false
                    }
                    'r' | 'R' => {
                        if self.mode_r19 {
                            self.recompute_r19(self.r19_frames);
                        } else {
                            self.reset_pc();
                        }
                        true
                    }
                    '1' => { self.recompute_r19(4); self.mode_r19 = true; true }
                    '2' => { self.recompute_r19(8); self.mode_r19 = true; true }
                    '3' => { self.recompute_r19(12); self.mode_r19 = true; true }
                    '4' => { self.recompute_r19(16); self.mode_r19 = true; true }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    pub fn handle_click(&mut self, mouse: &MouseDriver, wx: usize, wy: usize, ww: usize, _wh: usize) -> bool {
        // Sub-tab switcher
        if mouse.in_rect(wx + 24, wy + 50, 210, 30) {
            self.mode_r19 = false;
            return true;
        }
        if mouse.in_rect(wx + 242, wy + 50, 200, 30) {
            self.mode_r19 = true;
            return true;
        }

        if !self.mode_r19 {
            let btn_y = wy + 258;
            if mouse.in_rect(wx + 24, btn_y, 126, 30) {
                self.running = !self.running;
                return true;
            }
            if mouse.in_rect(wx + 160, btn_y, 126, 30) {
                self.produce_one(1);
                return true;
            }
            if mouse.in_rect(wx + 296, btn_y, 126, 30) {
                self.consume_one(1);
                return true;
            }
            if mouse.in_rect(wx + 432, btn_y, 100, 30) {
                self.reset_pc();
                return true;
            }
        } else {
            let opts = [4usize, 8, 12, 16];
            for (i, &f) in opts.iter().enumerate() {
                let bx = wx + ww - 236 + i * 54;
                if mouse.in_rect(bx, wy + 50, 48, 30) {
                    self.recompute_r19(f);
                    return true;
                }
            }
        }
        false
    }

    pub fn draw(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize) {
        let pc_bg = if !self.mode_r19 { Color::INK_SOLID } else { Color::SURFACE_ALT };
        let pc_fg = if !self.mode_r19 { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 24, wy + 50, 210, 30, 8, pc_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 42, wy + 53, &UI_SANS, "Producer-Consumer", pc_fg, None);

        let r19_bg = if self.mode_r19 { Color::INK_SOLID } else { Color::SURFACE_ALT };
        let r19_fg = if self.mode_r19 { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 242, wy + 50, 200, 30, 8, r19_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 260, wy + 53, &UI_SANS, "R19 Page Visualizer", r19_fg, None);

        if !self.mode_r19 {
            self.draw_producer_consumer(fb, wx, wy, ww, wh);
        } else {
            self.draw_r19_visualizer(fb, wx, wy, ww, wh);
        }
    }

    fn draw_producer_consumer(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize) {
        // 3 Semaphore cards
        let card_w = (ww.saturating_sub(72)) / 3;
        let cards = [
            ("MUTEX LOCK", self.last_actor, format!("Produced: {}  |  Consumed: {}", self.produced_total, self.consumed_total), Color::ACCENT),
            ("SEM EMPTY", if self.empty_sem == 0 { "0 (FULL)" } else { "READY" }, format!("Free Slots: {} / {}", self.empty_sem, BUFFER_CAP), Color::SAGE),
            ("SEM FULL", if self.full_sem == 0 { "0 (EMPTY)" } else { "READY" }, format!("Filled: {} / {}  |  Blocked: {}", self.full_sem, BUFFER_CAP, self.blocked_events), Color::AMBER),
        ];

        for (i, (title, badge, sub, col)) in cards.iter().enumerate() {
            let cx = wx + 24 + i * (card_w + 12);
            fb.draw_glass_panel(cx, wy + 90, card_w, 54, 9, Color::SURFACE_CARD, Color::BORDER);
            fb.draw_aa_text(cx + 12, wy + 94, &UI_SANS, title, Color::TEXT_MUTED, None);
            let bw = UI_SANS.text_width(badge);
            if cx + card_w > cx + 100 + bw {
                fb.draw_aa_text(cx + card_w - bw - 12, wy + 94, &UI_SANS, badge, *col, None);
            }
            let max_s = (card_w.saturating_sub(24) / 8).max(14);
            let sub_fit: String = sub.chars().take(max_s).collect();
            fb.draw_aa_text(cx + 12, wy + 118, &UI_SANS, &sub_fit, Color::TEXT_PRIMARY, None);
        }

        // Circular Buffer Panel
        let buf_y = wy + 154;
        let buf_w = ww.saturating_sub(48);
        fb.draw_glass_panel(wx + 24, buf_y, buf_w, 94, 10, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(wx + 38, buf_y + 6, &UI_SANS, "Shared Circular Ring Buffer (8 Slots):", Color::TEXT_PRIMARY, None);

        let slot_gap = 8usize;
        let slot_w = (buf_w.saturating_sub(28 + slot_gap * 7)) / BUFFER_CAP;
        for i in 0..BUFFER_CAP {
            let sx = wx + 38 + i * (slot_w + slot_gap);
            let sy = buf_y + 34;
            let (fill, brd, txt) = if let Some((pid, seq)) = self.slots[i] {
                (Color::new(220, 244, 228), Color::SAGE, format!("P{}:#{}", pid, seq))
            } else {
                (Color::SURFACE, Color::BORDER, format!("[{}]", i))
            };
            fb.draw_rounded_rect(sx, sy, slot_w, 36, 7, fill, Some(brd));
            let tw = TERM_MONO.text_width(&txt);
            fb.draw_aa_text(sx + (slot_w.saturating_sub(tw)) / 2, sy + 7, &TERM_MONO, &txt, Color::TEXT_PRIMARY, None);

            if i == self.in_idx {
                fb.draw_aa_text(sx + 4, sy + 38, &UI_SANS, "IN", Color::SAGE, None);
            }
            if i == self.out_idx {
                let ox = if i == self.in_idx { sx + slot_w.saturating_sub(32) } else { sx + 4 };
                fb.draw_aa_text(ox, sy + 38, &UI_SANS, "OUT", Color::CORAL, None);
            }
        }

        // Controls
        let btn_y = wy + 258;
        let play_lbl = if self.running { "Pause Sim" } else { "Resume Sim" };
        let play_col = if self.running { Color::AMBER } else { Color::SAGE };
        fb.draw_rounded_rect(wx + 24, btn_y, 126, 30, 7, Color::SURFACE_HOVER, Some(play_col));
        let pw = UI_SANS.text_width(play_lbl);
        fb.draw_aa_text(wx + 24 + (126usize.saturating_sub(pw)) / 2, btn_y + 3, &UI_SANS, play_lbl, play_col, None);

        fb.draw_rounded_rect(wx + 160, btn_y, 126, 30, 7, Color::SURFACE_HOVER, Some(Color::SAGE));
        let prw = UI_SANS.text_width("+ Produce");
        fb.draw_aa_text(wx + 160 + (126usize.saturating_sub(prw)) / 2, btn_y + 3, &UI_SANS, "+ Produce", Color::SAGE, None);

        fb.draw_rounded_rect(wx + 296, btn_y, 126, 30, 7, Color::SURFACE_HOVER, Some(Color::CORAL));
        let cw = UI_SANS.text_width("- Consume");
        fb.draw_aa_text(wx + 296 + (126usize.saturating_sub(cw)) / 2, btn_y + 3, &UI_SANS, "- Consume", Color::CORAL, None);

        fb.draw_rounded_rect(wx + 432, btn_y, 100, 30, 7, Color::SURFACE_HOVER, Some(Color::BORDER));
        let rw = UI_SANS.text_width("Reset");
        fb.draw_aa_text(wx + 432 + (100usize.saturating_sub(rw)) / 2, btn_y + 3, &UI_SANS, "Reset", Color::TEXT_PRIMARY, None);

        // Live Atomic Event Log
        let log_y = wy + 298;
        let log_h = wh.saturating_sub(342);
        if log_h >= 48 {
            fb.draw_glass_panel(wx + 24, log_y, buf_w, log_h, 10, Color::SURFACE_CARD, Color::BORDER);
            let max_rows = (log_h.saturating_sub(14) / 22).max(1);
            let max_cols = (buf_w.saturating_sub(24) / 9).max(30);
            let start = self.event_log.len().saturating_sub(max_rows);
            for (v_idx, line) in self.event_log[start..].iter().enumerate() {
                let col = if line.starts_with("[PROD") {
                    Color::SAGE
                } else if line.starts_with("[CONS") {
                    Color::ACCENT
                } else if line.starts_with("[WAIT") {
                    Color::AMBER
                } else {
                    Color::TEXT_MUTED
                };
                let lfit: String = line.chars().take(max_cols).collect();
                fb.draw_aa_text(wx + 36, log_y + 8 + v_idx * 22, &TERM_MONO, &lfit, col, None);
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(
            wx + 28,
            wy + wh - 26,
            &UI_SANS,
            "Producer-Consumer Semaphore Lab  |  Space: Play/Pause  |  P: Produce  |  C: Consume",
            Color::ACCENT,
            None,
        );
    }

    fn draw_r19_visualizer(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize) {
        // Frame count selector buttons on top right
        for (i, &f) in [4usize, 8, 12, 16].iter().enumerate() {
            let bx = wx + ww - 236 + i * 54;
            let active = self.r19_frames == f;
            let bg = if active { Color::INK_SOLID } else { Color::SURFACE_CARD };
            let fg = if active { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
            fb.draw_rounded_rect(bx, wy + 50, 48, 30, 7, bg, Some(Color::BORDER));
            let lbl = format!("{}F", f);
            let lw = UI_SANS.text_width(&lbl);
            fb.draw_aa_text(bx + (48usize.saturating_sub(lw)) / 2, wy + 53, &UI_SANS, &lbl, fg, None);
        }

        // Legend
        let legend = format!(
            "Page Faults across 500 AI/ML References (Frames = {})  |  Red: FIFO   Blue: LRU   Green: OPT",
            self.r19_frames
        );
        let max_leg = ((ww - 56) / 8).max(32);
        let leg_fit: String = legend.chars().take(max_leg).collect();
        fb.draw_aa_text(wx + 28, wy + 90, &UI_SANS, &leg_fit, Color::TEXT_MUTED, None);

        // 4 Workload Bar Cards
        let start_y = wy + 120;
        let avail_h = (wh.saturating_sub(164)).max(240);
        let card_h = (avail_h / 4).saturating_sub(6);
        let card_w = ww.saturating_sub(48);
        let bar_max_w = card_w.saturating_sub(280).max(120);

        for (idx, row) in self.r19_rows.iter().enumerate() {
            let cy = start_y + idx * (card_h + 6);
            fb.draw_glass_panel(wx + 24, cy, card_w, card_h, 9, Color::SURFACE_CARD, Color::BORDER);
            fb.draw_aa_text(wx + 38, cy + 8, &UI_HEADING, row.name, Color::TEXT_PRIMARY, None);

            let bars = [
                ("FIFO", row.fifo, Color::CORAL, cy + 8),
                ("LRU ", row.lru, Color::ACCENT, cy + 28),
                ("OPT ", row.opt, Color::SAGE, cy + 48),
            ];
            for &(lbl, faults, col, by) in &bars {
                if by + 16 > cy + card_h {
                    continue;
                }
                fb.draw_aa_text(wx + 166, by - 2, &TERM_MONO, lbl, col, None);
                let fill_w = ((faults * bar_max_w) / REF_LENGTH).max(6);
                fb.draw_rounded_rect(wx + 210, by + 2, bar_max_w, 12, 4, Color::SURFACE, None);
                fb.draw_rounded_rect(wx + 210, by + 2, fill_w, 12, 4, col, None);
                let pct = (faults * 100) / REF_LENGTH;
                let stat = format!("{:>3} ({:>2}%)", faults, pct);
                fb.draw_aa_text(wx + 218 + bar_max_w, by - 2, &TERM_MONO, &stat, Color::TEXT_PRIMARY, None);
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(
            wx + 28,
            wy + wh - 26,
            &UI_SANS,
            "R19 Research Visualizer (CA-2 / CA-6)  |  Click 4F / 8F / 12F / 16F to compare live",
            Color::VIOLET,
            None,
        );
    }
}
