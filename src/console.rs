//! Kernel Console and Diagnostics System (CA-6 Layer 2)
//!
//! Provides structured logging macros, color formatting, and unified
//! output to both the hardware UART serial port and UEFI text console.

use core::fmt::{self, Write};
use crate::serial::SerialPort;

pub struct KernelConsole;

impl Write for KernelConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        SerialPort::write_str(s);
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    let mut console = KernelConsole;
    let _ = console.write_fmt(args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        $crate::console::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n")
    };
    ($($arg:tt)*) => {
        $crate::console::_print(format_args!("{}\n", format_args!($($arg)*)))
    };
}

#[macro_export]
macro_rules! kok {
    ($subsys:expr, $($arg:tt)*) => {
        $crate::kprintln!("\x1b[32m[OK   ]\x1b[0m \x1b[1m[{:<8}]\x1b[0m {}", $subsys, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kinfo {
    ($subsys:expr, $($arg:tt)*) => {
        $crate::kprintln!("\x1b[36m[INFO ]\x1b[0m \x1b[1m[{:<8}]\x1b[0m {}", $subsys, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kwarn {
    ($subsys:expr, $($arg:tt)*) => {
        $crate::kprintln!("\x1b[33m[WARN ]\x1b[0m \x1b[1m[{:<8}]\x1b[0m {}", $subsys, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kerror {
    ($subsys:expr, $($arg:tt)*) => {
        $crate::kprintln!("\x1b[31m[ERROR]\x1b[0m \x1b[1m[{:<8}]\x1b[0m {}", $subsys, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! ktrace {
    ($subsys:expr, $($arg:tt)*) => {
        $crate::kprintln!("\x1b[35m[TRACE]\x1b[0m \x1b[1m[{:<8}]\x1b[0m {}", $subsys, format_args!($($arg)*))
    };
}
