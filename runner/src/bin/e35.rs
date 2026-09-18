//! E35 host gates — encryption at rest with temporal scramble.

use kernel_crypt::{decrypt_page, decrypt_word, destroy, encrypt_page, encrypt_word, init, is_ready, reset};

fn main() {
    // C1: roundtrip with the temporal handle.
    reset();
    init(0xE35_5EED);
    let words = vec![0xDEAD_BEEF, 0x1234_5678, 0xAAAA_BBBB, 1, 2, 3];
    let (ct, nonce) = encrypt_page(&words, 1000);
    assert_ne!(ct, words);
    let pt = decrypt_page(&ct, nonce);
    assert_eq!(pt, words);
    println!("C1 PASS: encrypt/decrypt roundtrip with temporal handle");

    // C2: wrong temporal handle = noise (the 4D gate).
    let wrong = decrypt_page(&ct, 0xDEAD);
    assert_ne!(wrong, words);
    println!("C2 PASS: wrong temporal handle = noise — the 4D layer is the gate");

    // C3: same word, different tick, different ciphertext.
    let (w1, _) = encrypt_word(0xFEED_FACE, 100);
    let (w2, _) = encrypt_word(0xFEED_FACE, 200);
    assert_ne!(w1, w2);
    println!("C3 PASS: temporal scramble — same plaintext, different ciphertext per tick");

    // C4: destruction cascade kills decryption.
    let (ct2, n2) = encrypt_page(&[42u64], 100);
    destroy();
    assert!(!is_ready());
    let pt2 = decrypt_page(&ct2, n2);
    assert_ne!(pt2, vec![42u64]);
    println!("C4 PASS: destroyed key does not decrypt");

    // C5: deterministic for same seed + tick, and the word API roundtrips.
    reset();
    init(0xE35_5EED);
    let (a, na) = encrypt_word(0xABCD, 500);
    assert_eq!(decrypt_word(a, na), 0xABCD);
    reset();
    init(0xE35_5EED);
    let (b, _) = encrypt_word(0xABCD, 500);
    assert_eq!(a, b);
    println!("C5 PASS: deterministic (same seed + same tick = same ciphertext)");

    println!("\nE35 HOST PASS: RAM holds ciphertext; the temporal dimension is the gate");
}
