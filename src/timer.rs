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
        let cnt: u64;
        let freq: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nomem, nostack, preserves_flags));
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack, preserves_flags));
        }
        let f = freq.max(1);
        // Scale ARM generic timer ticks to an equivalent 2.4 GHz cycle count without u64 overflow
        let secs = cnt / f;
        let rem = cnt % f;
        secs.saturating_mul(2_400_000_000)
            .saturating_add((rem.saturating_mul(2_400_000_000)) / f)
    }
}

/// Sleeps for `ms` milliseconds using UEFI BootServices stall.
pub fn sleep_ms(ms: usize) {
    uefi::boot::stall(ms * 1000);
}

