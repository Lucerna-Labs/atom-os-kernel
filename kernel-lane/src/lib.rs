//! kernel-lane — E25, the key lane: one owned way out.
//!
//! The HSM principle as a kernel state machine. Key material has
//! exactly one legal address outside its keeper: this lane. The
//! lane is DUMB by construction — no parser, no allocator, no
//! branches beyond the fixed protocol:
//!
//!   Empty --deposit--> Holding --handoff--> Empty (+ transit record)
//!
//! Every transit is logged (append-only ring). Unauthorized handoffs
//! do not fail — they receive HONEY: a deterministic decoy word,
//! key-shaped, indistinguishable to a thief who never held the real
//! one. The lane keeps a permanent REVOKE latch: once the spider has
//! ever condemned the lane task, real material never travels again
//! through it (honeymoon is over; a new ceremony — a reboot — is the
//! only path back). Together with the E24 egress cone (every other
//! door is prose-shaped), entropy outside this lane is illicit by
//! definition.
//!
//! Honest labels: honey is detectable by comparison with a known
//! good transit (the defense targets parties who never had the real
//! word); the lane crate is deliberately dependency-free and knows
//! nothing of the spider — the ORCHESTRATOR consults the sensor and
//! trips the latch, so the lane stays dumb.

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const LOG_SLOTS: usize = 16;
pub const KIND_REAL: u64 = 1;
pub const KIND_HONEY: u64 = 2;

static LANE_PID: AtomicU64 = AtomicU64::new(u64::MAX);
static HOLDING_WORD: AtomicU64 = AtomicU64::new(0);
static HOLDING_TAG: AtomicU64 = AtomicU64::new(0);
static HOLDING: AtomicBool = AtomicBool::new(false);
static REVOKED: AtomicBool = AtomicBool::new(false);
static REAL_COUNT: AtomicU64 = AtomicU64::new(0);
static HONEY_COUNT: AtomicU64 = AtomicU64::new(0);
/// Append-only ring: (kind, word), index = transit# % LOG_SLOTS.
static LOG: [(AtomicU64, AtomicU64); LOG_SLOTS] = [
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
    (AtomicU64::new(0), AtomicU64::new(0)),
];

/// A mixing step (the splitmix finalizer): deterministic avalanche.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The honey word: key-shaped, deterministic per transit number, so
/// repeat thieves get consistent garbage (consistency reads as real).
fn honey_word(transit_number: u64) -> u64 {
    mix(0xE25_C0DE ^ transit_number.wrapping_mul(7919))
}

/// Claim the lane (the ceremony): the first caller becomes the lane
/// task; one claim per boot, no re-claiming.
pub fn claim(pid: u64) -> bool {
    let none = u64::MAX;
    LANE_PID
        .compare_exchange(none, pid, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub fn lane_pid() -> u64 {
    LANE_PID.load(Ordering::Acquire)
}

/// Deposit material into the lane (lane task only, must be Empty,
/// and the lane must not be revoked — a revoked lane carries
/// nothing, fail closed).
pub fn deposit(pid: u64, word: u64, tag: u64) -> bool {
    if pid != lane_pid() || REVOKED.load(Ordering::Acquire) {
        return false;
    }
    if HOLDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false; // already holding: refuse, never overwrite
    }
    HOLDING_WORD.store(word, Ordering::Release);
    HOLDING_TAG.store(tag, Ordering::Release);
    true
}

/// The verdict of a handoff attempt.
pub enum Handoff {
    /// Real material left the system through the one way out.
    Real { word: u64, tag: u64, transit: u64 },
    /// Honey served: unauthorized requester or revoked lane. The
    /// word is decoy material; the real word (if any) stays Holding.
    Honey { word: u64, transit: u64 },
    /// Nothing to carry (lane Empty) — plain refusal, no theater.
    Nothing,
}

/// THE HANDOFF — the one way out. Lane task + trusted + holding
/// carries real material; anyone else (or a revoked lane) receives
/// honey; an empty lane refuses plainly.
pub fn handoff(pid: u64) -> Handoff {
    if !HOLDING.load(Ordering::Acquire) {
        return Handoff::Nothing;
    }
    let transit = REAL_COUNT.load(Ordering::Acquire) + HONEY_COUNT.load(Ordering::Acquire);
    if pid != lane_pid() || REVOKED.load(Ordering::Acquire) {
        let word = honey_word(transit);
        HONEY_COUNT.fetch_add(1, Ordering::AcqRel);
        let slot = (transit % LOG_SLOTS as u64) as usize;
        LOG[slot].0.store(KIND_HONEY, Ordering::Release);
        LOG[slot].1.store(word, Ordering::Release);
        return Handoff::Honey { word, transit };
    }
    let word = HOLDING_WORD.load(Ordering::Acquire);
    let tag = HOLDING_TAG.load(Ordering::Acquire);
    HOLDING.store(false, Ordering::Release);
    REAL_COUNT.fetch_add(1, Ordering::AcqRel);
    let slot = (transit % LOG_SLOTS as u64) as usize;
    LOG[slot].0.store(KIND_REAL, Ordering::Release);
    LOG[slot].1.store(word, Ordering::Release);
    Handoff::Real { word, tag, transit }
}

/// The spider's verdict, tripped by the orchestrator: permanent
/// distrust. Real material never travels through this lane again.
pub fn revoke() {
    REVOKED.store(true, Ordering::Release);
}

pub fn is_revoked() -> bool {
    REVOKED.load(Ordering::Acquire)
}

/// Read the i-th transit record: (kind, word). Out-of-range and
/// unwritten slots return (0, 0).
pub fn log_get(index: u64) -> (u64, u64) {
    let total = REAL_COUNT.load(Ordering::Acquire) + HONEY_COUNT.load(Ordering::Acquire);
    if index >= total || index >= LOG_SLOTS as u64 {
        return (0, 0);
    }
    (
        LOG[index as usize].0.load(Ordering::Acquire),
        LOG[index as usize].1.load(Ordering::Acquire),
    )
}

/// Status: (lane_pid, holding, revoked, real_count, honey_count).
pub fn status() -> (u64, bool, bool, u64, u64) {
    (
        lane_pid(),
        HOLDING.load(Ordering::Acquire),
        REVOKED.load(Ordering::Acquire),
        REAL_COUNT.load(Ordering::Acquire),
        HONEY_COUNT.load(Ordering::Acquire),
    )
}

/// Host-test support.
#[cfg(feature = "std")]
pub fn reset() {
    LANE_PID.store(u64::MAX, Ordering::Release);
    HOLDING_WORD.store(0, Ordering::Release);
    HOLDING_TAG.store(0, Ordering::Release);
    HOLDING.store(false, Ordering::Release);
    REVOKED.store(false, Ordering::Release);
    REAL_COUNT.store(0, Ordering::Release);
    HONEY_COUNT.store(0, Ordering::Release);
    for (kind, word) in LOG.iter() {
        kind.store(0, Ordering::Release);
        word.store(0, Ordering::Release);
    }
}
