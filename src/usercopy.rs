//! Validation of pointers that user code hands to the kernel. Pure, host-testable.

/// True when `[ptr, ptr + len)` lies entirely inside the user window `[start, end)`. A zero length is
/// fine as long as the pointer is inside the window or exactly at its end.
pub fn check(start: u64, end: u64, ptr: u64, len: u64) -> bool {
    match ptr.checked_add(len) {
        Some(stop) => ptr >= start && stop <= end,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 0x4000_0000;
    const E: u64 = 0x4001_0000;

    #[test]
    fn rejects_overflow() {
        assert!(!check(S, E, u64::MAX - 1, 8));
        assert!(!check(S, E, S, u64::MAX));
    }

    #[test]
    fn rejects_kernel_range() {
        assert!(!check(S, E, 0, 1));
        assert!(!check(S, E, 0x10_0000, 16));
        assert!(!check(S, E, 0xFFFF_8000_0000_0000, 1));
        assert!(!check(S, E, S - 1, 1));
    }

    #[test]
    fn rejects_straddle() {
        assert!(!check(S, E, S - 4, 8));
        assert!(!check(S, E, E - 4, 8));
    }

    #[test]
    fn accepts_zero_len_inside() {
        assert!(check(S, E, S, 0));
        assert!(check(S, E, E, 0));
        assert!(!check(S, E, E + 1, 0));
        assert!(!check(S, E, 0, 0));
    }

    #[test]
    fn accepts_exact_window() {
        assert!(check(S, E, S, E - S));
        assert!(!check(S, E, S, E - S + 1));
    }

    #[test]
    fn empty_window_accepts_nothing() {
        assert!(!check(0, 0, 1, 1));
    }
}
