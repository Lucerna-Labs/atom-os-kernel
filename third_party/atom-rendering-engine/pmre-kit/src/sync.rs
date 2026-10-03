//! Shared-state shims. With `std` these are the standard types; without it (an OS kernel's
//! userspace, firmware) they are a minimal spin lock, a once-cell and an ordered map, so
//! the font caches work unchanged on a single-threaded, allocator-only target.

#[cfg(feature = "std")]
pub use std::sync::{Arc, Mutex, OnceLock};
/// Cache map: hashed with `std`, ordered without it (keys are all `Ord`).
#[cfg(feature = "std")]
pub type Map<K, V> = std::collections::HashMap<K, V>;

#[cfg(not(feature = "std"))]
pub use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
pub type Map<K, V> = alloc::collections::BTreeMap<K, V>;
#[cfg(not(feature = "std"))]
pub use spin::{Mutex, OnceLock};

#[cfg(not(feature = "std"))]
mod spin {
    use core::cell::UnsafeCell;
    use core::convert::Infallible;
    use core::ops::{Deref, DerefMut};
    use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    /// Spin lock with the `std::sync::Mutex` call shape (`lock()` returns a `Result`).
    pub struct Mutex<T> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }
    unsafe impl<T: Send> Sync for Mutex<T> {}
    unsafe impl<T: Send> Send for Mutex<T> {}

    pub struct MutexGuard<'a, T> {
        lock: &'a Mutex<T>,
    }

    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Self {
            Self {
                locked: AtomicBool::new(false),
                value: UnsafeCell::new(value),
            }
        }
        pub fn lock(&self) -> Result<MutexGuard<'_, T>, Infallible> {
            while self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            Ok(MutexGuard { lock: self })
        }
    }
    impl<T> Deref for MutexGuard<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            unsafe { &*self.lock.value.get() }
        }
    }
    impl<T> DerefMut for MutexGuard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            unsafe { &mut *self.lock.value.get() }
        }
    }
    impl<T> Drop for MutexGuard<'_, T> {
        fn drop(&mut self) {
            self.lock.locked.store(false, Ordering::Release);
        }
    }

    const EMPTY: u8 = 0;
    const BUSY: u8 = 1;
    const READY: u8 = 2;

    /// Write-once cell with the `std::sync::OnceLock` call shape.
    pub struct OnceLock<T> {
        state: AtomicU8,
        value: UnsafeCell<Option<T>>,
    }
    unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}
    unsafe impl<T: Send> Send for OnceLock<T> {}

    impl<T> Default for OnceLock<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<T> OnceLock<T> {
        pub const fn new() -> Self {
            Self {
                state: AtomicU8::new(EMPTY),
                value: UnsafeCell::new(None),
            }
        }
        pub fn get(&self) -> Option<&T> {
            if self.state.load(Ordering::Acquire) == READY {
                unsafe { (*self.value.get()).as_ref() }
            } else {
                None
            }
        }
        pub fn set(&self, value: T) -> Result<(), T> {
            if self
                .state
                .compare_exchange(EMPTY, BUSY, Ordering::Acquire, Ordering::Acquire)
                .is_err()
            {
                return Err(value);
            }
            unsafe { *self.value.get() = Some(value) };
            self.state.store(READY, Ordering::Release);
            Ok(())
        }
        pub fn get_or_init(&self, f: impl FnOnce() -> T) -> &T {
            if let Some(v) = self.get() {
                return v;
            }
            if self
                .state
                .compare_exchange(EMPTY, BUSY, Ordering::Acquire, Ordering::Acquire)
                .is_ok()
            {
                unsafe { *self.value.get() = Some(f()) };
                self.state.store(READY, Ordering::Release);
            }
            while self.state.load(Ordering::Acquire) != READY {
                core::hint::spin_loop();
            }
            self.get().unwrap()
        }
    }
}
