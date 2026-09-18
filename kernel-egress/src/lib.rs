//! kernel-egress — E24, the egress cone (the entropy gate).
//!
//! The coffee strainer for keys. During boot, the gate LEARNS what
//! legitimate egress looks like: per-window empirical entropy and
//! prose structure (punctuation/space presence). After freeze, every
//! outbound window faces two bounds:
//!
//!   1. DISTINCT-COUNT CEILING — key material is the noise class
//!      (E18: incompressible). Random bytes in a 64-byte window carry
//!      ~55-63 distinct values; English prose repeats its alphabet
//!      (~35-45 distinct). (Empirical Shannon entropy on 64-sample
//!      windows separates these only weakly — distinct count is the
//!      honest discriminator at this scale, found the hard way.)
//!   2. PROSE-STRUCTURE PROFILE — encoded exfil (hex, base64) sits
//!      UNDER the distinct ceiling (hex uses only 16 symbols), but it
//!      has no spaces, no punctuation: prose windows always carry
//!      >= 5% structure bytes; encodings carry 0%.
//!
//! Union rule: a window passes only under BOTH bounds. The profile is
//! retrained every boot (no static transfer function to invert — the
//! known-plaintext attack from the crypto.rs demonstration has
//! nothing fixed to learn).
//!
//! Honest labels: this raises exfil cost and kills bulk/raw/encoded
//! smuggling; it does NOT stop slow steganographic drip (a few
//! hex chars hidden per valid sentence) — bandwidth-limited
//! attackers always eventually win; the gate is a rate-and-cost
//! wall, not an absolute. Small writes (< 32 bytes) pass ungated
//! (that drip hole, labeled). Storage writes are out of scope (v1
//! gates the two OUTBOUND channels: display buffer and IPC).

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Window size for entropy scoring.
pub const WINDOW: usize = 64;
/// Windows smaller than this pass ungated (the drip hole, labeled).
pub const MIN_GATED_LEN: usize = 32;
/// Freeze after this many training windows.
pub const TRAINING_WINDOWS: u64 = 256;
/// Absolute slack over the trained distinct maximum: prose windows
/// sit ~10+ below random; 8 is a wide, safe margin.
pub const DISTINCT_SLACK: u32 = 8;
/// Structure floor: fraction of bytes that must be prose structure.
pub const STRUCTURE_FLOOR_FRAC: u64 = 5; // 5%
static FROZEN: AtomicBool = AtomicBool::new(false);
static TRAIN_WINDOWS: AtomicU64 = AtomicU64::new(0);
/// Trained max window distinct count.
static DISTINCT_MAX: AtomicU64 = AtomicU64::new(0);
static VIOLATIONS: AtomicU64 = AtomicU64::new(0);
/// Distinct ceiling (trained max + slack), packed as (max<<16).
static PASSED: AtomicU64 = AtomicU64::new(0);

#[inline]
fn is_structure(byte: u8) -> bool {
    matches!(byte, b' ' | b'.' | b',' | b'\n' | b':' | b';' | b'-' | b'(' | b')' | b'[' | b']' | b'!' | b'?')
}

/// Distinct-byte count of a window: the honest noise discriminator
/// at this scale. Prose repeats its alphabet (a 64-byte English
/// window carries ~35-45 distinct byte values); random key material
/// barely repeats (~55-63 distinct). Empirical Shannon entropy on
/// 64 samples separates the two only weakly — distinct count is the
/// wider margin, and it needs no floating point at all.
fn window_distinct(window: &[u8]) -> u32 {
    let mut seen = [false; 256];
    let mut count = 0u32;
    for &byte in window {
        let index = byte as usize;
        if !seen[index] {
            seen[index] = true;
            count += 1;
        }
    }
    count
}

fn window_structure_ok(window: &[u8]) -> bool {
    let structure = window.iter().filter(|&&b| is_structure(b)).count() as u64;
    structure * 100 >= window.len() as u64 * STRUCTURE_FLOOR_FRAC
}

fn train(window: &[u8]) {
    let distinct = window_distinct(window) as u64;
    let mut current = DISTINCT_MAX.load(Ordering::Relaxed);
    while distinct > current {
        match DISTINCT_MAX.compare_exchange_weak(
            current, distinct, Ordering::AcqRel, Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(seen) => current = seen,
        }
    }
    if TRAIN_WINDOWS.fetch_add(1, Ordering::AcqRel) + 1 >= TRAINING_WINDOWS {
        FROZEN.store(true, Ordering::Release);
    }
}

/// The gate: true = pass, false = refused (a violation). Short
/// buffers pass ungated (the labeled drip hole).
pub fn gate(bytes: &[u8]) -> bool {
    if bytes.len() < MIN_GATED_LEN {
        return true;
    }
    if !FROZEN.load(Ordering::Acquire) {
        for window in bytes.chunks(WINDOW) {
            if window.len() >= MIN_GATED_LEN {
                train(window);
            }
        }
        PASSED.fetch_add(1, Ordering::AcqRel);
        return true;
    }
    let ceiling = DISTINCT_MAX.load(Ordering::Acquire) as u32 + DISTINCT_SLACK;
    for window in bytes.chunks(WINDOW) {
        if window.len() < MIN_GATED_LEN {
            continue;
        }
        let distinct = window_distinct(window);
        if distinct > ceiling {
            VIOLATIONS.fetch_add(1, Ordering::AcqRel);
            return false; // noise class: raw key material
        }
        if !window_structure_ok(window) {
            VIOLATIONS.fetch_add(1, Ordering::AcqRel);
            return false; // encoded exfil: bits without prose structure
        }
    }
    PASSED.fetch_add(1, Ordering::AcqRel);
    true
}

/// Status: (frozen, trained windows, distinct max, violations, passed).
pub fn status() -> (bool, u64, u64, u64, u64) {
    (
        FROZEN.load(Ordering::Acquire),
        TRAIN_WINDOWS.load(Ordering::Acquire),
        DISTINCT_MAX.load(Ordering::Acquire),
        VIOLATIONS.load(Ordering::Acquire),
        PASSED.load(Ordering::Acquire),
    )
}

/// Host-test support.
#[cfg(feature = "std")]
pub fn reset() {
    FROZEN.store(false, Ordering::Release);
    TRAIN_WINDOWS.store(0, Ordering::Release);
    DISTINCT_MAX.store(0, Ordering::Release);
    VIOLATIONS.store(0, Ordering::Release);
    PASSED.store(0, Ordering::Release);
}

#[cfg(feature = "std")]
pub fn force_freeze() {
    if DISTINCT_MAX.load(Ordering::Acquire) == 0 {
        // A sane floor if nothing was ever trained (English prose).
        DISTINCT_MAX.store(45, Ordering::Release);
    }
    FROZEN.store(true, Ordering::Release);
}
