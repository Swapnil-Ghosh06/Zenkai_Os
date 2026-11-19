//! Zenkai OS - Dual Graphical & Serial Console (Anti-Aliased Menlo)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Simultaneous rendering to bounded UEFI Terminal Window and UART COM1 (0x3F8) Serial Port.
//! Features 600-line wrapped scrollback buffer and an interactive right-edge scrollbar.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use crate::font_aa::{TERM_MONO, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::serial::SerialPort;
use crate::sync::SpinLock;

const CELL_W: usize = 9;
const CELL_H: usize = 22;
const MAX_HISTORY: usize = 600;
const SCROLLBAR_RESERVED_W: usize = 20;

#[derive(Clone)]
pub struct ConsoleLine {
    pub text: String,
    pub color: Color,
}

pub struct Console {
    pub fb: Option<Framebuffer>,
    pub vp_x: usize,
    pub vp_y: usize,
    pub vp_w: usize,
    pub vp_h: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub fg: Color,
    pub bg: Color,
    pub paint_fb: bool,
    pub history: Vec<ConsoleLine>,
    pub current_line: String,
    pub scroll_offset: usize,
}

impl Console {
    pub const fn new() -> Self {
        Self {
            fb: None,
            vp_x: 24,
            vp_y: 24,
            vp_w: 1200,
            vp_h: 720,
            cursor_x: 24,
            cursor_y: 24,
            fg: Color::TEXT_PRIMARY,
            bg: Color::SURFACE,
            paint_fb: true,
            history: Vec::new(),
            current_line: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn set_framebuffer(&mut self, fb: Framebuffer) {
        self.vp_x = 24;
        self.vp_y = 24;
        self.vp_w = fb.width.saturating_sub(48);
        self.vp_h = fb.height.saturating_sub(48);
        self.cursor_x = self.vp_x;
        self.cursor_y = self.vp_y;
        self.fb = Some(fb);
    }

    pub fn set_viewport(&mut self, x: usize, y: usize, w: usize, h: usize, bg: Color) {
        self.vp_x = x;
        self.vp_y = y;
        self.vp_w = w;
        self.vp_h = h;
        self.bg = bg;
        self.cursor_x = x;
        self.cursor_y = y;
    }

    pub fn max_cols(&self) -> usize {
        self.vp_w.saturating_sub(SCROLLBAR_RESERVED_W + 4) / CELL_W
    }

    pub fn max_rows(&self) -> usize {
        (self.vp_h / CELL_H).max(1)
    }

    pub fn max_scroll(&self) -> usize {
        let total = self.history.len() + 1;
        total.saturating_sub(self.max_rows())
    }

    pub fn scrollbar_bounds(&self) -> (usize, usize, usize, usize) {
        let sb_w = 14usize;
        let sb_x = self.vp_x + self.vp_w.saturating_sub(sb_w);
        (sb_x, self.vp_y, sb_w, self.vp_h)
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.current_line.clear();
        self.scroll_offset = 0;
        let max_rows = self.max_rows();
        if self.paint_fb {
            if let Some(fb) = &mut self.fb {
                fb.fill_rect(self.vp_x, self.vp_y, self.vp_w, self.vp_h, self.bg);
                Self::draw_scrollbar_static(
                    fb,
                    self.vp_x,
                    self.vp_y,
                    self.vp_w,
                    self.vp_h,
                    1,
                    max_rows,
                    0,
                    self.bg,
                );
            }
        }
        self.cursor_x = self.vp_x;
        self.cursor_y = self.vp_y;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let max_s = self.max_scroll();
        let next = (self.scroll_offset + lines).min(max_s);
        if next != self.scroll_offset {
            self.scroll_offset = next;
            if self.paint_fb {
                self.repaint_viewport();
            }
        }
    }

    pub fn scroll_down(&mut self, lines: usize) {
        let next = self.scroll_offset.saturating_sub(lines);
        if next != self.scroll_offset {
            self.scroll_offset = next;
            if self.paint_fb {
                self.repaint_viewport();
            }
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.scroll_offset != 0 {
            self.scroll_offset = 0;
            if self.paint_fb {
                self.repaint_viewport();
            }
        }
    }

    fn push_history_line(&mut self, text: String, color: Color) {
        self.history.push(ConsoleLine { text, color });
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    fn draw_scrollbar_static(
        fb: &mut Framebuffer,
        vp_x: usize,
        vp_y: usize,
        vp_w: usize,
        vp_h: usize,
        total_lines: usize,
        max_rows: usize,
        scroll_offset: usize,
        bg: Color,
    ) {
        if vp_h < 48 || vp_w < 30 {
            return;
        }
        let sb_w = 14usize;
        let sb_x = vp_x + vp_w.saturating_sub(sb_w);
        fb.fill_rect(sb_x, vp_y, sb_w, vp_h, bg);

        // Top Up button [▲]
        fb.draw_rounded_rect(sb_x, vp_y, sb_w, 18, 4, Color::SURFACE_CARD, Some(Color::BORDER));
        fb.fill_rect(sb_x + 6, vp_y + 6, 2, 2, Color::TEXT_PRIMARY);
        fb.fill_rect(sb_x + 5, vp_y + 8, 4, 2, Color::TEXT_PRIMARY);
        fb.fill_rect(sb_x + 4, vp_y + 10, 6, 2, Color::TEXT_PRIMARY);

        // Bottom Down button [▼]
        let down_y = vp_y + vp_h.saturating_sub(18);
        fb.draw_rounded_rect(sb_x, down_y, sb_w, 18, 4, Color::SURFACE_CARD, Some(Color::BORDER));
        fb.fill_rect(sb_x + 4, down_y + 6, 6, 2, Color::TEXT_PRIMARY);
        fb.fill_rect(sb_x + 5, down_y + 8, 4, 2, Color::TEXT_PRIMARY);
        fb.fill_rect(sb_x + 6, down_y + 10, 2, 2, Color::TEXT_PRIMARY);

        // Track
        let track_y = vp_y + 22;
        let track_h = vp_h.saturating_sub(44);
        if track_h < 12 {
            return;
        }
        fb.draw_rounded_rect(sb_x + 3, track_y, 8, track_h, 4, Color::SURFACE_ALT, None);

        let max_scroll = total_lines.saturating_sub(max_rows);
        let thumb_h = if total_lines <= max_rows {
            track_h
        } else {
            ((max_rows * track_h) / total_lines).clamp(18, track_h)
        };
        let thumb_travel = track_h.saturating_sub(thumb_h);
        let thumb_y = if max_scroll == 0 {
            track_y + thumb_travel
        } else {
            let pos_from_top = max_scroll.saturating_sub(scroll_offset.min(max_scroll));
            track_y + (pos_from_top * thumb_travel) / max_scroll
        };
        let thumb_col = if scroll_offset > 0 { Color::AMBER } else { Color::ACCENT };
        fb.draw_rounded_rect(sb_x + 3, thumb_y, 8, thumb_h, 4, thumb_col, None);
    }

    fn draw_styled_line(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        base_col: Color,
        bg: Color,
        max_cols: usize,
    ) -> usize {
        let tag_col = if text.starts_with("[OK   ]") {
            Some(Color::SAGE)
        } else if text.starts_with("[INFO ]") {
            Some(Color::ACCENT)
        } else if text.starts_with("[WARN ]") {
            Some(Color::AMBER)
        } else if text.starts_with("[ERROR]") {
            Some(Color::CORAL)
        } else if text.starts_with("(.venv:") {
            Some(Color::VIOLET)
        } else {
            None
        };

        let mut cx = x;
        for (i, ch) in text.chars().enumerate() {
            if i >= max_cols {
                break;
            }
            let col = if let Some(tc) = tag_col {
                if text.starts_with("(.venv:") {
                    if let Some(close_idx) = text.find(')') {
                        if i <= close_idx { tc } else { base_col }
                    } else {
                        base_col
                    }
                } else if i < 18 {
                    tc
                } else {
                    base_col
                }
            } else {
                base_col
            };
            cx += fb.draw_aa_char(cx, y, &TERM_MONO, ch, col, Some(bg));
        }
        cx
    }

    /// Repaints the terminal history inside the current bounded viewport at `scroll_offset`.
    pub fn repaint_viewport(&mut self) {
        let max_rows = self.max_rows();
        let max_cols = self.max_cols().max(20);
        let total = self.history.len() + 1;
        let max_scroll = total.saturating_sub(max_rows);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        let end_idx = total.saturating_sub(self.scroll_offset);
        let start_idx = end_idx.saturating_sub(max_rows);
        let scroll_off = self.scroll_offset;
        let vp_x = self.vp_x;
        let vp_y = self.vp_y;
        let vp_w = self.vp_w;
        let vp_h = self.vp_h;
        let bg = self.bg;
        let fg = self.fg;

        if let Some(fb) = &mut self.fb {
            let was_buffered = fb.is_buffered();
            if !was_buffered {
                fb.begin_frame();
            }
            fb.fill_rect(vp_x, vp_y, vp_w, vp_h, bg);

            let mut cy = vp_y;
            let mut last_cx = vp_x;
            let mut last_cy = vp_y;

            for idx in start_idx..end_idx {
                if idx < self.history.len() {
                    let line = &self.history[idx];
                    Self::draw_styled_line(fb, vp_x, cy, &line.text, line.color, bg, max_cols);
                } else {
                    last_cx = Self::draw_styled_line(fb, vp_x, cy, &self.current_line, fg, bg, max_cols);
                    last_cy = cy;
                }
                cy += CELL_H;
            }

            if scroll_off == 0 {
                self.cursor_x = last_cx;
                self.cursor_y = last_cy;
            }

            Self::draw_scrollbar_static(fb, vp_x, vp_y, vp_w, vp_h, total, max_rows, scroll_off, bg);

            if scroll_off > 0 && vp_w > 320 {
                let pill = format!("Scrollback: -{} lines  [v / End]", scroll_off);
                let pw = UI_SANS.text_width(&pill) + 18;
                let px = vp_x + vp_w.saturating_sub(pw + 24);
                let py = vp_y + 2;
                fb.draw_rounded_rect(px, py, pw, 22, 6, Color::SURFACE_CARD, Some(Color::AMBER));
                fb.draw_aa_text(px + 9, py, &UI_SANS, &pill, Color::AMBER, None);
            }
            if !was_buffered {
                fb.present_rect(vp_x, vp_y, vp_w, vp_h);
            }
        }
    }

    pub fn write_char(&mut self, ch: char) {
        // 1. Always mirror to UART serial as valid UTF-8
        if ch == '\n' {
            SerialPort::write_byte(b'\r');
            SerialPort::write_byte(b'\n');
        } else {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            for b in encoded.bytes() {
                SerialPort::write_byte(b);
            }
        }

        // If user scrolled up and new output arrives, snap back to bottom
        let was_scrolled = self.scroll_offset > 0;
        if was_scrolled {
            self.scroll_offset = 0;
        }

        let max_cols = self.max_cols().max(40);

        // 2. Track in wrapped history buffer
        if ch == '\n' {
            let finished = core::mem::take(&mut self.current_line);
            self.push_history_line(finished, self.fg);
        } else if ch == '\x08' {
            self.current_line.pop();
        } else if ch >= ' ' && ch <= '~' {
            if self.current_line.chars().count() >= max_cols {
                let finished = core::mem::take(&mut self.current_line);
                self.push_history_line(finished, self.fg);
                if self.paint_fb && !was_scrolled {
                    let vp_x = self.vp_x;
                    let vp_y = self.vp_y;
                    let text_w = self.vp_w.saturating_sub(SCROLLBAR_RESERVED_W);
                    let vp_h = self.vp_h;
                    let bg = self.bg;
                    if let Some(fb) = &mut self.fb {
                        self.cursor_x = vp_x;
                        self.cursor_y += CELL_H;
                        Self::scroll_if_needed(fb, vp_x, vp_y, text_w, vp_h, &mut self.cursor_y, bg);
                    }
                }
            }
            self.current_line.push(ch);
        }

        // 3. Render to Framebuffer viewport if active
        if !self.paint_fb {
            return;
        }

        if was_scrolled {
            self.repaint_viewport();
            return;
        }

        let vp_x = self.vp_x;
        let vp_y = self.vp_y;
        let vp_w = self.vp_w;
        let text_w = vp_w.saturating_sub(SCROLLBAR_RESERVED_W);
        let vp_h = self.vp_h;
        let fg = self.fg;
        let bg = self.bg;
        let total_lines = self.history.len() + 1;
        let max_rows = self.max_rows();

        if let Some(fb) = &mut self.fb {
            if ch == '\n' {
                self.cursor_x = vp_x;
                self.cursor_y += CELL_H;
                Self::scroll_if_needed(fb, vp_x, vp_y, text_w, vp_h, &mut self.cursor_y, bg);
                Self::draw_scrollbar_static(fb, vp_x, vp_y, vp_w, vp_h, total_lines, max_rows, 0, bg);
                return;
            } else if ch == '\r' {
                self.cursor_x = vp_x;
                return;
            } else if ch == '\x08' {
                if self.cursor_x >= vp_x + CELL_W {
                    self.cursor_x -= CELL_W;
                    fb.fill_rect(self.cursor_x, self.cursor_y, CELL_W, CELL_H, bg);
                }
                return;
            }

            if self.cursor_x + CELL_W >= vp_x + text_w {
                self.cursor_x = vp_x;
                self.cursor_y += CELL_H;
                Self::scroll_if_needed(fb, vp_x, vp_y, text_w, vp_h, &mut self.cursor_y, bg);
            }

            let adv = fb.draw_aa_char(self.cursor_x, self.cursor_y, &TERM_MONO, ch, fg, Some(bg));
            self.cursor_x += adv;
        }
    }

    fn scroll_if_needed(
        fb: &mut Framebuffer,
        vp_x: usize,
        vp_y: usize,
        text_w: usize,
        vp_h: usize,
        cursor_y: &mut usize,
        bg: Color,
    ) {
        if *cursor_y + CELL_H > vp_y + vp_h {
            fb.scroll_region(vp_x, vp_y, text_w, vp_h, CELL_H, bg);
            *cursor_y = cursor_y.saturating_sub(CELL_H);
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.write_char(ch);
        }
    }
}

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

pub static GLOBAL_CONSOLE: SpinLock<Console> = SpinLock::new(Console::new());

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        let _ = core::fmt::write(&mut *crate::console::GLOBAL_CONSOLE.lock(), format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n");
    };
    ($($arg:tt)*) => {
        let _ = core::fmt::write(&mut *crate::console::GLOBAL_CONSOLE.lock(), format_args!($($arg)*));
        $crate::kprint!("\n");
    };
}

#[macro_export]
macro_rules! kok {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::SAGE;
        let _ = core::fmt::write(&mut *con, format_args!("[OK   ] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::TEXT_PRIMARY;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! kinfo {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::ACCENT;
        let _ = core::fmt::write(&mut *con, format_args!("[INFO ] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::TEXT_PRIMARY;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! kwarn {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::AMBER;
        let _ = core::fmt::write(&mut *con, format_args!("[WARN ] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::TEXT_PRIMARY;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! kerror {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::CORAL;
        let _ = core::fmt::write(&mut *con, format_args!("[ERROR] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::TEXT_PRIMARY;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}
