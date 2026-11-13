//! Global logger. One `info!("target", "key={}", v)` call formats a single structured line,
//! `[ssss.uuuuuu] LEVEL target message`, and writes it to every active sink: an in-memory ring,
//! the 16550 UART, the QEMU debug console, and the framebuffer console (or the firmware console
//! before the framebuffer exists).

use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::arch::{self, Uart16550};
use crate::fb::Console;
use crate::global::Global;
use crate::logfmt::{self, Level, LineBuf, Ring};
use crate::uefi::TextOut;

/// A destination for finished log lines.
pub trait Sink {
    fn line(&mut self, level: Level, bytes: &[u8]);
}

impl Sink for Uart16550 {
    fn line(&mut self, _level: Level, bytes: &[u8]) {
        self.write_raw(bytes);
    }
}

pub struct DebugCon;

impl Sink for DebugCon {
    fn line(&mut self, _level: Level, bytes: &[u8]) {
        arch::debugcon_write(bytes);
    }
}

impl Sink for Console {
    fn line(&mut self, level: Level, bytes: &[u8]) {
        self.write_line(level, bytes);
    }
}

/// Firmware text console. Only valid while boot services are alive.
pub struct ConOut(*mut TextOut);

impl Sink for ConOut {
    fn line(&mut self, _level: Level, bytes: &[u8]) {
        let mut buf = [0u16; 128];
        let mut n = 0;
        for &b in bytes {
            let units: &[u16] = if b == b'\n' { &[b'\r' as u16, b'\n' as u16] } else { &[b as u16] };
            if n + units.len() >= buf.len() {
                self.flush(&mut buf, &mut n);
            }
            for &u in units {
                buf[n] = u;
                n += 1;
            }
        }
        self.flush(&mut buf, &mut n);
    }
}

impl ConOut {
    fn flush(&self, buf: &mut [u16; 128], n: &mut usize) {
        if *n == 0 {
            return;
        }
        buf[*n] = 0;
        // SAFETY: `self.0` was non-null and live when attached; the logger drops this sink before
        // ExitBootServices. `buf` is a NUL-terminated UTF-16 string.
        unsafe { ((*self.0).output_string)(self.0, buf.as_ptr()) };
        *n = 0;
    }
}

struct Logger {
    tsc_base: u64,
    tsc_hz: u64,
    uart: Option<Uart16550>,
    debugcon: bool,
    fb: Option<Console>,
    conout: Option<ConOut>,
}

/// Kept apart from `Logger` so the all-zero ring lands in .bss instead of the image.
static RING: Global<Ring<65536>> = Global::new(Ring::new());

static LOGGER: Global<Logger> = Global::new(Logger {
    tsc_base: 0,
    tsc_hz: 0,
    uart: None,
    debugcon: false,
    fb: None,
    conout: None,
});

static BUSY: AtomicBool = AtomicBool::new(false);
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Debug as u8);

/// Runs `f` with exclusive access to the logger, or returns `None` when called re-entrantly.
fn with_logger<R>(f: impl FnOnce(&mut Logger) -> R) -> Option<R> {
    // Interrupts stay off for the whole call: a preempting task could otherwise find the logger busy
    // and silently lose its message.
    arch::without_interrupts(|| {
        if BUSY.swap(true, Ordering::Acquire) {
            return None;
        }
        // SAFETY: BUSY was clear, so no other `&mut Logger` exists; it is cleared again below.
        let r = f(unsafe { LOGGER.get_irqs_off() });
        BUSY.store(false, Ordering::Release);
        Some(r)
    })
}

pub fn init() {
    with_logger(|l| l.tsc_base = arch::rdtsc());
}

pub fn set_tsc_hz(hz: u64) {
    with_logger(|l| l.tsc_hz = hz);
}

pub fn tsc_hz() -> u64 {
    // SAFETY: plain read of an integer that is only written during early boot.
    unsafe { (*LOGGER.as_ptr()).tsc_hz }
}

pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Microseconds since `init`.
pub fn uptime_us() -> u64 {
    // SAFETY: plain reads of integers written only during early boot.
    let (base, hz) = unsafe { ((*LOGGER.as_ptr()).tsc_base, (*LOGGER.as_ptr()).tsc_hz) };
    logfmt::ticks_to_us(arch::rdtsc().wrapping_sub(base), hz)
}

pub fn enable_serial() -> bool {
    with_logger(|l| {
        l.uart = Uart16550::probe_com1();
        l.uart.is_some()
    })
    .unwrap_or(false)
}

pub fn enable_debugcon() -> bool {
    let present = arch::debugcon_present();
    with_logger(|l| l.debugcon = present);
    present
}

pub fn attach_console(c: Console) {
    with_logger(|l| l.fb = Some(c));
}

/// Mutable access to the framebuffer console (for the status bar and key-driven screens).
pub fn with_console<R>(f: impl FnOnce(&mut Console) -> R) -> Option<R> {
    with_logger(|l| l.fb.as_mut().map(f)).flatten()
}

pub fn attach_conout(p: *mut TextOut) {
    if !p.is_null() {
        with_logger(|l| l.conout = Some(ConOut(p)));
    }
}

/// Must be called before ExitBootServices: firmware console pointers die with boot services.
pub fn detach_conout() {
    with_logger(|l| l.conout = None);
}

/// Replays the in-memory ring (oldest first) into `f`.
pub fn replay(mut f: impl FnMut(&[u8])) {
    // Copy out so `f` may itself log without hitting the re-entrancy guard.
    let mut chunk = [0u8; 256];
    let mut pos = 0usize;
    loop {
        let n = with_logger(|_| {
            // SAFETY: BUSY is held by `with_logger`, so no other reference to the ring exists.
            let (a, b) = unsafe { RING.get_irqs_off() }.slices();
            let total = a.len() + b.len();
            if pos >= total {
                return 0;
            }
            let take = (total - pos).min(chunk.len());
            for (i, slot) in chunk.iter_mut().take(take).enumerate() {
                let idx = pos + i;
                *slot = if idx < a.len() { a[idx] } else { b[idx - a.len()] };
            }
            take
        })
        .unwrap_or(0);
        if n == 0 {
            break;
        }
        f(&chunk[..n]);
        pos += n;
    }
}

pub fn log(level: Level, target: &str, args: fmt::Arguments) {
    if level as u8 > MAX_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let mut line = LineBuf::<256>::new();
    let us = uptime_us();
    let _ = logfmt::write_prefix(&mut line, us, level, target);
    let _ = fmt::write(&mut line, args);
    let bytes = line.finish();
    with_logger(|l| {
        // SAFETY: BUSY is held by `with_logger`, so no other reference to the ring exists.
        unsafe { RING.get_irqs_off() }.push(bytes);
        if let Some(u) = l.uart.as_mut() {
            u.line(level, bytes);
        }
        if l.debugcon {
            DebugCon.line(level, bytes);
        }
        if let Some(c) = l.fb.as_mut() {
            c.line(level, bytes);
        } else if let Some(c) = l.conout.as_mut() {
            c.line(level, bytes);
        }
    });
}

/// Writes text with no prefix or timestamp to the UART, debug console and screen (shell output).
/// It is not stored in the log ring.
pub fn print_raw(bytes: &[u8]) {
    with_logger(|l| {
        if let Some(u) = l.uart.as_mut() {
            u.write_raw(bytes);
        }
        if l.debugcon {
            arch::debugcon_write(bytes);
        }
        if let Some(c) = l.fb.as_mut() {
            c.write_line(Level::Info, bytes);
        } else if let Some(c) = l.conout.as_mut() {
            c.line(Level::Info, bytes);
        }
    });
}

/// Releases the re-entrancy guard after a fault so the fault report can be printed.
///
/// The code that was interrupted may have been in the middle of a log write, so after this the logger
/// can be in a half-updated state. That is only acceptable because the caller halts the machine when it
/// is done: it must never return to the interrupted code, which would still believe it owns the logger.
/// Interrupts are switched off here so nothing else can run in between either (the rule is enforced,
/// not just promised).
pub fn reset_after_fault() {
    arch::disable_interrupts();
    BUSY.store(false, Ordering::SeqCst);
}

/// Writes straight to the serial and debug ports, bypassing every lock and buffer. For the
/// nested-fault and nested-panic paths where the normal logger may be in an unknown state.
pub fn raw_panic_line(s: &str) {
    let lg = LOGGER.as_ptr();
    // SAFETY: only used on paths that halt immediately afterwards; the UART state is plain data and
    // a torn update at worst garbles one line.
    unsafe {
        if let Some(u) = (*lg).uart.as_mut() {
            u.write_raw(s.as_bytes());
        }
        if (*lg).debugcon {
            arch::debugcon_write(s.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! error {
    ($t:expr_2021, $($a:tt)*) => { $crate::log::log($crate::logfmt::Level::Error, $t, format_args!($($a)*)) };
}
#[macro_export]
macro_rules! warn {
    ($t:expr_2021, $($a:tt)*) => { $crate::log::log($crate::logfmt::Level::Warn, $t, format_args!($($a)*)) };
}
#[macro_export]
macro_rules! info {
    ($t:expr_2021, $($a:tt)*) => { $crate::log::log($crate::logfmt::Level::Info, $t, format_args!($($a)*)) };
}
#[macro_export]
macro_rules! debug {
    ($t:expr_2021, $($a:tt)*) => { $crate::log::log($crate::logfmt::Level::Debug, $t, format_args!($($a)*)) };
}
#[allow(unused_macros)]
#[macro_export]
macro_rules! trace {
    ($t:expr_2021, $($a:tt)*) => { $crate::log::log($crate::logfmt::Level::Trace, $t, format_args!($($a)*)) };
}
