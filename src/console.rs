//! Zenkai OS - Dual Graphical & Serial Console
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Simultaneous rendering to UEFI Framebuffer and UART COM1 (0x3F8) Serial Port.

use core::fmt::{self, Write};
use crate::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::gop::{Color, Framebuffer};
use crate::serial::SerialPort;
use crate::sync::SpinLock;

pub struct Console {
    pub fb: Option<Framebuffer>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub margin_x: usize,
    pub margin_y: usize,
    pub fg: Color,
    pub bg: Color,
}

impl Console {
    pub const fn new() -> Self {
        Self {
            fb: None,
            cursor_x: 20,
            cursor_y: 20,
            margin_x: 20,
            margin_y: 20,
            fg: Color::WHITE,
            bg: Color::BG_OBSIDIAN,
        }
    }

    pub fn set_framebuffer(&mut self, fb: Framebuffer) {
        self.cursor_x = self.margin_x;
        self.cursor_y = self.margin_y;
        self.fb = Some(fb);
    }

    pub fn clear(&mut self) {
        if let Some(fb) = &mut self.fb {
            fb.clear(self.bg);
        }
        self.cursor_x = self.margin_x;
        self.cursor_y = self.margin_y;
    }

    pub fn write_char(&mut self, ch: char) {
        // 1. Mirror to UART serial as valid UTF-8
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

        // 2. Render to Framebuffer if available
        let margin_x = self.margin_x;
        let margin_y = self.margin_y;
        let fg = self.fg;
        let bg = self.bg;

        if let Some(fb) = &mut self.fb {
            if ch == '\n' {
                self.cursor_x = margin_x;
                self.cursor_y += FONT_HEIGHT;
                Self::scroll_if_needed(fb, &mut self.cursor_y, margin_y, bg);
                return;
            } else if ch == '\r' {
                self.cursor_x = margin_x;
                return;
            } else if ch == '\x08' {
                // Backspace
                if self.cursor_x >= margin_x + FONT_WIDTH {
                    self.cursor_x -= FONT_WIDTH;
                    fb.fill_rect(self.cursor_x, self.cursor_y, FONT_WIDTH, FONT_HEIGHT, bg);
                }
                return;
            }

            // Word wrap
            if self.cursor_x + FONT_WIDTH >= fb.width - margin_x {
                self.cursor_x = margin_x;
                self.cursor_y += FONT_HEIGHT;
                Self::scroll_if_needed(fb, &mut self.cursor_y, margin_y, bg);
            }

            fb.draw_char(self.cursor_x, self.cursor_y, ch, fg, Some(bg), 1);
            self.cursor_x += FONT_WIDTH;
        }
    }

    fn scroll_if_needed(fb: &mut Framebuffer, cursor_y: &mut usize, margin_y: usize, bg: Color) {
        if *cursor_y + FONT_HEIGHT >= fb.height - margin_y {
            let scroll_lines = FONT_HEIGHT;
            fb.scroll_up(scroll_lines, bg);
            *cursor_y = cursor_y.saturating_sub(scroll_lines);
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
