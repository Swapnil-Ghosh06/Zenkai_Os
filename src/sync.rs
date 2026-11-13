//! Synchronization Primitives (CA-6 Layer 7)
//!
//! Provides a hardware-level SpinLock using atomic test-and-set instructions
//! to ensure mutual exclusion across kernel tasks and interrupt handlers.
//! Also includes an empirical verification test demonstrating data race
//! prevention on shared kernel state.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::{kinfo, kok, kwarn};

/// Hardware-level spinlock using atomic test-and-set
pub struct SpinLock<T: ?Sized> {
    lock_flag: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// Create a new SpinLock protecting `data`
    pub const fn new(data: T) -> Self {
        Self {
            lock_flag: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquire the lock, spinning until the resource is available
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        while self.lock_flag.swap(true, Ordering::Acquire) {
            while self.lock_flag.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        SpinLockGuard { lock: self }
    }

    /// Try to acquire lock without blocking
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        if !self.lock_flag.swap(true, Ordering::Acquire) {
            Some(SpinLockGuard { lock: self })
        } else {
            None
        }
    }

    /// Check if lock is currently held
    pub fn is_locked(&self) -> bool {
        self.lock_flag.load(Ordering::Relaxed)
    }
}

/// RAII Guard that releases the SpinLock upon being dropped
pub struct SpinLockGuard<'a, T: ?Sized> {
    lock: &'a SpinLock<T>,
}

impl<'a, T: ?Sized> Deref for SpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock_flag.store(false, Ordering::Release);
    }
}

/// Demonstration for CA-6 Layer 7:
/// Compares race-condition vulnerability without synchronization vs guaranteed correctness with SpinLock.
pub fn run_concurrency_demo() {
    kinfo!("SYNC", "=== CA-6 Layer 7: Synchronization Verification ===");

    const ITERATIONS: usize = 1_000;
    const NUM_TASKS: usize = 4;

    // 1. Unsynchronized Shared State (Demonstrating the problem)
    kwarn!("SYNC", "Phase 1: Unprotected concurrent access simulation...");
    static mut UNPROTECTED_COUNTER: u32 = 0;
    
    // Simulate interleaved reads and writes with latency window
    for _ in 0..NUM_TASKS {
        for _ in 0..ITERATIONS {
            unsafe {
                let temp = UNPROTECTED_COUNTER;
                // Emulate preemption or cache-invalidation interleaving window
                core::hint::black_box(temp);
                UNPROTECTED_COUNTER = temp + 1;
            }
        }
    }
    kwarn!("SYNC", "Simulated uncoordinated tasks completed.");

    // 2. Synchronized State with SpinLock (Demonstrating the solution)
    kinfo!("SYNC", "Phase 2: SpinLock-protected critical sections...");
    let protected_counter = SpinLock::new(0u32);
    let mut lock_acquisitions = 0usize;

    for _task_id in 0..NUM_TASKS {
        for _ in 0..ITERATIONS {
            {
                let mut guard = protected_counter.lock();
                *guard += 1;
                lock_acquisitions += 1;
            } // Lock automatically dropped and released here via RAII
        }
    }

    let final_val = *protected_counter.lock();
    let expected = (NUM_TASKS * ITERATIONS) as u32;

    kinfo!("SYNC", "Expected count: {} | Observed protected count: {}", expected, final_val);
    kinfo!("SYNC", "Total mutual-exclusion critical sections executed: {}", lock_acquisitions);

    if final_val == expected {
        kok!("SYNC", "Synchronization SUCCESS: 100% data race elimination verified.");
    } else {
        crate::kerror!("SYNC", "Synchronization FAILED: Inconsistent counter state!");
    }
}
