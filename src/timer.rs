//! Hardware Timer and Tick Generator (CA-6 Layer 6)
//!
//! Provides cycle-accurate hardware timing using x86 RDTSC instructions
//! and simulated periodic timer ticks for preemptive quantum enforcement.

use core::arch::asm;

pub struct HardwareTimer;

impl HardwareTimer {
    /// Read the 64-bit x86 Time Stamp Counter (RDTSC)
    #[inline]
    pub fn read_tsc() -> u64 {
        let low: u32;
        let high: u32;
        unsafe {
            asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack));
        }
        ((high as u64) << 32) | (low as u64)
    }

    /// Spin-delay for approximately `cycles` CPU clock cycles
    pub fn delay_cycles(cycles: u64) {
        let start = Self::read_tsc();
        while Self::read_tsc().wrapping_sub(start) < cycles {
            core::hint::spin_loop();
        }
    }
}

