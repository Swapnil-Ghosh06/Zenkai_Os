//! Minimal interior-mutability cell for kernel globals.
//!
//! # The rule Aeon relies on
//!
//! Aeon runs on one CPU. The only way two pieces of code can then touch the same static at the same
//! time is an interrupt arriving in the middle of the first one. And the interrupt handlers are not
//! small: the 100 Hz timer runs the scheduler (`sched::tick`), and the yield vector switches tasks.
//! So a `static` is safe to use through [`Global::get`] only when ONE of these holds:
//!
//! 1. no interrupt handler or other task ever touches it (for example data used only by the shell task), or
//! 2. interrupts are switched off for as long as the `&mut` is alive.
//!
//! Case 2 is checkable, so state shared with the timer handler uses [`Global::get_irqs_off`], which
//! stops the kernel with a message when interrupts are on. Locks that must be held across an interrupt
//! use `spin::SpinLock` (it switches interrupts off) or a flag (see `vblk`) when the work is slow.

use core::cell::UnsafeCell;

pub struct Global<T>(UnsafeCell<T>);

// SAFETY: there is a single hardware thread; every use site upholds one of the two rules in the
// module comment, so no two threads of control can reach the same `Global` concurrently.
unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// Raw pointer to the contents.
    pub fn as_ptr(&self) -> *mut T {
        self.0.get()
    }

    /// Mutable access to the contents.
    ///
    /// # Safety
    /// The caller must ensure no other reference to the contents is live, i.e. this is not called
    /// re-entrantly while a previous `get` result is still in use, and no interrupt handler can reach
    /// the same data while the result is alive (see the rule in the module comment).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self) -> &mut T {
        // SAFETY: exclusivity is the caller's obligation, stated in the function contract.
        unsafe { &mut *self.0.get() }
    }

    /// Like [`Global::get`] for data an interrupt handler also uses: stops the kernel if interrupts are
    /// on, because then a handler could run while the `&mut` is alive.
    ///
    /// # Safety
    /// The same as `get`, except that "no interrupt handler can interfere" is checked here. Interrupts
    /// must stay off for as long as the result is alive.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_irqs_off(&self) -> &mut T {
        assert!(!crate::arch::interrupts_enabled(), "Global::get_irqs_off called with interrupts on (the interrupts-off rule)");
        // SAFETY: interrupts are off (checked above) and the caller guarantees the rest.
        unsafe { &mut *self.0.get() }
    }
}
