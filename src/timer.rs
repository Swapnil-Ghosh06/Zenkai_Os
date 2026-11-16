//! Zenkai OS - Hardware Timer & Cycle Counter
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! x86 rdtsc cycle counter and microsecond stall calibration.

#[inline(always)]
pub fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let low: u32;
        let high: u32;
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") low,
                out("edx") high,
                options(nomem, nostack, preserves_flags)
            );
        }
        ((high as u64) << 32) | (low as u64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        let val: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, cntvct_el0",
                out(reg) val,
                options(nomem, nostack, preserves_flags)
            );
        }
        val
    }
}

/// Sleeps for `ms` milliseconds using UEFI BootServices stall.
pub fn sleep_ms(ms: usize) {
    uefi::boot::stall(ms * 1000);
}

