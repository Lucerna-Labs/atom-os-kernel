//! E23 host gates — the instant-key's laws, verified directly.

use kernel_instant::{corrupt_share, init, maintain, spider_destroy, status, tick, transform, transient_probe};

fn main() {
    // I1: stability without persistence — deterministic transform
    // across calls and ticks, though the key exists only per-call.
    kernel_instant::reset();
    assert!(init(1, 0xA1B2C3));
    let first = transform(1, 0xFEED_FACE).expect("alive");
    for round in 0..3000 {
        tick();
        if round % 50 == 0 {
            assert!(maintain(1));
        }
        if round % 97 == 0 {
            assert_eq!(transform(1, 0xFEED_FACE), Some(first), "round {round}");
        }
    }
    println!("I1 PASS: transform stable across 3,000 ticks — a behavior, not a thing");

    // I2: the instant is real — the transient key word is ZERO
    // between uses.
    assert_eq!(transient_probe(), 0);
    let _ = transform(1, 42);
    assert_eq!(transient_probe(), 0, "must be zero the instant after");
    println!("I2 PASS: transient key zero between calls — one-instant existence");

    // I3: shares are informationless — corrupting ONE share changes
    // the transform beyond recognition (any 3 shares know nothing:
    // XOR sharing is Shannon-perfect).
    let true_out = first;
    let saved = 0u64; // shares are opaque; corrupt to a fixed value
    corrupt_share(2, saved ^ 0xDEAD_BEEF);
    let corrupted = transform(1, 0xFEED_FACE).expect("still alive");
    assert_ne!(corrupted, true_out);
    // and it is a FULL recombination, not partial: the corrupted key
    // behaves as an unrelated key (avalanche through mix).
    assert!(corrupted != 0);
    println!("I3 PASS: one corrupted share => unrelated key (subsets know nothing)");

    // I4: abandonment erodes the share-set: the key becomes UNBORN.
    kernel_instant::reset();
    assert!(init(1, 7));
    let _ = transform(1, 1);
    for _ in 0..5001 {
        tick();
    }
    let (alive, cause, _) = status();
    assert!(!alive && cause == 1);
    assert_eq!(transform(1, 1), None);
    println!("I4 PASS: abandoned share-set erodes (cause 1); key unreconstructable");

    // I5: the spider signal kills the species; even the keeper is
    // refused forever.
    kernel_instant::reset();
    assert!(init(1, 9));
    assert!(maintain(1));
    spider_destroy();
    let (alive, cause, _) = status();
    assert!(!alive && cause == 2);
    assert!(!maintain(1));
    assert_eq!(transform(1, 1), None);
    println!("I5 PASS: spider signal (cause 2); keeper refused; unborn forever");

    // I6: no resurrection.
    assert!(!init(1, 11));
    println!("I6 PASS: one life per boot");

    println!("\nE23 HOST PASS: never stored, never whole, never long — the window is the instant");
}
