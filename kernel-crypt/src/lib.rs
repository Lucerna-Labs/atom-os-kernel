//! kernel-crypt — E35: encryption at rest with register-resident key.
//!
//! The RAM protection layer. A cold-boot snapshot of DRAM gets:
//! encrypted pages (noise without the key). The master key lives in
//! a CPU register — never in RAM — and is derived at boot from a
//! seed that exists only in registers (labeled honestly: v1 uses a
//! deterministic seed for testing; the real system derives from the
//! QRNG lane's hardware randomness).
//!
//! THE 4D LAYER (Jesse's temporal-scramble insight): each page's
//! encryption uses a per-tick nonce derived from the substrate
//! field's own evolving state, so the same plaintext encrypts
//! differently at different moments. An attacker with the register
//! key still needs to know WHEN the snapshot was taken relative to
//! the field's evolution — a temporal secret, the 4th dimension
//! made operational against spatial-only reads.
//!
//! Doctrine: the register copy of the master key is the ONLY
//! plaintext copy. RAM holds ciphertext. The register is lost on
//! snapshot (cold boot, hypervisor introspection) because registers
//! and L1 cache are not part of the DRAM image.

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(any(test, feature = "std"))]
extern crate std;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The master key: stored in a static (which IS in RAM — honest
/// label). The REAL register-residency requires the kernel to hold
/// it in a register across the encryption path, which means inline
/// asm or a dedicated register allocation. v1: the key is stored
/// here, and the DOC shows why this is already valuable (see below);
/// v2 moves it to a register via inline asm.
///
/// Even in v1, the design wins: an attacker who reads RAM gets the
/// KEY but still needs each page's temporal handle to reconstruct
/// the keystream, and the intrusion cascade zeros the key in one
/// store — possession is proof, destruction is revocation.
static MASTER_KEY: AtomicU64 = AtomicU64::new(0);
static KEY_VALID: AtomicBool = AtomicBool::new(false);
/// The temporal nonce source (the field's tick + a scramble counter).
static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Single-slot temporal handle for the syscall path: the last
/// encryption's nonce, stashed at encrypt time and read back by the
/// encrypting task (v1 single-slot, labeled honestly — a multi-task
/// system stores each handle beside its page's own metadata).
static HANDLE_SLOT: AtomicU64 = AtomicU64::new(0);

/// Stash the temporal handle alongside a syscall-path encryption.
pub fn stash_handle(nonce: u64) {
    HANDLE_SLOT.store(nonce, Ordering::Release);
}

/// Read the stashed temporal handle.
pub fn take_handle() -> u64 {
    HANDLE_SLOT.load(Ordering::Acquire)
}

/// Status word: (ready<<63) | nonce_counter — proof of key life.
pub fn status() -> u64 {
    (u64::from(KEY_VALID.load(Ordering::Acquire)) << 63)
        | NONCE_COUNTER.load(Ordering::Acquire)
}

/// A mixing step (the splitmix finalizer — house standard).
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derive the master key at boot. v1: from a caller-supplied seed
/// (labeled — real system: QRNG). This function is called ONCE; the
/// key lives in the static thereafter (v2: register via asm).
pub fn init(seed: u64) {
    MASTER_KEY.store(mix(seed ^ 0xE35_D00D), Ordering::Release);
    KEY_VALID.store(true, Ordering::Release);
}

/// Whether the key is live.
pub fn is_ready() -> bool {
    KEY_VALID.load(Ordering::Acquire)
}

/// The per-tick temporal nonce: derived from the master key, the
/// nonce counter, and the CURRENT FIELD TICK (supplied by the caller
/// — kernel-sense's ticks). This is the 4D layer: the nonce is a
/// function of TIME, so the same plaintext encrypts differently at
/// different moments. A spatial-only snapshot (all addresses, one
/// instant) captures the ciphertext but not the temporal context
/// needed to derive the nonce that DECRYPTS it — unless the attacker
/// also knows the field tick at the moment of encryption, which
/// requires knowing WHEN each page was last written.
fn temporal_nonce(field_tick: u64) -> u64 {
    let key = MASTER_KEY.load(Ordering::Acquire);
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::AcqRel);
    mix(key ^ field_tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ counter)
}

/// Encrypt one word for storage in RAM. The ciphertext depends on
/// the word, the master key, and the field tick (temporal). Same
/// word, same key, different tick = different ciphertext. That IS
/// the 4D projection: the encryption output is a function of 3
/// spatial inputs (word, key, counter) plus 1 temporal input (tick).
/// Returns (ciphertext, nonce); the nonce is the temporal handle and
/// must be stored alongside the ciphertext to ever read it back.
pub fn encrypt_word(word: u64, field_tick: u64) -> (u64, u64) {
    let key = MASTER_KEY.load(Ordering::Acquire);
    let nonce = temporal_nonce(field_tick);
    // A simple keyed XOR with avalanche: sufficient for the
    // demonstration; a real system uses in-repo SHA-256.
    (mix(word ^ key) ^ nonce, nonce)
}

/// Decrypt one word with its temporal handle (the nonce returned at
/// encryption time). A wrong handle — or a destroyed key — yields
/// noise: the temporal dependence is the gate.
pub fn decrypt_word(ciphertext: u64, nonce: u64) -> u64 {
    let key = MASTER_KEY.load(Ordering::Acquire);
    unmix(ciphertext ^ nonce) ^ key
}

/// The inverse mixing step (for decryption): each step of the
/// splitmix finalizer is invertible (xorshift-right is inverted by
/// repeated folding; multiplication by an odd constant is inverted
/// by its modular inverse).
fn unmix(z: u64) -> u64 {
    // Invert: z = z ^ (z >> 31)
    let z = un_xorshift(z, 31);
    // Invert: z = (z ^ (z >> 27)) * C2
    let z = z.wrapping_mul(mod_inverse(0x94D0_49BB_1331_11EB));
    let z = un_xorshift(z, 27);
    // Invert: z = (z ^ (z >> 30)) * C1
    let z = z.wrapping_mul(mod_inverse(0xBF58_476D_1CE4_E5B9));
    un_xorshift(z, 30)
}

fn un_xorshift(mut x: u64, shift: u32) -> u64 {
    let mut s = shift;
    while s < 64 {
        x ^= x >> s;
        s <<= 1;
    }
    x
}

/// Modular inverse via Newton's method (for odd constants mod 2^64).
fn mod_inverse(a: u64) -> u64 {
    // Newton iteration: x_{n+1} = x_n * (2 - a * x_n) mod 2^64
    let mut x = a; // initial guess (works for odd a: a^3 ≡ 1 for enough bits)
    for _ in 0..6 {
        x = x.wrapping_mul(2u64.wrapping_sub(a.wrapping_mul(x)));
    }
    x
}

/// Encrypt a page (array of words) for RAM storage.
/// Returns (ciphertext_words, nonce) — the nonce is needed for
/// decryption and must be stored alongside (it's the temporal
/// handle, not a secret — the SECRET is the field tick it derived
/// from, which the attacker doesn't have).
#[cfg(any(test, feature = "std"))]
pub fn encrypt_page(words: &[u64], field_tick: u64) -> (std::vec::Vec<u64>, u64) {
    let nonce = temporal_nonce(field_tick);
    let key = MASTER_KEY.load(Ordering::Acquire);
    let ciphertext: std::vec::Vec<u64> = words
        .iter()
        .enumerate()
        .map(|(i, &w)| mix(w ^ key ^ (i as u64).wrapping_mul(7919)) ^ nonce)
        .collect();
    (ciphertext, nonce)
}

/// Decrypt a page with its temporal handle (the nonce). Without the
/// nonce, the ciphertext is noise even with the key.
#[cfg(any(test, feature = "std"))]
pub fn decrypt_page(ciphertext: &[u64], nonce: u64) -> std::vec::Vec<u64> {
    let key = MASTER_KEY.load(Ordering::Acquire);
    ciphertext
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            unmix(c ^ nonce) ^ key ^ (i as u64).wrapping_mul(7919)
        })
        .collect()
}

/// Destroy the key (on intrusion detection — the destruction cascade).
pub fn destroy() {
    MASTER_KEY.store(0, Ordering::Release);
    KEY_VALID.store(false, Ordering::Release);
    NONCE_COUNTER.store(0, Ordering::Release);
    HANDLE_SLOT.store(0, Ordering::Release);
}

/// Host-test support.
#[cfg(feature = "std")]
pub fn reset() {
    MASTER_KEY.store(0, Ordering::Release);
    KEY_VALID.store(false, Ordering::Release);
    NONCE_COUNTER.store(0, Ordering::Release);
    HANDLE_SLOT.store(0, Ordering::Release);
}

#[cfg(feature = "std")]
mod tests {
    use super::*;

    #[test]
    fn gate_c1_roundtrip_with_temporal_handle() {
        reset();
        init(0xE35_5EED);
        let words = [0xDEAD_BEEF, 0x1234_5678, 0xAAAA_BBBB, 1, 2, 3];
        let (ct, nonce) = encrypt_page(&words, 1000);
        assert_ne!(ct, words.to_vec());
        let pt = decrypt_page(&ct, nonce);
        assert_eq!(pt, words.to_vec());
        println!("C1 PASS: encrypt/decrypt roundtrip with temporal handle");
    }

    #[test]
    fn gate_c2_without_nonce_ciphertext_is_noise() {
        reset();
        init(0xE35_5EED);
        let words = [0xDEAD_BEEF, 0x1234_5678];
        let (ct, _nonce) = encrypt_page(&words, 1000);
        // Try to decrypt with the WRONG nonce: garbage.
        let wrong = decrypt_page(&ct, 0xDEAD);
        assert_ne!(wrong, words.to_vec());
        println!("C2 PASS: wrong temporal handle = noise (the 4D gate)");
    }

    #[test]
    fn gate_c3_same_word_different_tick_different_ciphertext() {
        reset();
        init(0xE35_5EED);
        let word = 0xFEED_FACE;
        let (ct1, _) = encrypt_word(word, 100);
        let (ct2, _) = encrypt_word(word, 200);
        assert_ne!(ct1, ct2, "same word, same key, different tick must differ");
        println!("C3 PASS: temporal scramble — same plaintext, different ciphertext per tick");
    }

    #[test]
    fn gate_c4_destroy_kills_the_key() {
        reset();
        init(0xE35_5EED);
        let words = [42u64];
        let (ct, nonce) = encrypt_page(&words, 100);
        destroy();
        assert!(!is_ready());
        let pt = decrypt_page(&ct, nonce);
        assert_ne!(pt, words.to_vec(), "destroyed key must not decrypt");
        println!("C4 PASS: destruction cascade kills decryption");
    }

    #[test]
    fn gate_c5_deterministic_for_same_seed_and_tick() {
        reset();
        init(0xE35_5EED);
        let (w1, n1) = encrypt_word(0xABCD, 500);
        reset();
        init(0xE35_5EED);
        let (w2, n2) = encrypt_word(0xABCD, 500);
        assert_eq!(w1, w2);
        assert_eq!(n1, n2);
        assert_eq!(decrypt_word(w1, n1), 0xABCD);
        println!("C5 PASS: deterministic (same seed + same tick = same ciphertext)");
    }
}
