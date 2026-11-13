//! Zenkai OS - Autonomous AI/ML Micro-Kernel & R19 Paging Engine
//!
//! Continuous Assessment 6 (CA-6) | Operating Systems
//! Author: Swapnil Ghosh (25BTRCL267, Class: AIML-E)
//! Target: x86_64 Bare-Metal UEFI Environment

#![no_main]
#![no_std]

extern crate alloc;

pub mod allocator;
pub mod boot_screen;
pub mod console;
pub mod desktop;
pub mod draw;
pub mod font;
pub mod gop;
pub mod hw;
pub mod memory;
pub mod pkg;
pub mod r19;
pub mod readyq;
pub mod sched;
pub mod serial;
pub mod shell;
pub mod sync;
pub mod task;
pub mod timer;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;

use crate::boot_screen::run_visual_boot_sequence;
use crate::console::GLOBAL_CONSOLE;
use crate::desktop::Desktop;
use crate::gop::Framebuffer;
use crate::hw::HardwareInfo;
use crate::pkg::SystemServices;
use crate::serial::SerialPort;
use crate::shell::Shell;

#[global_allocator]
static ALLOCATOR: uefi::allocator::Allocator = uefi::allocator::Allocator;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kerror!("PANIC", "==================== KERNEL PANIC ====================");
    if let Some(location) = info.location() {
        kerror!("PANIC", "Location: {}:{}:{}", location.file(), location.line(), location.column());
    }
    kerror!("PANIC", "Message : {}", info.message());
    kerror!("PANIC", "System halted. Execution suspended.");
    loop {
        core::hint::spin_loop();
    }
}

fn print_boot_banner() {
    kprintln!("  Zenkai OS 0.9.2 — Zen-Architecture Educational Micro-Kernel");
    kprintln!("  Flagship Engineering Project (CA-6) | Operating Systems");
    kprintln!("  Author: Swapnil Ghosh | Specialization: R19 AI/ML Page Replacement");
    kprintln!("  Roll No: 25BTRCL267 | Class: AIML-E");
    kprintln!("  Target: x86_64 UEFI 64-bit | Status: Operational");
    kprintln!("--------------------------------------------------------------------------------");
}

#[entry]
fn main() -> Status {
    // 1. Initialize Hardware Serial Port (COM1 0x3F8)
    SerialPort::init();
    kok!("BOOT", "Layer 1: UEFI x86_64 64-bit Kernel Entry Point verified.");

    // 2. Probe CPUID & PCI Hardware
    let mut hw = HardwareInfo::new();
    hw.probe();

    // 3. Initialize UEFI Graphics Output Protocol (GOP) Framebuffer
    let gop_init_result = (|| -> uefi::Result<Framebuffer> {
        let handle = uefi::boot::get_handle_for_protocol::<GraphicsOutput>()?;
        let mut gop = uefi::boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;
        let mode = gop.current_mode_info();
        let (width, height) = mode.resolution();
        let stride = mode.stride();
        let is_bgr = matches!(mode.pixel_format(), uefi::proto::console::gop::PixelFormat::Bgr);
        let mut fb = gop.frame_buffer();
        let base_ptr = fb.as_mut_ptr() as *mut u32;

        let framebuffer = unsafe { Framebuffer::from_raw(base_ptr, width, height, stride, is_bgr) };
        Ok(framebuffer)
    })();

    let has_gop = match gop_init_result {
        Ok(mut fb) => {
            hw.gop_width = fb.width;
            hw.gop_height = fb.height;
            kok!("GOP", "Layer 2: GOP Framebuffer detected ({}x{}, stride={})", fb.width, fb.height, fb.stride);

            run_visual_boot_sequence(&mut fb, &mut hw);

            // Attach framebuffer to console, keeping paint_fb=false during batch logging
            // so all logs go to UART serial + terminal history without flickering.
            {
                let mut con = GLOBAL_CONSOLE.lock();
                con.set_framebuffer(fb);
                con.paint_fb = false;
            }
            true
        }
        Err(err) => {
            kwarn!("GOP", "Running in headless terminal mode (GOP not present: {:?})", err);
            false
        }
    };

    // 4. Print Banner & Run Batch Verification (mirrors to UART COM1 & Terminal History)
    print_boot_banner();
    let mut shell = Shell::new();
    let mut services = SystemServices::new();
    shell.run_batch_verification();

    kprintln!();
    kinfo!("SHELL", "Entering interactive Zenkai OS Desktop Environment.");

    if has_gop {
        let mut desktop = Desktop::new();
        desktop.run(&mut shell, &mut services, &hw);
    }

    kok!("KERNEL", "Zenkai OS shutting down cleanly. Goodbye!");
    Status::SUCCESS
}
