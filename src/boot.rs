//! The kernel's single `BootInfo`, filled during boot and read-only afterwards.

use crate::bootinfo::BootInfo;
use crate::global::Global;

static BOOT: Global<BootInfo> = Global::new(BootInfo::new());

/// Access to the boot information.
///
/// Boot is single-threaded while this is being filled, and afterwards callers only read it, keeping
/// each reference short-lived; that is the invariant that makes handing out `&mut` here sound.
pub fn info() -> &'static BootInfo {
    // SAFETY: after boot nothing writes, and during boot the only writer is `info_mut`, whose caller
    // keeps its reference short-lived and never holds it across a call that reads through here.
    unsafe { &*BOOT.as_ptr() }
}

/// Mutable access for the boot code that fills the information in.
///
/// # Safety
/// Only the boot flow may call this, single-threaded, and the returned reference must not be held
/// across a call that reads `info()`.
pub unsafe fn info_mut() -> &'static mut BootInfo {
    // SAFETY: forwarded contract.
    unsafe { BOOT.get() }
}
