//! Local APIC timer: replaces the legacy PIC/PIT tick when the CPU has an xAPIC. Falls back (returns
//! false) for anything unusual, and the caller keeps the PIT.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::apicmath;
use crate::arch;
use crate::log;
use crate::paging;

const MSR_APIC_BASE: u32 = 0x1B;
const BASE_ENABLE: u64 = 1 << 11;
const BASE_X2APIC: u64 = 1 << 10;
const BASE_ADDR: u64 = 0x000F_FFFF_FFFF_F000;

const REG_EOI: usize = 0xB0;
const REG_SVR: usize = 0xF0;
const REG_LVT_TIMER: usize = 0x320;
const REG_INIT_COUNT: usize = 0x380;
const REG_CUR_COUNT: usize = 0x390;
const REG_DIVIDE: usize = 0x3E0;

const SVR_ENABLE: u32 = 1 << 8;
const LVT_MASKED: u32 = 1 << 16;
const LVT_PERIODIC: u32 = 1 << 17;
/// Divide the bus clock by 16.
const DIVIDE_BY_16: u32 = 0x3;
/// Vector the timer fires on: the same one the PIT used, so the existing stub and gate are reused.
pub const TIMER_VECTOR: u32 = 32;
/// Vector for spurious interrupts (the low four bits must be 1111).
pub const SPURIOUS_VECTOR: u32 = 0xFF;

/// Address of the LAPIC registers (identity mapped) once the timer runs, else 0.
static BASE: AtomicU64 = AtomicU64::new(0);

fn read(base: u64, reg: usize) -> u32 {
    // SAFETY: `base` is a UC-mapped LAPIC page (checked by `start_timer`) and `reg` is a register offset below 4 KiB.
    unsafe { read_volatile((base as usize + reg) as *const u32) }
}

fn write(base: u64, reg: usize, v: u32) {
    // SAFETY: as in `read`.
    unsafe { write_volatile((base as usize + reg) as *mut u32, v) };
}

/// True once the LAPIC timer is the tick source.
pub fn active() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

/// Acknowledges the timer interrupt at the LAPIC. Returns false when the LAPIC is not in use.
pub fn eoi() -> bool {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return false;
    }
    write(base, REG_EOI, 0);
    true
}

/// Calibrates the LAPIC timer against the TSC and starts it in periodic mode at `hz`, then masks the
/// 8259 PICs. Returns false (and leaves the PIC/PIT path untouched) if the LAPIC is disabled, in x2APIC
/// mode, cannot be mapped uncached, or calibration gives an implausible value.
///
/// # Safety
/// Interrupts must be off, the IDT must route vector 32 to the tick stub and `SPURIOUS_VECTOR` to a
/// stub that just returns, and paging must be active.
pub unsafe fn start_timer(hz: u64) -> bool {
    let msr = arch::rdmsr(MSR_APIC_BASE);
    if msr & BASE_ENABLE == 0 || msr & BASE_X2APIC != 0 {
        return false;
    }
    let base = msr & BASE_ADDR;
    let tsc_hz = log::tsc_hz();
    if base == 0 || tsc_hz == 0 || !paging::map_mmio_uc(base) {
        return false;
    }
    write(base, REG_SVR, SVR_ENABLE | SPURIOUS_VECTOR);
    write(base, REG_DIVIDE, DIVIDE_BY_16);
    write(base, REG_LVT_TIMER, LVT_MASKED | TIMER_VECTOR);
    write(base, REG_INIT_COUNT, u32::MAX);
    let t0 = arch::rdtsc();
    let window = tsc_hz / 100; // 10 ms
    while arch::rdtsc().wrapping_sub(t0) < window {
        arch::pause();
    }
    let cur = read(base, REG_CUR_COUNT);
    let elapsed = arch::rdtsc().wrapping_sub(t0);
    write(base, REG_INIT_COUNT, 0);
    let Some(count) = apicmath::period_count(u64::from(u32::MAX - cur), elapsed, tsc_hz, hz) else {
        return false;
    };
    // SAFETY: caller contract: interrupts off.
    unsafe { arch::mask_pic() };
    BASE.store(base, Ordering::Relaxed);
    write(base, REG_LVT_TIMER, LVT_PERIODIC | TIMER_VECTOR);
    write(base, REG_INIT_COUNT, count);
    true
}
