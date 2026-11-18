//! Zenkai OS - UEFI Mouse & Absolute Pointer Driver + Hardware-Like Cursor
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Supports USB-Tablet (EFI_ABSOLUTE_POINTER_PROTOCOL) and PS/2 Mouse
//! (EFI_SIMPLE_POINTER_PROTOCOL) with scroll wheel delta and a flicker-free
//! save-under 12x19 macOS-style arrow cursor.

extern crate alloc;
use alloc::vec::Vec;
use core::ffi::c_void;

use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::unsafe_protocol;
use uefi::Status;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
struct AbsolutePointerMode {
    absolute_min_x: u64,
    absolute_min_y: u64,
    absolute_min_z: u64,
    absolute_max_x: u64,
    absolute_max_y: u64,
    absolute_max_z: u64,
    attributes: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
struct AbsolutePointerState {
    current_x: u64,
    current_y: u64,
    current_z: u64,
    active_buttons: u32,
}

#[repr(C)]
#[unsafe_protocol("8d59d32b-c655-4ae9-9b15-f25904992a43")]
struct AbsolutePointer {
    reset: unsafe extern "efiapi" fn(this: *mut Self, extended_verification: u8) -> Status,
    get_state: unsafe extern "efiapi" fn(this: *const Self, state: *mut AbsolutePointerState) -> Status,
    wait_for_input: *mut c_void,
    mode: *mut AbsolutePointerMode,
}

impl AbsolutePointer {
    fn read_state(&self) -> Option<AbsolutePointerState> {
        let mut st = AbsolutePointerState::default();
        let status = unsafe { (self.get_state)(self as *const Self, &mut st as *mut _) };
        if status == Status::SUCCESS {
            Some(st)
        } else {
            None
        }
    }

    fn mode_info(&self) -> Option<AbsolutePointerMode> {
        if self.mode.is_null() {
            None
        } else {
            Some(unsafe { *self.mode })
        }
    }
}

fn open_best<P: uefi::proto::ProtocolPointer>(handle: uefi::Handle) -> Option<ScopedProtocol<P>> {
    if let Ok(p) = uefi::boot::open_protocol_exclusive::<P>(handle) {
        return Some(p);
    }
    unsafe {
        uefi::boot::open_protocol::<P>(
            OpenProtocolParams {
                handle,
                agent: uefi::boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .ok()
    }
}

pub struct MouseDriver {
    rels: Vec<ScopedProtocol<Pointer>>,
    abss: Vec<ScopedProtocol<AbsolutePointer>>,
    pub x: usize,
    pub y: usize,
    pub max_w: usize,
    pub max_h: usize,
    pub left_down: bool,
    pub prev_left: bool,
    abs_left: bool,
    rel_left: bool,
    pub moved: bool,
    pub scroll_delta: isize,
    save_under: [u32; CUR_W * CUR_H],
    saved: bool,
    drawn_x: usize,
    drawn_y: usize,
}

impl MouseDriver {
    pub fn init(max_w: usize, max_h: usize) -> Self {
        let mut rels = Vec::new();
        if let Ok(handles) = uefi::boot::find_handles::<Pointer>() {
            for h in handles {
                if let Some(p) = open_best::<Pointer>(h) {
                    rels.push(p);
                }
            }
        }
        if rels.is_empty() {
            if let Ok(h) = uefi::boot::get_handle_for_protocol::<Pointer>() {
                if let Some(p) = open_best::<Pointer>(h) {
                    rels.push(p);
                }
            }
        }

        let mut abss = Vec::new();
        if let Ok(handles) = uefi::boot::find_handles::<AbsolutePointer>() {
            for h in handles {
                if let Some(p) = open_best::<AbsolutePointer>(h) {
                    abss.push(p);
                }
            }
        }

        Self {
            rels,
            abss,
            x: max_w / 2,
            y: max_h / 2,
            max_w: max_w.max(1),
            max_h: max_h.max(1),
            left_down: false,
            prev_left: false,
            abs_left: false,
            rel_left: false,
            moved: false,
            scroll_delta: 0,
            save_under: [0; CUR_W * CUR_H],
            saved: false,
            drawn_x: 0,
            drawn_y: 0,
        }
    }

    pub fn set_bounds(&mut self, max_w: usize, max_h: usize) {
        self.max_w = max_w.max(1);
        self.max_h = max_h.max(1);
        self.x = self.x.min(self.max_w - 1);
        self.y = self.y.min(self.max_h - 1);
    }

    /// Polls UEFI AbsolutePointer & SimplePointer protocols for motion, clicks, and Z-wheel scroll.
    pub fn poll(&mut self) -> bool {
        self.prev_left = self.left_down;
        self.moved = false;
        self.scroll_delta = 0;
        let old_x = self.x;
        let old_y = self.y;

        for abs in &mut self.abss {
            if let Some(m) = abs.mode_info() {
                while let Some(st) = abs.read_state() {
                    if m.absolute_max_x > m.absolute_min_x && m.absolute_max_y > m.absolute_min_y {
                        let span_x = (m.absolute_max_x - m.absolute_min_x).max(1);
                        let span_y = (m.absolute_max_y - m.absolute_min_y).max(1);
                        let cx = st.current_x.clamp(m.absolute_min_x, m.absolute_max_x) - m.absolute_min_x;
                        let cy = st.current_y.clamp(m.absolute_min_y, m.absolute_max_y) - m.absolute_min_y;
                        self.x = ((cx as u128 * (self.max_w.saturating_sub(1)) as u128) / span_x as u128) as usize;
                        self.y = ((cy as u128 * (self.max_h.saturating_sub(1)) as u128) / span_y as u128) as usize;
                    }
                    self.abs_left = (st.active_buttons & 1) != 0;
                }
            }
        }

        for rel in &mut self.rels {
            let res_z = rel.mode().resolution[2] as isize;
            while let Ok(Some(state)) = rel.read_state() {
                let dx = (state.relative_movement[0] / 2) as isize;
                let dy = (state.relative_movement[1] / 2) as isize;
                if dx != 0 || dy != 0 {
                    let nx = (self.x as isize + dx).clamp(0, (self.max_w - 1) as isize) as usize;
                    let ny = (self.y as isize + dy).clamp(0, (self.max_h - 1) as isize) as usize;
                    self.x = nx;
                    self.y = ny;
                }
                if state.relative_movement[2] != 0 {
                    let dz = state.relative_movement[2] as isize;
                    let step = if res_z > 0 {
                        let s = dz / res_z;
                        if s == 0 { dz.signum() } else { s }
                    } else {
                        dz.signum()
                    };
                    self.scroll_delta += step;
                }
                self.rel_left = state.button[0];
            }
        }

        self.left_down = self.abs_left || self.rel_left;

        if self.x != old_x || self.y != old_y {
            self.moved = true;
        }
        self.moved || (self.left_down != self.prev_left) || self.scroll_delta != 0
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
