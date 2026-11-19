//! Zenkai OS - In-Memory Notes & Python Scratchpad App
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Editable scratchpad integrated with the File Manager & Terminal (.venv) runner.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use uefi::proto::console::text::{Key, ScanCode};

use crate::font_aa::{TERM_MONO, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::mouse::MouseDriver;

pub struct NoteDoc {
    pub name: &'static str,
    pub lines: Vec<String>,
}

pub struct NotesState {
    pub active_doc: usize,
    pub docs: Vec<NoteDoc>,
    pub scroll_top: usize,
}

impl NotesState {
    pub fn new() -> Self {
        Self {
            active_doc: 0,
            scroll_top: 0,
            docs: vec![
                NoteDoc {
                    name: "notes.txt",
                    lines: vec![
                        String::from("# Zenkai OS v1.0 - Engineering Notes"),
                        String::from("Author : Swapnil Ghosh (25BTRCL267, AIML-E)"),
                        String::from("Course : 25PCCS201 Operating Systems (CA-6)"),
                        String::from(""),
                        String::from("- Bare-metal UEFI micro-kernel written in no_std Rust"),
                        String::from("- R19 Research: FIFO vs LRU vs OPT on AI/ML workloads"),
                        String::from("- Ephemeral Role Profiles (.venv) auto-clean on switch"),
                        String::from("- Live Host Wi-Fi Scanner + Password Auth Modal"),
                        String::from(""),
                        String::from("Type directly here to edit this note in memory!"),
                    ],
                },
                NoteDoc {
                    name: "train_ai.py",
                    lines: vec![
                        String::from("# R19 AI/ML Tensor & Page Tracker Script"),
                        String::from("import torch, numpy as np"),
                        String::from("from zenkai.r19 import PageSimulator"),
                        String::from(""),
                        String::from("batch = torch.randn(64, 128)"),
                        String::from("sim = PageSimulator(policy='R19-OPT', frames=12)"),
                        String::from("loss = sim.step(batch)"),
                        String::from("print('R19 Tensor Pass Complete | Loss:', loss)"),
                    ],
                },
                NoteDoc {
                    name: "todo.md",
                    lines: vec![
                        String::from("# CA-6 Operating Systems Submission Checklist"),
                        String::from("[x] 6 Micro-Kernel Layers + Dual Serial/GOP Console"),
                        String::from("[x] R19 AI/ML Page Replacement (H1-H5 validated)"),
                        String::from("[x] Live Host Wi-Fi Scan + WPA2/WPA3 Password Auth"),
                        String::from("[x] Zenkai Home Desktop, File Manager, OS Lab, Snake"),
                        String::from("[x] Hybrid UEFI Live ISO (< 35 MB, VirtualBox OVA)"),
                    ],
                },
            ],
        }
    }

    pub fn get_doc_lines(&self, name: &str) -> Option<Vec<String>> {
        self.docs.iter().find(|d| d.name == name).map(|d| d.lines.clone())
    }

    pub fn handle_key(&mut self, key: Key) -> Option<&'static str> {
        let doc = self.docs.get_mut(self.active_doc)?;
        match key {
            Key::Special(ScanCode::UP) => {
                self.scroll_top = self.scroll_top.saturating_sub(1);
            }
            Key::Special(ScanCode::DOWN) => {
                if self.scroll_top + 1 < doc.lines.len() {
                    self.scroll_top += 1;
                }
            }
            Key::Printable(c16) => {
                let ch = char::from(c16);
                if ch == '\r' || ch == '\n' {
                    doc.lines.push(String::new());
                } else if ch == '\x08' || ch == '\x7f' {
                    let line_count = doc.lines.len();
                    if let Some(last) = doc.lines.last_mut() {
                        if last.is_empty() && line_count > 1 {
                            doc.lines.pop();
                        } else {
                            last.pop();
                        }
                    }
                } else if ch.is_ascii() && !ch.is_control() {
                    if doc.lines.is_empty() {
                        doc.lines.push(String::new());
                    }
                    if let Some(last) = doc.lines.last_mut() {
                        if last.len() < 76 {
                            last.push(ch);
                        } else {
                            let mut next = String::new();
                            next.push(ch);
                            doc.lines.push(next);
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }

    pub fn handle_click(
        &mut self,
        mouse: &MouseDriver,
        wx: usize,
        wy: usize,
        ww: usize,
        _wh: usize,
    ) -> Option<&'static str> {
        for i in 0..self.docs.len() {
            let tx = wx + 24 + i * 130;
            if mouse.in_rect(tx, wy + 50, 122, 30) {
                self.active_doc = i;
                self.scroll_top = 0;
                return None;
            }
        }
        // [ Clear ] button
        if mouse.in_rect(wx + ww - 228, wy + 50, 86, 30) {
            if let Some(doc) = self.docs.get_mut(self.active_doc) {
                doc.lines.clear();
                doc.lines.push(String::new());
                self.scroll_top = 0;
            }
            return None;
        }
        // [ Run .py ] button
        if mouse.in_rect(wx + ww - 134, wy + 50, 110, 30) {
            return Some("code train_ai.py");
        }
        None
    }

    pub fn draw(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize) {
        for (i, doc) in self.docs.iter().enumerate() {
            let tx = wx + 24 + i * 130;
            let active = i == self.active_doc;
            let bg = if active { Color::INK_SOLID } else { Color::SURFACE_ALT };
            let fg = if active { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
            let brd = if active { None } else { Some(Color::BORDER) };
            fb.draw_rounded_rect(tx, wy + 50, 122, 30, 7, bg, brd);
            let tw = UI_SANS.text_width(doc.name);
            fb.draw_aa_text(tx + (122usize.saturating_sub(tw)) / 2, wy + 53, &UI_SANS, doc.name, fg, None);
        }

        fb.draw_rounded_rect(wx + ww - 228, wy + 50, 86, 30, 7, Color::SURFACE, Some(Color::BORDER));
        let cw = UI_SANS.text_width("Clear");
        fb.draw_aa_text(wx + ww - 228 + (86usize.saturating_sub(cw)) / 2, wy + 53, &UI_SANS, "Clear", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(wx + ww - 134, wy + 50, 110, 30, 7, Color::INK_SOLID, None);
        let rw = UI_SANS.text_width("Run Script");
        fb.draw_aa_text(wx + ww - 134 + (110usize.saturating_sub(rw)) / 2, wy + 53, &UI_SANS, "Run Script", Color::GLASS_TEXT, None);

        let ed_x = wx + 24;
        let ed_y = wy + 90;
        let ed_w = ww.saturating_sub(48);
        let ed_h = wh.saturating_sub(136);
        fb.draw_glass_panel(ed_x, ed_y, ed_w, ed_h, 10, Color::SURFACE_ALT, Color::BORDER);

        if let Some(doc) = self.docs.get(self.active_doc) {
            let max_rows = (ed_h.saturating_sub(16) / 22).max(1);
            let max_cols = (ed_w.saturating_sub(68) / 9).max(30);
            let last_idx = doc.lines.len().saturating_sub(1);
            for (v_idx, line) in doc.lines.iter().skip(self.scroll_top).take(max_rows).enumerate() {
                let line_num = self.scroll_top + v_idx + 1;
                let ry = ed_y + 10 + v_idx * 22;
                let num_s = format!("{:>3} |", line_num);
                fb.draw_aa_text(ed_x + 10, ry, &TERM_MONO, &num_s, Color::TEXT_DIM, None);

                let col = if line.trim_start().starts_with('#') {
                    Color::SAGE
                } else if line.trim_start().starts_with("import") || line.trim_start().starts_with("from") {
                    Color::VIOLET
                } else {
                    Color::TEXT_PRIMARY
                };
                let mut text_fit: String = line.chars().take(max_cols).collect();
                if self.scroll_top + v_idx == last_idx {
                    text_fit.push('_');
                }
                fb.draw_aa_text(ed_x + 60, ry, &TERM_MONO, &text_fit, col, None);
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        let line_cnt = self.docs.get(self.active_doc).map(|d| d.lines.len()).unwrap_or(0);
        let foot = format!("Sticky Notes  |  Lines: {}  |  Type to edit  |  Synced with Files", line_cnt);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &foot, Color::TEXT_MUTED, None);
    }
}
