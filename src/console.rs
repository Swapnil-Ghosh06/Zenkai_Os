//! Zenkai OS - Dual Graphical & Serial Console with Bounded Viewport
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Simultaneous rendering to bounded UEFI Terminal Window and UART COM1 (0x3F8) Serial Port.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use crate::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::gop::{Color, Framebuffer};
use crate::serial::SerialPort;
use crate::sync::SpinLock;

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
}

impl Console {
    pub const fn new() -> Self {
        Self {
            fb: None,
            vp_x: 20,
            vp_y: 20,
            vp_w: 1240,
            vp_h: 760,
            cursor_x: 20,
            cursor_y: 20,
            fg: Color::WHITE,
            bg: Color::BG_PANEL,
            paint_fb: true,
            history: Vec::new(),
            current_line: String::new(),
        }
    }

    pub fn set_framebuffer(&mut self, fb: Framebuffer) {
        self.vp_x = 20;
        self.vp_y = 20;
        self.vp_w = fb.width.saturating_sub(40);
        self.vp_h = fb.height.saturating_sub(40);
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

    pub fn clear(&mut self) {
        self.history.clear();
        self.current_line.clear();
        if self.paint_fb {
            if let Some(fb) = &mut self.fb {
                fb.fill_rect(self.vp_x, self.vp_y, self.vp_w, self.vp_h, self.bg);
            }
        }
        self.cursor_x = self.vp_x;
        self.cursor_y = self.vp_y;
    }

    /// Repaints the terminal history inside the current bounded viewport.
    pub fn repaint_viewport(&mut self) {
        if let Some(fb) = &mut self.fb {
            fb.fill_rect(self.vp_x, self.vp_y, self.vp_w, self.vp_h, self.bg);
            let max_rows = self.vp_h / FONT_HEIGHT;
            let start = self.history.len().saturating_sub(max_rows.saturating_sub(1));
            let mut cy = self.vp_y;
            let max_cols = self.vp_w / FONT_WIDTH;

            for line in &self.history[start..] {
                let mut cx = self.vp_x;
                for (i, ch) in line.text.chars().enumerate() {
                    if i >= max_cols {
                        break;
                    }
                    fb.draw_char(cx, cy, ch, line.color, Some(self.bg), 1);
                    cx += FONT_WIDTH;
                }
                cy += FONT_HEIGHT;
            }

            // Draw current incomplete line (e.g. prompt + typed input)
            let mut cx = self.vp_x;
            for (i, ch) in self.current_line.chars().enumerate() {
                if i >= max_cols {
                    break;
                }
                fb.draw_char(cx, cy, ch, self.fg, Some(self.bg), 1);
                cx += FONT_WIDTH;
            }
            self.cursor_x = cx;
            self.cursor_y = cy;
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

        // 2. Track in history buffer
        if ch == '\n' {
            let finished = core::mem::take(&mut self.current_line);
            self.history.push(ConsoleLine {
                text: finished,
                color: self.fg,
            });
            if self.history.len() > 80 {
                self.history.remove(0);
            }
        } else if ch == '\x08' {
            self.current_line.pop();
        } else if ch >= ' ' && ch <= '~' {
            self.current_line.push(ch);
        }

        // 3. Render to Framebuffer viewport if active
        if !self.paint_fb {
            return;
        }

        let vp_x = self.vp_x;
        let vp_y = self.vp_y;
        let vp_w = self.vp_w;
        let vp_h = self.vp_h;
        let fg = self.fg;
        let bg = self.bg;

        if let Some(fb) = &mut self.fb {
            if ch == '\n' {
                self.cursor_x = vp_x;
                self.cursor_y += FONT_HEIGHT;
                Self::scroll_if_needed(fb, vp_x, vp_y, vp_w, vp_h, &mut self.cursor_y, bg);
                return;
            } else if ch == '\r' {
                self.cursor_x = vp_x;
                return;
            } else if ch == '\x08' {
                if self.cursor_x >= vp_x + FONT_WIDTH {
                    self.cursor_x -= FONT_WIDTH;
                    fb.fill_rect(self.cursor_x, self.cursor_y, FONT_WIDTH, FONT_HEIGHT, bg);
                }
                return;
            }

            if self.cursor_x + FONT_WIDTH >= vp_x + vp_w {
                self.cursor_x = vp_x;
                self.cursor_y += FONT_HEIGHT;
                Self::scroll_if_needed(fb, vp_x, vp_y, vp_w, vp_h, &mut self.cursor_y, bg);
            }

            fb.draw_char(self.cursor_x, self.cursor_y, ch, fg, Some(bg), 1);
            self.cursor_x += FONT_WIDTH;
        }
    }

    fn scroll_if_needed(
        fb: &mut Framebuffer,
        vp_x: usize,
        vp_y: usize,
        vp_w: usize,
        vp_h: usize,
        cursor_y: &mut usize,
        bg: Color,
    ) {
        if *cursor_y + FONT_HEIGHT > vp_y + vp_h {
            fb.scroll_region(vp_x, vp_y, vp_w, vp_h, FONT_HEIGHT, bg);
            *cursor_y = cursor_y.saturating_sub(FONT_HEIGHT);
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
        con.fg = crate::gop::Color::EMERALD;
        let _ = core::fmt::write(&mut *con, format_args!("[OK   ] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::WHITE;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! kinfo {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::CYAN;
        let _ = core::fmt::write(&mut *con, format_args!("[INFO ] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::WHITE;
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
        con.fg = crate::gop::Color::WHITE;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}

#[macro_export]
macro_rules! kerror {
    ($subsys:expr, $($arg:tt)*) => {{
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::CRIMSON;
        let _ = core::fmt::write(&mut *con, format_args!("[ERROR] [{:<7}] ", $subsys));
        con.fg = crate::gop::Color::WHITE;
        let _ = core::fmt::write(&mut *con, format_args!($($arg)*));
        let _ = core::fmt::write(&mut *con, format_args!("\n"));
    }};
}
