//! Global field-state glue.
//!
//! Owns the `FieldState` the kernel timer IRQ advances and the scheduler
//! consults. Lazily initialised on first kernel boot; once initialised it
//! persists for the lifetime of the kernel.
//!
//! The field is configured via `kernel_glue::FieldState::default_kernel_config`
//! (a 24x24 adaptive substrate). Per-tick cost is bounded by the small field;
//! the timer IRQ calls `evolve_once()` once every `EVOLUTION_DIVISOR` ticks
//! (default 10) so a 100 Hz timer drives the field at 10 Hz.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::glue::FieldState;

/// Number of timer ticks between field evolutions. At 100 Hz timer this
/// drives the field at 10 Hz. Tunable at runtime via `set_evolution_divisor`.
const DEFAULT_EVOLUTION_DIVISOR: u64 = 10;

static EVOLUTION_DIVISOR: AtomicU64 = AtomicU64::new(DEFAULT_EVOLUTION_DIVISOR);
static INITIALISED: AtomicBool = AtomicBool::new(false);

// Statically reserved storage for the FieldState. The FieldState itself owns
// a heap-backed `World`; we store an Option in static storage and initialise
// it on first boot. Use raw-pointer access throughout to comply with
// Rust-2024 `static_mut_refs` rules (the kernel already uses this idiom for
// `SYSTEM`).
static mut FIELD: Option<FieldState> = None;

/// Initialise the global field. Safe to call exactly once at boot. Calling
/// more than once is a no-op (the existing field is kept).
pub fn init() {
    let state = FieldState::new(FieldState::default_kernel_config())
        .expect("default kernel field config should validate");
    unsafe {
        if !INITIALISED.swap(true, Ordering::SeqCst) {
            (*(&raw mut FIELD)) = Some(state);
        }
    }
}

/// Advance the field one moment, if `tick` falls on an evolution boundary.
/// Called from the timer IRQ handler.
pub fn maybe_evolve(tick: u64) {
    let divisor = EVOLUTION_DIVISOR.load(Ordering::Relaxed).max(1);
    if tick % divisor != 0 {
        return;
    }
    unsafe {
        if let Some(field) = &*(&raw const FIELD) {
            field.evolve_once();
        }
    }
}

/// Returns the number of moments the field has evolved. 0 if uninitialised.
pub fn age() -> u64 {
    unsafe {
        match &*(&raw const FIELD) {
            Some(f) => f.age(),
            None => 0,
        }
    }
}

/// Override the per-tick evolution divisor. Useful for tests and tuning.
pub fn set_evolution_divisor(divisor: u64) {
    EVOLUTION_DIVISOR.store(divisor.max(1), Ordering::Relaxed);
}

/// Access the global field under a closure. Returns `None` if uninitialised.
/// Used by the syscall layer (SYS_FIELD_STIMULATE, SYS_FIELD_EVOLVE,
/// SYS_FIELD_OBSERVE, SYS_FIELD_MEASUREMENTS) and the IPC and scheduler
/// paths.
pub fn with_field<R>(f: impl FnOnce(&FieldState) -> R) -> Option<R> {
    unsafe { (&*(&raw const FIELD)).as_ref().map(f) }
}
