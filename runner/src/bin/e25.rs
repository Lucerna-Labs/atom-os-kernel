//! E25 host gates — the lane's laws, verified directly.

use kernel_lane::{claim, deposit, handoff, is_revoked, log_get, reset, revoke, status, Handoff};

fn main() {
    // L1: one claim per boot; second claimant refused.
    reset();
    assert!(claim(7));
    assert!(!claim(9));
    println!("L1 PASS: lane claimed once; second claimant refused");

    // L2: the legal cycle — deposit, handoff real, logged, Empty after.
    assert!(deposit(7, 0xA1, 1));
    match handoff(7) {
        Handoff::Real { word, transit, .. } => {
            assert_eq!(word, 0xA1);
            assert_eq!(transit, 0);
        }
        _ => panic!("trusted handoff must be real"),
    }
    assert!(matches!(handoff(7), Handoff::Nothing));
    let (kind, word) = log_get(0);
    assert_eq!((kind, word), (kernel_lane::KIND_REAL, 0xA1));
    println!("L2 PASS: deposit -> real handoff -> logged -> Empty");

    // L3: non-lane requester receives honey, not the real word, and
    // the material stays Holding.
    assert!(deposit(7, 0xB2, 2));
    match handoff(9) {
        Handoff::Honey { word, transit } => {
            assert_ne!(word, 0xB2);
            assert_eq!(transit, 1);
        }
        _ => panic!("untrusted handoff must be honey"),
    }
    match handoff(7) {
        Handoff::Real { word, .. } => assert_eq!(word, 0xB2),
        _ => panic!("real word must still be carried after honey"),
    }
    println!("L3 PASS: unauthorized requester gets honey; real B still travels after");

    // L4: revocation is permanent — after revoke, even the lane task
    // gets honey, and deposit fails closed.
    assert!(deposit(7, 0xC3, 3));
    revoke();
    assert!(is_revoked());
    assert!(!deposit(7, 0xD4, 4), "revoked lane must not accept deposits");
    match handoff(7) {
        Handoff::Honey { word, .. } => assert_ne!(word, 0xC3),
        _ => panic!("revoked lane must serve honey, never C3"),
    }
    println!("L4 PASS: revoked lane serves honey forever; deposits fail closed");

    // L5: the log tells the truth: [real A, honey, real B, honey].
    assert_eq!(log_get(0), (kernel_lane::KIND_REAL, 0xA1));
    let (k1, _) = log_get(1);
    assert_eq!(k1, kernel_lane::KIND_HONEY);
    let (k2, w2) = log_get(2);
    assert_eq!((k2, w2), (kernel_lane::KIND_REAL, 0xB2));
    let (k3, _) = log_get(3);
    assert_eq!(k3, kernel_lane::KIND_HONEY);
    let (_, _, revoked, real, honey) = status();
    assert!(revoked && real == 2 && honey == 2);
    println!("L5 PASS: transit log exact (2 real, 2 honey); status agrees");

    // L6: honey is deterministic per transit — consistent garbage.
    reset();
    assert!(claim(5));
    assert!(deposit(5, 0xE5, 5));
    let a = match handoff(6) {
        Handoff::Honey { word, .. } => word,
        _ => panic!("honey expected"),
    };
    reset();
    assert!(claim(5));
    assert!(deposit(5, 0xE5, 5));
    let b = match handoff(6) {
        Handoff::Honey { word, .. } => word,
        _ => panic!("honey expected"),
    };
    assert_eq!(a, b);
    println!("L6 PASS: honey deterministic across runs");

    println!("\nE25 HOST PASS: one way out, watched, honey for the untrusted");
}
