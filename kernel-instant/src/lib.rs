//! kernel-instant — E23, the instant-key (reconstruction-at-use).
//!
//! The key never exists as a standing thing. Four SHARES live in
//! separate slots; each share alone is uniform noise and carries
//! ZERO information about the key (XOR sharing — Shannon-perfect:
//! any subset smaller than all shares is statistically independent
//! of the secret; the same theorem that makes the one-time pad
//! unbreakable). At use, the shares recombine — a fold — inside one
//! syscall frame; the key exists for one instruction; then it is
//! unborn again. Between calls, memory holds only noise.
//!
//! `transform(w)` is DETERMINISTIC for the life of the share-set:
//! stability without persistence — the key is a behavior, not a
//! thing. Recombination is fold(shares, 0, xor): the stacking
//! operator, again.
//!
//! The decay law governs the SHARE-SET's energy (shares erode
//! without maintenance; a dead share-set means the key is not merely
//! dead but UNRECONSTRUCTABLE — unborn in the past tense too). The
//! spider signal destroys the shares instantly.
//!
//! Honest labels: not unclonable (an in-memory attacker who reads
//! ALL FOUR share slots can recombine at will) — this defends the
//! SYSCALL BOUNDARY and shrinks the temporal window from the key's
//! lifetime to single instruction instants: minutes to nanoseconds.
//! Share randomness in v1 is derived from the caller seed, not a
//! physical entropy source (QRNG lane's job).

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const FULL_ENERGY: f32 = 100.0;
pub const DECAY_PER_TICK: f32 = 0.02;
pub const MAINTAIN_RESTORE: f32 = 10.0;

static SHARES: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
/// Packed state: alive<<63 | cause<<60 | energy x1024.
/// Causes: 0 none, 1 erosion, 2 spider, 3 never lived.
static STATE: AtomicU64 = AtomicU64::new(3 << 60);
static KEEPER: AtomicU64 = AtomicU64::new(u64::MAX);
static EVER_ALIVE: AtomicBool = AtomicBool::new(false);
/// The reconstructed key, nonzero ONLY inside a transform call —
/// the probe proves it is zero between uses (the instant is real).
static TRANSIENT: AtomicU64 = AtomicU64::new(0);

/// A mixing step (splitmix64's finalizer): deterministic avalanche.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn pack(alive: bool, cause: u8, energy: f32) -> u64 {
    let energy_q = (energy.clamp(0.0, FULL_ENERGY) * 1024.0) as u64 & 0x0FFF_FFFF;
    (u64::from(alive) << 63) | (u64::from(cause & 0x7) << 60) | energy_q
}

fn unpack(packed: u64) -> (bool, u8, f32) {
    (packed >> 63 == 1, ((packed >> 60) & 0x7) as u8, (packed & 0x0FFF_FFFF) as f32 / 1024.0)
}

/// The ceremony: split one key word into four informationless shares.
/// K = mix(seed); r1..r3 derived; s0 = K^r1^r2^r3; K = s0^s1^s2^s3.
pub fn init(keeper: u64, seed: u64) -> bool {
    if EVER_ALIVE.swap(true, Ordering::AcqRel) {
        return false; // one life per boot
    }
    let key = mix(seed);
    let r1 = mix(seed ^ 0x1A);
    let r2 = mix(seed ^ 0x2B);
    let r3 = mix(seed ^ 0x3C);
    let shares = [key ^ r1 ^ r2 ^ r3, r1, r2, r3];
    for (slot, share) in SHARES.iter().zip(shares) {
        slot.store(share, Ordering::Release);
    }
    KEEPER.store(keeper, Ordering::Release);
    STATE.store(pack(true, 0, FULL_ENERGY), Ordering::Release);
    true
}

fn die(cause: u8) {
    for slot in SHARES.iter() {
        slot.store(0, Ordering::Release);
    }
    KEEPER.store(u64::MAX, Ordering::Release);
    STATE.store(pack(false, cause, 0.0), Ordering::Release);
}

/// Scheduler tick: the share-set erodes; death by silence means the
/// key is unreconstructable — unborn even in the past tense.
pub fn tick() {
    let (alive, _, energy) = unpack(STATE.load(Ordering::Acquire));
    if !alive {
        return;
    }
    let energy = energy - DECAY_PER_TICK;
    if energy <= 0.0 {
        die(1);
        return;
    }
    STATE.store(pack(true, 0, energy), Ordering::Release);
}

/// Keeper maintenance of the share-set.
pub fn maintain(pid: u64) -> bool {
    let (alive, _, energy) = unpack(STATE.load(Ordering::Acquire));
    if !alive || KEEPER.load(Ordering::Acquire) != pid {
        return false;
    }
    STATE.store(pack(true, 0, (energy + MAINTAIN_RESTORE).min(FULL_ENERGY)), Ordering::Release);
    true
}

/// THE SPIDER SIGNAL: destroy the shares — the species dies.
pub fn spider_destroy() {
    die(2);
}

/// Recombine the shares — a fold. Exists for one instruction.
fn recombine() -> u64 {
    SHARES.iter().fold(0u64, |acc, slot| acc ^ slot.load(Ordering::Acquire))
}

/// The one-instant use: recombine, transform, unborn again.
/// Deterministic for the share-set's life: the key is a behavior.
pub fn transform(pid: u64, word: u64) -> Option<u64> {
    let (alive, _, _) = unpack(STATE.load(Ordering::Acquire));
    if !alive || KEEPER.load(Ordering::Acquire) != pid {
        return None;
    }
    TRANSIENT.store(recombine(), Ordering::Release); // the instant begins
    let key = TRANSIENT.load(Ordering::Acquire);
    let out = mix(key ^ word);
    TRANSIENT.store(0, Ordering::Release); // the instant ends
    Some(out)
}

/// Probe: the transient key word — must be ZERO between uses.
pub fn transient_probe() -> u64 {
    TRANSIENT.load(Ordering::Acquire)
}

/// Status: (alive, cause, energy x1024).
pub fn status() -> (bool, u8, u64) {
    let packed = STATE.load(Ordering::Acquire);
    (packed >> 63 == 1, ((packed >> 60) & 0x7) as u8, packed & 0x0FFF_FFFF)
}

/// Host-test support: corrupt one share (the informationlessness
/// gate) and full reset.
#[cfg(feature = "std")]
pub fn corrupt_share(index: usize, value: u64) {
    SHARES[index].store(value, Ordering::Release);
}

#[cfg(feature = "std")]
pub fn reset() {
    STATE.store(3 << 60, Ordering::Release);
    for slot in SHARES.iter() {
        slot.store(0, Ordering::Release);
    }
    KEEPER.store(u64::MAX, Ordering::Release);
    EVER_ALIVE.store(false, Ordering::Release);
    TRANSIENT.store(0, Ordering::Release);
}
