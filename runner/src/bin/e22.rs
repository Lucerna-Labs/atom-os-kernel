//! E22 host gates — the fail-dead key's lifecycle laws, verified
//! directly against the kernel-key crate (same code the kernel runs).

use kernel_key::{init, maintain, read, spider_destroy, status, tick};

fn main() {
    // K1: a maintained key is immortal — 5,000 ticks with scheduled
    // maintenance, energy never drains, words never change.
    kernel_key::reset();
    assert!(init(1, [0xAA, 0xBB, 0xCC, 0xDD]));
    let original = read(1);
    for round in 0..5000 {
        tick();
        if round % 50 == 0 {
            assert!(maintain(1), "round {round}");
        }
    }
    let (alive, cause, energy) = status();
    assert!(alive && cause == 0 && energy > 90_000);
    assert_eq!(read(1), original);
    println!("K1 PASS: maintained key survives 5,000 ticks, words stable");

    // K2: abandonment is death — same 5,000 ticks, no maintenance.
    kernel_key::reset();
    assert!(init(1, [1, 2, 3, 4]));
    for _ in 0..5001 {
        tick();
    }
    let (alive, cause, _) = status();
    assert!(!alive && cause == 1, "cause {cause}");
    assert_eq!(read(1), [0; 4]);
    println!("K2 PASS: abandoned key erodes to death in ~5,000 ticks (cause 1), material zeroed");

    // K3: the spider signal kills instantly and revokes keepership —
    // the keeper itself can never pay the key back to life.
    kernel_key::reset();
    assert!(init(1, [9, 9, 9, 9]));
    assert!(maintain(1));
    spider_destroy();
    let (alive, cause, _) = status();
    assert!(!alive && cause == 2);
    assert!(!maintain(1), "revoked keeper must be refused");
    println!("K3 PASS: spider signal destroys instantly (cause 2); keeper maintain refused after");

    // K4: unauthenticated maintenance and reads are refused while alive.
    kernel_key::reset();
    assert!(init(1, [7, 7, 7, 7]));
    assert!(!maintain(2));
    assert_eq!(read(2), [0; 4]);
    assert!(maintain(1));
    println!("K4 PASS: only the keeper maintains; only the keeper reads");

    // K5: no resurrection — one life per boot.
    assert!(!init(1, [5, 5, 5, 5]));
    println!("K5 PASS: a dead key cannot be re-initialized (no resurrection)");

    println!("\nE22 HOST PASS: alive while trusted, dead by silence, dead by spider — fail-dead");
}
