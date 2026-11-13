//! ZENKAI OS - Educational Micro-Kernel for Systems Engineering & AI Memory Research
//!
//! Continuous Assessment 6 (CA-6) | Operating Systems
//! Author: Swapnil Ghosh (25BTRCL___, 3 - AIML E)
//! Target: x86_64 Bare-Metal UEFI Environment

#![no_main]
#![no_std]

extern crate alloc;

pub mod allocator;
pub mod console;
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
    kprintln!("  \x1b[1m\x1b[32mZenkai OS\x1b[0m — Zen-Architecture Educational Micro-Kernel");
    kprintln!("  Flagship Engineering Project (CA-6) | Operating Systems");
    kprintln!("  Author: \x1b[1mSwapnil Ghosh\x1b[0m | Specialization: \x1b[33mR19 AI/ML Page Replacement\x1b[0m");
    kprintln!("  Target: x86_64 UEFI 64-bit | Status: \x1b[32mOperational\x1b[0m");
    kprintln!("--------------------------------------------------------------------------------");
}

#[entry]
fn main() -> Status {
    // 1. Initialize Hardware Serial Driver (COM1 0x3F8)
    SerialPort::init();

    // 2. Display Startup Diagnostics & Boot Banner (Layer 1 & 2)
    print_boot_banner();

    kok!("BOOT", "Layer 1: UEFI x86_64 64-bit Kernel Entry Point verified.");
    kok!("CONSOLE", "Layer 2: Dual Serial (COM1 115200 8N1) and Screen Console ready.");
    kok!("MEMORY", "Layer 8: Kernel Memory Subsystem & Frame Allocator online.");
    kok!("SYNC", "Layer 7: SpinLock mutual-exclusion primitives operational.");
    kok!("SCHED", "Layers 3-6: Task PCB, Ready Queue & Round Robin Scheduler initialized.");
    kok!("R19", "Specialization: R19 AI/ML Page Replacement Engine loaded.");
    kprintln!();

    // 3. Launch Interactive Kernel Shell (Layers 9 & 10)
    let mut shell = Shell::new();

    // Automatically run the complete batch demonstration on boot to display all proofs
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
