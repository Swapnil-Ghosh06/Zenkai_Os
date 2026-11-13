//! Interrupt-safe spinlock (`c-lock`).
//!
//! Locking switches interrupts off first and puts them back as they were when the guard is dropped, so
//! an interrupt handler can never find the lock held by the code it interrupted. On the single CPU Aeon
//! runs on, finding the lock already taken from the same thread of control is a bug, so the kernel build
//! stops with a message instead of spinning forever. On the host (unit tests) it is an ordinary spinlock.
//!
//! Rules: never yield or sleep while holding a guard, and keep the critical section short.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "uefi")]
use crate::arch::{irq_restore, irq_save};

#[cfg(not(target_os = "uefi"))]
fn irq_save() -> bool {
    false
}
#[cfg(not(target_os = "uefi"))]
fn irq_restore(_was_enabled: bool) {}

/// Spins before the kernel build decides the lock can never be released (single CPU: nobody else will).
#[cfg(target_os = "uefi")]
const STUCK_AFTER: u32 = 1 << 22;

pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

// SAFETY: the lock hands out at most one guard at a time, so `T` is only ever accessed by one thread.
unsafe impl<T: Send> Sync for SpinLock<T> {}
// SAFETY: moving the lock moves the data; it is sound exactly when `T` can move between threads.
unsafe impl<T: Send> Send for SpinLock<T> {}

pub struct SpinGuard<'a, T> {
    lock: &'a SpinLock<T>,
    irq_was_enabled: bool,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), data: UnsafeCell::new(value) }
    }

    /// Takes the lock, with interrupts off until the guard is dropped.
    pub fn lock(&self) -> SpinGuard<'_, T> {
        let irq_was_enabled = irq_save();
        #[cfg(target_os = "uefi")]
        let mut spins = 0u32;
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            #[cfg(target_os = "uefi")]
            {
                spins += 1;
                assert!(spins < STUCK_AFTER, "spinlock deadlock: the lock is held by the thread that asked for it");
            }
            core::hint::spin_loop();
        }
        SpinGuard { lock: self, irq_was_enabled }
    }

    /// Takes the lock if it is free, otherwise returns `None` and leaves interrupts as they were.
    pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
        let irq_was_enabled = irq_save();
        if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            Some(SpinGuard { lock: self, irq_was_enabled })
        } else {
            irq_restore(irq_was_enabled);
            None
        }
    }
}

impl<T> Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the guard proves this thread holds the lock, so no other reference to the data exists.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`, and `&mut self` makes this the only reference through the guard.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        irq_restore(self.irq_was_enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn guard_gives_access_and_releases_on_drop() {
        let l = SpinLock::new(5u32);
        {
            let mut g = l.lock();
            *g += 1;
            assert_eq!(*g, 6);
            assert!(l.try_lock().is_none());
        }
        assert_eq!(*l.lock(), 6);
        assert!(l.try_lock().is_some());
    }

    #[test]
    fn try_lock_fails_while_held_and_works_after() {
        let l = SpinLock::new(());
        let g = l.lock();
        assert!(l.try_lock().is_none());
        assert!(l.try_lock().is_none());
        drop(g);
        let a = l.try_lock();
        assert!(a.is_some());
        assert!(l.try_lock().is_none());
    }

    #[test]
    fn threads_never_overlap_in_the_critical_section() {
        let l = Arc::new(SpinLock::new(0u64));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let l = Arc::clone(&l);
                std::thread::spawn(move || {
                    for _ in 0..5_000 {
                        let mut g = l.lock();
                        // A non-atomic read-modify-write: it only comes out right if access is exclusive.
                        let v = *g;
                        std::hint::black_box(&v);
                        *g = v + 1;
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(*l.lock(), 20_000);
    }

    #[test]
    fn lock_can_hold_non_copy_data() {
        let l = SpinLock::new(Vec::<u8>::new());
        l.lock().push(1);
        l.lock().push(2);
        assert_eq!(&*l.lock(), &[1, 2]);
    }
}
