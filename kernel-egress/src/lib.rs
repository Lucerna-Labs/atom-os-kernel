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
//! wall, not an absolute.
//!
//! `gate` scores one buffer and lets windows under 32 bytes through
//! (the drip hole). `egress` composes a per-process STREAM over it:
//! short writes and short tails accumulate in the process's own
//! 64-byte window and are scored when it fills, so a byte-at-a-time
//! loop is scored exactly like one 64-byte write. Bytes that left
//! before the window filled are gone (the stream refuses the write
//! that completes a bad window, not the ones before it) — the wall
//! is per 64 bytes, labeled. Storage writes are out of scope (v1
//! gates the OUTBOUND channels: console, stdout, pipes and IPC).
//! The desktop's pixel framebuffer is not a prose channel and is not
//! gated; only the display owner (the bundled desktop) can map it,
//! and windowed programs reach the screen through gated text.

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

/// Per-process accumulators: one window per live pid (the task cap).
pub const STREAMS: usize = 16;

struct Stream { pid: u64, len: usize, window: [u8; WINDOW] }

static STREAM_LOCK: AtomicBool = AtomicBool::new(false);
static mut STREAM_TABLE: [Stream; STREAMS] = [const { Stream { pid: 0, len: 0, window: [0; WINDOW] } }; STREAMS];

fn with_streams<R>(f: impl FnOnce(&mut [Stream; STREAMS]) -> R) -> R {
    while STREAM_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    // Single-core kernel: the lock is the only path to the table.
    let result = f(unsafe { &mut *core::ptr::addr_of_mut!(STREAM_TABLE) });
    STREAM_LOCK.store(false, Ordering::Release);
    result
}

/// The gate for a process's outbound write. Full windows are scored
/// as `gate` scores them; the short remainder joins the process's
/// stream and is scored when 64 bytes have accumulated. Returns false
/// when the write is refused (a violation). A process with no free
/// stream slot is held to the buffer rule alone (the table has one
/// slot per possible task, so this only happens if `forget` is missed).
pub fn egress(pid: u64, bytes: &[u8]) -> bool {
    let full = bytes.len() - bytes.len() % WINDOW;
    if full > 0 && !gate(&bytes[..full]) {
        return false;
    }
    let tail = &bytes[full..];
    if tail.is_empty() {
        return true;
    }
    with_streams(|streams| {
        let slot = match streams.iter().position(|s| s.pid == pid && s.len > 0)
            .or_else(|| streams.iter().position(|s| s.len == 0)) {
            Some(slot) => slot,
            None => return gate(tail),
        };
        let stream = &mut streams[slot];
        stream.pid = pid;
        for &byte in tail {
            stream.window[stream.len] = byte;
            stream.len += 1;
            if stream.len == WINDOW {
                let window = stream.window;
                stream.len = 0;
                if !gate(&window) {
                    return false;
                }
            }
        }
        true
    })
}

/// A reaped pid's pending window is dropped.
pub fn forget(pid: u64) {
    with_streams(|streams| {
        for stream in streams.iter_mut() {
            if stream.pid == pid { stream.len = 0; stream.pid = 0; }
        }
    });
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
    with_streams(|streams| for s in streams.iter_mut() { s.len = 0; s.pid = 0; });
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 64] = [
        0x8d, 0x20, 0xfb, 0x62, 0x5f, 0x6f, 0x5c, 0xc4, 0x68, 0x08, 0xd7, 0x0f, 0x37, 0x2e, 0xc4, 0xad,
        0x91, 0x7c, 0x3b, 0xa4, 0xe2, 0xf1, 0x55, 0x19, 0x0d, 0xc2, 0x7e, 0x41, 0xb8, 0x9d, 0x02, 0x51,
        0xf7, 0x30, 0xa1, 0xcc, 0x6e, 0x19, 0x4b, 0x8a, 0x2d, 0x76, 0x03, 0xc9, 0xaa, 0x5e, 0x71, 0xf0,
        0x44, 0x2b, 0xbe, 0x87, 0xd6, 0x12, 0xc4, 0x99, 0x37, 0xe5, 0x8c, 0x21, 0x0b, 0x74, 0xde, 0x66,
    ];
    const PROSE: &[u8] = b"status report: all systems nominal, the door out is prose-shaped and this passes. ";

    fn trained() { reset(); force_freeze(); }

    #[test]
    fn gate_e1_buffer_rule_unchanged() {
        trained();
        assert!(gate(PROSE));
        assert!(!gate(&KEY));
        assert!(gate(&KEY[..31]), "short buffers pass the buffer rule (the labeled drip hole)");
    }

    #[test]
    fn gate_e2_byte_at_a_time_key_is_refused_at_the_window() {
        trained();
        let mut refused_at = None;
        for (i, &b) in KEY.iter().enumerate() {
            if !egress(7, &[b]) { refused_at = Some(i); break; }
        }
        assert_eq!(refused_at, Some(63), "the write that completes the window is refused");
    }

    #[test]
    fn gate_e3_byte_at_a_time_prose_passes() {
        trained();
        for _ in 0..3 { for &b in PROSE { assert!(egress(8, &[b])); } }
    }

    #[test]
    fn gate_e4_short_tails_of_long_writes_are_scored() {
        trained();
        // 33-byte writes: a 33-byte buffer is one short window under the
        // buffer rule alone. Streamed, the tails fill a window of key bytes.
        let mut refused = false;
        for chunk in KEY.iter().cycle().take(33 * 4).copied().collect::<Vec<u8>>().chunks(33) {
            if !egress(9, chunk) { refused = true; break; }
        }
        assert!(refused);
    }

    #[test]
    fn gate_e5_streams_are_per_pid_and_forgotten_on_reap() {
        trained();
        for &b in &KEY[..40] { assert!(egress(10, &[b])); }
        for &b in PROSE { assert!(egress(11, &[b])); }
        forget(10);
        // A fresh pid 10 starts a clean window: 40 key bytes fit again.
        for &b in &KEY[..40] { assert!(egress(10, &[b])); }
    }
}

#[cfg(feature = "std")]
pub fn force_freeze() {
    if DISTINCT_MAX.load(Ordering::Acquire) == 0 {
        // A sane floor if nothing was ever trained (English prose).
        DISTINCT_MAX.store(45, Ordering::Release);
    }
    FROZEN.store(true, Ordering::Release);
}
