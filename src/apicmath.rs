//! Local APIC timer arithmetic. Pure, host-testable.

/// Initial-count value that makes the LAPIC timer fire `hz` times a second.
///
/// During calibration the timer counted down `counted` ticks while the TSC advanced `tsc_elapsed`
/// ticks at `tsc_hz`. Returns `None` for any input that is zero or gives a count outside 1..=u32::MAX.
pub fn period_count(counted: u64, tsc_elapsed: u64, tsc_hz: u64, hz: u64) -> Option<u32> {
    if counted == 0 || tsc_elapsed == 0 || tsc_hz == 0 || hz == 0 {
        return None;
    }
    // lapic_hz = counted * tsc_hz / tsc_elapsed; count = lapic_hz / hz. Done in u128 so nothing overflows.
    let count = (counted as u128 * tsc_hz as u128) / (tsc_elapsed as u128 * hz as u128);
    u32::try_from(count).ok().filter(|&c| c > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_values() {
        // 10 ms of a 1 GHz LAPIC bus clock on a 2 GHz TSC, 100 Hz target: 10 ms per tick.
        assert_eq!(period_count(10_000_000, 20_000_000, 2_000_000_000, 100), Some(10_000_000));
        // Half the TSC time for the same count means a twice-as-fast LAPIC clock.
        assert_eq!(period_count(10_000_000, 10_000_000, 2_000_000_000, 100), Some(20_000_000));
    }

    #[test]
    fn zero_inputs_are_none() {
        assert_eq!(period_count(0, 1, 1, 1), None);
        assert_eq!(period_count(1, 0, 1, 1), None);
        assert_eq!(period_count(1, 1, 0, 1), None);
        assert_eq!(period_count(1, 1, 1, 0), None);
    }

    #[test]
    fn out_of_range_is_none() {
        // Too slow to give even one count per period.
        assert_eq!(period_count(1, u64::MAX, 1, 100), None);
        // Does not fit 32 bits.
        assert_eq!(period_count(u64::MAX, 1, u64::MAX, 1), None);
    }

    #[test]
    fn no_overflow_at_the_extremes() {
        let _ = period_count(u64::MAX, u64::MAX, u64::MAX, u64::MAX);
        let _ = period_count(u64::MAX, 1, u64::MAX, 1);
    }
}
