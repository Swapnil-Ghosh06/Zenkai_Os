//! Zenkai OS - UEFI Mouse & Absolute Pointer Driver + Hardware-Like Cursor
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Supports QEMU USB-Tablet (AbsolutePointer) and PS/2 Mouse (SimplePointer)
//! with a flicker-free save-under 12x19 macOS-style arrow cursor.

use uefi::boot::ScopedProtocol;
use uefi::proto::console::pointer::Pointer;

use crate::gop::{Color, Framebuffer};

const CUR_W: usize = 12;
const CUR_H: usize = 19;

// 0 = transparent, 1 = crisp white border, 2 = dark obsidian fill
#[rustfmt::skip]
const CURSOR_SHAPE: [[u8; CUR_W]; CUR_H] = [
    [1,0,0,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0,0,0],
    [1,2,2,1,0,0,0,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0,0,0],
    [1,2,2,2,2,1,0,0,0,0,0,0],
    [1,2,2,2,2,2,1,0,0,0,0,0],
    [1,2,2,2,2,2,2,1,0,0,0,0],
    [1,2,2,2,2,2,2,2,1,0,0,0],
    [1,2,2,2,2,2,2,2,2,1,0,0],
    [1,2,2,2,2,2,2,2,2,2,1,0],
    [1,2,2,2,2,2,2,1,1,1,1,1],
    [1,2,2,2,1,2,2,1,0,0,0,0],
    [1,2,2,1,0,1,2,2,1,0,0,0],
    [1,2,1,0,0,1,2,2,1,0,0,0],
    [1,1,0,0,0,0,1,2,2,1,0,0],
    [1,0,0,0,0,0,1,2,2,1,0,0],
    [0,0,0,0,0,0,0,1,2,1,0,0],
    [0,0,0,0,0,0,0,0,1,0,0,0],
];

pub struct MouseDriver {
    rel: Option<ScopedProtocol<Pointer>>,
    pub x: usize,
    pub y: usize,
    pub max_w: usize,
    pub max_h: usize,
    pub left_down: bool,
    pub prev_left: bool,
    pub moved: bool,
    save_under: [u32; CUR_W * CUR_H],
    saved: bool,
    drawn_x: usize,
    drawn_y: usize,
}

impl MouseDriver {
    pub fn init(max_w: usize, max_h: usize) -> Self {
        let rel = uefi::boot::get_handle_for_protocol::<Pointer>()
            .ok()
            .and_then(|h| uefi::boot::open_protocol_exclusive::<Pointer>(h).ok());

        Self {
            rel,
            x: max_w / 2,
            y: max_h / 2,
            max_w: max_w.max(1),
            max_h: max_h.max(1),
            left_down: false,
            prev_left: false,
            moved: false,
            save_under: [0; CUR_W * CUR_H],
            saved: false,
            drawn_x: 0,
            drawn_y: 0,
        }
    }

    /// Polls UEFI SimplePointer protocol for new movement or button clicks.
    pub fn poll(&mut self) -> bool {
        self.prev_left = self.left_down;
        self.moved = false;
        let old_x = self.x;
        let old_y = self.y;

        if let Some(rel) = &mut self.rel {
            while let Ok(Some(state)) = rel.read_state() {
                let dx = (state.relative_movement[0] / 2) as isize;
                let dy = (state.relative_movement[1] / 2) as isize;
                let nx = (self.x as isize + dx).clamp(0, (self.max_w - 1) as isize) as usize;
                let ny = (self.y as isize + dy).clamp(0, (self.max_h - 1) as isize) as usize;
                self.x = nx;
                self.y = ny;
                self.left_down = state.button[0];
            }
        }

        if self.x != old_x || self.y != old_y {
            self.moved = true;
        }
        self.moved || (self.left_down != self.prev_left)
    }

    pub fn just_clicked(&self) -> bool {
        self.left_down && !self.prev_left
    }

    pub fn in_rect(&self, rx: usize, ry: usize, rw: usize, rh: usize) -> bool {
        self.x >= rx && self.x < rx + rw && self.y >= ry && self.y < ry + rh
    }

    pub fn hide(&mut self, fb: &mut Framebuffer) {
        if !self.saved {
            return;
        }
        for r in 0..CUR_H {
            for c in 0..CUR_W {
                let px = self.drawn_x + c;
                let py = self.drawn_y + r;
                if px < fb.width && py < fb.height && CURSOR_SHAPE[r][c] != 0 {
                    fb.write_raw_pixel(px, py, self.save_under[r * CUR_W + c]);
                }
            }
        }
        self.saved = false;
    }

    pub fn draw(&mut self, fb: &mut Framebuffer) {
        self.drawn_x = self.x;
        self.drawn_y = self.y;
        for r in 0..CUR_H {
            for c in 0..CUR_W {
                let px = self.drawn_x + c;
                let py = self.drawn_y + r;
                let cell = CURSOR_SHAPE[r][c];
                if px < fb.width && py < fb.height && cell != 0 {
                    self.save_under[r * CUR_W + c] = fb.read_raw_pixel(px, py);
                    let col = if cell == 1 {
                        Color::TEXT_PRIMARY
                    } else {
                        Color::SHADOW
                    };
                    fb.set_pixel(px, py, col);
                }
            }
        }
        self.saved = true;
    }
}
