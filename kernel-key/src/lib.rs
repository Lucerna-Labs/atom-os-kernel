//! kernel-key — E22, the fail-dead thermodynamic key.
//!
//! The key is not a stored secret; it is an ENERGY LEVEL. While the
//! keeper pays maintenance, the key persists and reads out. Quiet
//! erodes it (the same law that erodes scars: writing requires
//! activity, decay never sleeps). The spider's intrusion signal
//! destroys it IMMEDIATELY and revokes keepership — after which no
//! pid can ever maintain again, so the intruder cannot pay the key
//! back to life.
//!
//! The polarity is the invention: classical zeroization must succeed
//! as an ACTION (fail-alive if the attacker blocks it); this key dies
//! unless maintenance keeps succeeding (fail-dead). The key is alive
//! only while it is trusted.
//!
//! Honest labels: the v1 seed is caller-supplied (the kernel has no
//! entropy source — the QRNG lane's job someday); the mechanism under
//! test is LIFECYCLE, not material. Not unclonable (classical states
//! can be copied if readable) — PERISHABLE, which defeats
//! theft-of-stored-key and stale-secret compromise, the threat models
//! that dominate real key loss.

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Full energy at creation; decay drains it every scheduler tick.
pub const FULL_ENERGY: f32 = 100.0;
/// Passive decay per tick: abandonment kills the key in 5,000 ticks.
pub const DECAY_PER_TICK: f32 = 0.02;
/// Maintenance restores this much, capped at full.
pub const MAINTAIN_RESTORE: f32 = 10.0;

/// The key material while alive; zeroed on death. Word i derives as
/// seed[i] ^ (i+1) so no two slots coincide even from a flat seed.
static WORDS: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
/// Packed lifecycle: alive<<63 | death_cause<<60 | energy x1024.
/// Death causes: 0 none, 1 erosion, 2 spider signal, 3 never lived.
static STATE: AtomicU64 = AtomicU64::new(3 << 60);
/// The only pid allowed to maintain/read; u64::MAX = none.
static KEEPER: AtomicU64 = AtomicU64::new(u64::MAX);
static EVER_ALIVE: AtomicBool = AtomicBool::new(false);

fn pack(alive: bool, cause: u8, energy: f32) -> u64 {
    let energy_q = (energy.clamp(0.0, FULL_ENERGY) * 1024.0) as u64 & 0x0FFF_FFFF;
    (u64::from(alive) << 63) | (u64::from(cause & 0x7) << 60) | energy_q
}

fn unpack(packed: u64) -> (bool, u8, f32) {
    (packed >> 63 == 1, ((packed >> 60) & 0x7) as u8, (packed & 0x0FFF_FFFF) as f32 / 1024.0)
}

/// Create the key (the ceremony). Caller-supplied seed words in v1 —
/// labeled: material is NOT entropy-sourced here. One life per boot:
/// no resurrection after death.
pub fn init(keeper: u64, seed: [u64; 4]) -> bool {
    if EVER_ALIVE.swap(true, Ordering::AcqRel) {
        return false;
    }
    for (slot, word) in WORDS.iter().zip(seed) {
        slot.store(word, Ordering::Release);
    }
    KEEPER.store(keeper, Ordering::Release);
    STATE.store(pack(true, 0, FULL_ENERGY), Ordering::Release);
    true
}

fn die(cause: u8) {
    for slot in WORDS.iter() {
        slot.store(0, Ordering::Release);
    }
    KEEPER.store(u64::MAX, Ordering::Release);
    STATE.store(pack(false, cause, 0.0), Ordering::Release);
}

/// Scheduler tick: passive erosion. Death by abandonment needs no
/// signal, no action, no luck — only silence.
pub fn tick() {
    let (alive, _, energy) = unpack(STATE.load(Ordering::Acquire));
    if !alive {
        return;
    }
    let energy = energy - DECAY_PER_TICK;
    if energy <= 0.0 {
        die(1); // erosion
        return;
    }
    STATE.store(pack(true, 0, energy), Ordering::Release);
}

/// Keeper maintenance: restores energy. Refused for any other pid,
/// and refused forever after the spider revokes keepership.
pub fn maintain(pid: u64) -> bool {
    let (alive, _, energy) = unpack(STATE.load(Ordering::Acquire));
    if !alive || KEEPER.load(Ordering::Acquire) != pid {
        return false;
    }
    let energy = (energy + MAINTAIN_RESTORE).min(FULL_ENERGY);
    STATE.store(pack(true, 0, energy), Ordering::Release);
    true
}

/// THE SPIDER SIGNAL: immediate destruction and keeper revocation.
/// Fired when the shadow web condemns an intruder. Passive decay in
/// `tick` means the intruder also cannot wait us out: keepership is
/// gone, so no one can ever pay again.
pub fn spider_destroy() {
    die(2);
}

/// Read the key: words while alive AND keeper-gated; zeros otherwise.
pub fn read(pid: u64) -> [u64; 4] {
    let (alive, _, _) = unpack(STATE.load(Ordering::Acquire));
    if !alive || KEEPER.load(Ordering::Acquire) != pid {
        return [0; 4];
    }
    let mut words = [0u64; 4];
    for (slot, word) in WORDS.iter().zip(words.iter_mut()) {
        *word = slot.load(Ordering::Acquire);
    }
    words
}

/// Status: (alive, death_cause, energy x1024).
pub fn status() -> (bool, u8, u64) {
    let packed = STATE.load(Ordering::Acquire);
    (packed >> 63 == 1, ((packed >> 60) & 0x7) as u8, packed & 0x0FFF_FFFF)
}

/// Host-test support: full reset (the kernel never calls this).
#[cfg(feature = "std")]
pub fn reset() {
    STATE.store(3 << 60, Ordering::Release);
    for slot in WORDS.iter() {
        slot.store(0, Ordering::Release);
    }
    KEEPER.store(u64::MAX, Ordering::Release);
    EVER_ALIVE.store(false, Ordering::Release);
}
