//! Boot counting, in the spirit of systemd-boot's automatic boot assessment: the counter is raised
//! at the start of every boot and cleared once the kernel is up. Several boots in a row that never
//! reach that point mean something in the normal path is failing, so the next boot enters recovery
//! mode (log on screen, menu waits, no timer or scheduler). Pure, host-testable.

/// Failed boots in a row before recovery mode starts on its own.
pub const MAX_FAILED: u8 = 3;

/// What to do for this boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Boot in recovery mode.
    pub recovery: bool,
    /// New counter value to store now, if counting is on.
    pub write: Option<u8>,
}

/// Reads the stored counter: absent, empty or oversized data counts as zero.
pub fn decode(raw: Option<&[u8]>) -> u8 {
    match raw {
        Some([b]) => *b,
        _ => 0,
    }
}

pub fn decide(count: u8, nocount: bool) -> Decision {
    if nocount {
        return Decision { recovery: false, write: None };
    }
    Decision { recovery: count >= MAX_FAILED, write: Some(count.saturating_add(1)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_boots_are_normal_and_count_up() {
        assert_eq!(decide(0, false), Decision { recovery: false, write: Some(1) });
        assert_eq!(decide(1, false), Decision { recovery: false, write: Some(2) });
        assert_eq!(decide(2, false), Decision { recovery: false, write: Some(3) });
    }

    #[test]
    fn after_max_failed_boots_recovery_starts() {
        assert!(decide(MAX_FAILED, false).recovery);
        assert!(decide(200, false).recovery);
    }

    #[test]
    fn counter_saturates() {
        assert_eq!(decide(255, false).write, Some(255));
    }

    #[test]
    fn nocount_never_recovers_and_never_writes() {
        assert_eq!(decide(9, true), Decision { recovery: false, write: None });
    }

    #[test]
    fn garbage_variable_reads_as_zero() {
        assert_eq!(decode(None), 0);
        assert_eq!(decode(Some(&[])), 0);
        assert_eq!(decode(Some(&[1, 2])), 0);
        assert_eq!(decode(Some(&[2])), 2);
    }
}
