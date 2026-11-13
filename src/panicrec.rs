//! Last-panic record, in the spirit of Linux pstore: the panic and exception handlers write a short
//! text into a firmware variable, and the next boot reads it, shows it and deletes it. Needs only the
//! firmware runtime services, which are still callable after ExitBootServices.

use crate::boot;
use crate::uefi::{self, RuntimeServices};

const NAME: [u16; 14] = uefi::utf16(b"AeonLastPanic\0");
pub const MAX_LEN: usize = 160;

fn rt() -> Option<&'static RuntimeServices> {
    let p = boot::info().runtime_services;
    // SAFETY: the table address was saved before ExitBootServices; runtime memory is identity-mapped
    // and stays valid, as for ResetSystem in `power`.
    (p != 0).then(|| unsafe { &*(p as *const RuntimeServices) })
}

/// Stores `text` (cut to `MAX_LEN`, printable ASCII only). Best effort: returns false on failure.
pub fn record(text: &[u8]) -> bool {
    let Some(rt) = rt() else { return false };
    let mut buf = [0u8; MAX_LEN];
    let mut n = 0;
    for &b in text.iter().take(MAX_LEN) {
        buf[n] = if (0x20..0x7F).contains(&b) { b } else { b'?' };
        n += 1;
    }
    (rt.set_variable)(NAME.as_ptr(), &uefi::AEON_GUID, uefi::VAR_NV_BS_RT, n, buf.as_ptr()) == uefi::SUCCESS
}

/// Reads and deletes the record left by the previous boot. Returns the text length written to `out`.
pub fn take(rt: &RuntimeServices, out: &mut [u8; MAX_LEN]) -> usize {
    let mut size = out.len();
    let mut attr = 0u32;
    let s = (rt.get_variable)(NAME.as_ptr(), &uefi::AEON_GUID, &mut attr, &mut size, out.as_mut_ptr());
    if s != uefi::SUCCESS {
        if s == uefi::BUFFER_TOO_SMALL {
            // Not one of ours (too long): delete it so it cannot sit there forever.
            let _ = (rt.set_variable)(NAME.as_ptr(), &uefi::AEON_GUID, uefi::VAR_NV_BS_RT, 0, core::ptr::null());
        }
        return 0;
    }
    // A zero-length write deletes the variable, so the record is shown once.
    let _ = (rt.set_variable)(NAME.as_ptr(), &uefi::AEON_GUID, uefi::VAR_NV_BS_RT, 0, core::ptr::null());
    size.min(out.len())
}
