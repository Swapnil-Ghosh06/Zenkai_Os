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
pub mod draw;
pub mod font;
pub mod gop;
pub mod memory;
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
use crate::gop::Framebuffer;
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
    kprintln!("\x1b[36m");
    kprintln!(r#"  ███████╗███████╗███╗   ██╗██╗  ██╗ █████╗ ██╗     ██████╗ ███████╗"#);
    kprintln!(r#"  ╚══███╔╝██╔════╝████╗  ██║██║ ██╔╝██╔══██╗██║    ██╔═══██╗██╔════╝"#);
    kprintln!(r#"    ███╔╝ █████╗  ██╔██╗ ██║█████╔╝ ███████║██║    ██║   ██║███████╗"#);
    kprintln!(r#"   ███╔╝  ██╔══╝  ██║╚██╗██║██╔═██╗ ██╔══██║██║    ██║   ██║╚════██║"#);
    kprintln!(r#"  ███████╗███████╗██║ ╚████║██║  ██╗██║  ██║██║    ╚██████╔╝███████║"#);
    kprintln!(r#"  ╚══════╝╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═════╝ ╚══════╝"#);
    kprintln!("\x1b[0m");
    kprintln!("  \x1b[1m\x1b[32mZenkai OS 0.9.2\x1b[0m — Zen-Architecture Educational Micro-Kernel");
    kprintln!("  Flagship Engineering Project (CA-6) | Operating Systems");
    kprintln!("  Author: \x1b[1mSwapnil Ghosh\x1b[0m | Specialization: \x1b[33mR19 AI/ML Page Replacement\x1b[0m");
    kprintln!("  Roll No: \x1b[33m25BTRCL267\x1b[0m | Class: \x1b[33mAIML-E\x1b[0m");
    kprintln!("  Target: x86_64 UEFI 64-bit | Status: \x1b[32mOperational\x1b[0m");
    kprintln!("--------------------------------------------------------------------------------");
}

#[entry]
fn main() -> Status {
    // 1. Initialize Hardware Serial Port (COM1 0x3F8)
    SerialPort::init();
    kok!("BOOT", "Layer 1: UEFI x86_64 64-bit Kernel Entry Point verified.");

    // 2. Initialize UEFI Graphics Output Protocol (GOP) Framebuffer
    let gop_init_result = (|| -> uefi::Result<Framebuffer> {
        let handle = uefi::boot::get_handle_for_protocol::<GraphicsOutput>()?;
        let mut gop = uefi::boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;
        let mode = gop.current_mode_info();
        let (width, height) = mode.resolution();
        let stride = mode.stride();
        let is_bgr = match mode.pixel_format() {
            uefi::proto::console::gop::PixelFormat::Bgr => true,
            _ => false,
        };
        let mut fb = gop.frame_buffer();
        let base_ptr = fb.as_mut_ptr() as *mut u32;

        let framebuffer = unsafe { Framebuffer::from_raw(base_ptr, width, height, stride, is_bgr) };
        Ok(framebuffer)
    })();

    match gop_init_result {
        Ok(mut fb) => {
            kok!("GOP", "Layer 2: GOP Framebuffer detected ({}x{}, stride={})", fb.width, fb.height, fb.stride);
            // Run gorgeous visual boot sequence with 5-stage progress bar & countdown
            run_visual_boot_sequence(&mut fb);

            // Clear screen and attach framebuffer to GLOBAL_CONSOLE
            fb.clear(crate::gop::Color::BG_OBSIDIAN);
            GLOBAL_CONSOLE.lock().set_framebuffer(fb);
        }
        Err(err) => {
            kwarn!("GOP", "Running in headless terminal mode (GOP not present: {:?})", err);
        }
    }

    // 3. Display Startup Diagnostics & Boot Banner
    print_boot_banner();

    // 4. Launch Interactive Kernel Shell
    let mut shell = Shell::new();

    // Automatically run batch verification to demonstrate all CA-6 layers and research on boot
    shell.run_batch_verification();

    kprintln!();
    kinfo!("SHELL", "Entering interactive shell mode. Type 'help' for commands.");
    kprintln!();

    loop {
        shell.print_prompt();
        let cmd = shell.read_command();
        if !shell.dispatch(&cmd) {
            break;
        }
    }

    kok!("KERNEL", "Zenkai OS shutting down cleanly. Goodbye!");
    Status::SUCCESS
}
