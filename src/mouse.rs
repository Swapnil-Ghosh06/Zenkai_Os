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
    ps2_left: bool,
    ps2_enabled: bool,
    ps2_packet: [u8; 3],
    ps2_idx: usize,
    clicked_this_poll: bool,
    pub moved: bool,
    pub scroll_delta: isize,
    save_under: [u32; CUR_W * CUR_H],
    saved: bool,
    drawn_x: usize,
    drawn_y: usize,
}

#[cfg(target_arch = "x86_64")]
mod ps2 {
    #[inline(always)]
    pub unsafe fn inb(port: u16) -> u8 {
        let val: u8;
        core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags));
        val
    }

    #[inline(always)]
    pub unsafe fn outb(port: u16, val: u8) {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    }

    unsafe fn wait_write() -> bool {
        for _ in 0..50_000 {
            if (inb(0x64) & 0x02) == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    unsafe fn wait_read_aux() -> Option<u8> {
        for _ in 0..50_000 {
            let st = inb(0x64);
            if (st & 0x21) == 0x21 {
                return Some(inb(0x60));
            }
            core::hint::spin_loop();
        }
        None
    }

    unsafe fn write_mouse(cmd: u8) -> bool {
        if !wait_write() { return false; }
        outb(0x64, 0xD4);
        if !wait_write() { return false; }
        outb(0x60, cmd);
        wait_read_aux().is_some()
    }

    /// Initializes the 8042 auxiliary PS/2 mouse port in polled mode (IRQ12 disabled so it
    /// never disturbs UEFI interrupts, and we only read port 0x60 when status bit 5 AUX is set).
    pub fn init() -> bool {
        unsafe {
            // Drain any stale AUX bytes only (never touch keyboard bytes!)
            for _ in 0..32 {
                if (inb(0x64) & 0x21) == 0x21 {
                    let _ = inb(0x60);
                } else {
                    break;
                }
            }
            // Enable auxiliary (mouse) device on 8042 controller
            if !wait_write() { return false; }
            outb(0x64, 0xA8);

            // Read controller configuration byte (0x20), clear bit 1 (IRQ12) and bit 5 (AUX clock disable)
            if !wait_write() { return false; }
            outb(0x64, 0x20);
            let mut cfg = 0u8;
            for _ in 0..50_000 {
                if (inb(0x64) & 0x01) != 0 {
                    cfg = inb(0x60);
                    break;
                }
                core::hint::spin_loop();
            }
            cfg &= !0x02; // disable IRQ12 (we poll safely!)
            cfg &= !0x20; // enable AUX clock
            if !wait_write() { return false; }
            outb(0x64, 0x60);
            if !wait_write() { return false; }
            outb(0x60, cfg);

            // Set defaults (0xF6) and Enable Data Reporting (0xF4)
            let _ = write_mouse(0xF6);
            write_mouse(0xF4)
        }
    }

    #[inline(always)]
    pub fn read_aux_byte() -> Option<u8> {
        unsafe {
            let st = inb(0x64);
            if (st & 0x21) == 0x21 {
                Some(inb(0x60))
            } else {
                None
            }
        }
    }
}

impl MouseDriver {
    fn connect_usb_and_input_controllers() {
        if let Ok(handles) = uefi::boot::locate_handle_buffer(uefi::boot::SearchType::AllHandles) {
            for &h in handles.iter() {
                let _ = uefi::boot::connect_controller(h, None, None, true);
            }
        }
    }

    pub fn init(max_w: usize, max_h: usize) -> Self {
        Self::connect_usb_and_input_controllers();

        let mut rels = Vec::new();
        if let Ok(handles) = uefi::boot::find_handles::<Pointer>() {
            for h in handles {
                if let Some(mut p) = open_best::<Pointer>(h) {
                    let _ = p.reset(false);
                    rels.push(p);
                }
            }
        }
        if rels.is_empty() {
            if let Ok(h) = uefi::boot::get_handle_for_protocol::<Pointer>() {
                if let Some(mut p) = open_best::<Pointer>(h) {
                    let _ = p.reset(false);
                    rels.push(p);
                }
            }
        }

        let mut abss = Vec::new();
        if let Ok(handles) = uefi::boot::find_handles::<AbsolutePointer>() {
            for h in handles {
                if let Some(mut p) = open_best::<AbsolutePointer>(h) {
                    unsafe {
                        let _ = (p.reset)(&mut *p as *mut AbsolutePointer, 0);
                    }
                    abss.push(p);
                }
            }
        }

        #[cfg(target_arch = "x86_64")]
        let ps2_enabled = ps2::init();
        #[cfg(not(target_arch = "x86_64"))]
        let ps2_enabled = false;

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
            ps2_left: false,
            ps2_enabled,
            ps2_packet: [0; 3],
            ps2_idx: 0,
            clicked_this_poll: false,
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
        self.clicked_this_poll = false;
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
                    let now_down = (st.active_buttons & 1) != 0;
                    if now_down && !self.abs_left {
                        self.clicked_this_poll = true;
                    }
                    self.abs_left = now_down;
                }
            }
        }

        for rel in &mut self.rels {
            let mode = *rel.mode();
            let res_x = (mode.resolution[0] as isize).max(1);
            let res_y = (mode.resolution[1] as isize).max(1);
            let res_z = mode.resolution[2] as isize;
            while let Ok(Some(state)) = rel.read_state() {
                let raw_x = state.relative_movement[0] as isize;
                let raw_y = state.relative_movement[1] as isize;
                let dx = if raw_x == 0 {
                    0
                } else {
                    let scaled = (raw_x * 2) / res_x.min(4);
                    if scaled == 0 { raw_x.signum() } else { scaled }
                };
                let dy = if raw_y == 0 {
                    0
                } else {
                    let scaled = (raw_y * 2) / res_y.min(4);
                    if scaled == 0 { raw_y.signum() } else { scaled }
                };
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
                let now_down = state.button[0];
                if now_down && !self.rel_left {
                    self.clicked_this_poll = true;
                }
                self.rel_left = now_down;
            }
        }

        #[cfg(target_arch = "x86_64")]
        if self.ps2_enabled {
            while let Some(b) = ps2::read_aux_byte() {
                // Bit 3 of byte 0 in standard PS/2 mouse packet is ALWAYS 1 (sync check)
                if self.ps2_idx == 0 && (b & 0x08) == 0 {
                    continue;
                }
                self.ps2_packet[self.ps2_idx] = b;
                self.ps2_idx += 1;
                if self.ps2_idx == 3 {
                    self.ps2_idx = 0;
                    let flags = self.ps2_packet[0];
                    // Ignore overflow packets
                    if (flags & 0xC0) == 0 {
                        let dx = (self.ps2_packet[1] as i8) as isize;
                        let dy = -((self.ps2_packet[2] as i8) as isize); // PS/2 Y is inverted (+up)
                        if dx != 0 || dy != 0 {
                            self.x = (self.x as isize + dx).clamp(0, (self.max_w - 1) as isize) as usize;
                            self.y = (self.y as isize + dy).clamp(0, (self.max_h - 1) as isize) as usize;
                        }
                        let now_down = (flags & 0x01) != 0;
                        if now_down && !self.ps2_left {
                            self.clicked_this_poll = true;
                        }
                        self.ps2_left = now_down;
                    }
                }
            }
        }

        self.left_down = self.abs_left || self.rel_left || self.ps2_left;

        if self.x != old_x || self.y != old_y {
            self.moved = true;
        }
        self.moved || self.clicked_this_poll || (self.left_down != self.prev_left) || self.scroll_delta != 0
    }

    pub fn just_clicked(&self) -> bool {
        self.clicked_this_poll || (self.left_down && !self.prev_left)
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
                        Color::GLASS_TEXT
                    } else {
                        Color::INK_SOLID
                    };
                    fb.set_pixel(px, py, col);
                }
            }
        }
        self.saved = true;
    }
}
