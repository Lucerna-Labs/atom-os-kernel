//! E24 host gates — the egress cone's two bounds, verified directly.

use kernel_egress::{force_freeze, gate, reset, status};

fn prose(n: usize) -> Vec<u8> {
    // Real-ish boot-log prose, deterministic.
    let template = b"[Spider] shadow-web probe started: the cone is frozen and the rogue is condemned by the sensor. ";
    template.iter().copied().cycle().take(n).collect()
}

fn main() {
    // Train on legitimate prose, then freeze.
    reset();
    for chunk in prose(64 * 400).chunks(64) {
        assert!(gate(chunk));
    }
    let (frozen, windows, ceiling, _, _) = status();
    assert!(frozen && windows >= 256 && ceiling > 0);
    println!("trained on prose: {windows} windows, distinct max {ceiling}");

    // G1: prose passes post-freeze.
    assert!(gate(&prose(256)));
    println!("G1 PASS: prose passes");

    // G2: raw key-shaped bytes refused (distinct-count ceiling).
    let key: Vec<u8> = (0..64u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    assert!(!gate(&key));
    println!("G2 PASS: raw key material refused (noise class)");

    // G3: hex encoding refused by the structure profile.
    let hex: Vec<u8> = key.iter().map(|b| b"0123456789abcdef"[(b >> 4) as usize]).collect();
    let hex2: Vec<u8> = key.iter().map(|b| b"0123456789abcdef"[(b & 15) as usize]).collect();
    let mut encoded = Vec::new();
    for (a, b) in hex.iter().zip(hex2) { encoded.push(*a); encoded.push(b); }
    assert_eq!(encoded.len(), 128);
    assert!(!gate(&encoded));
    println!("G3 PASS: hex-encoded key refused (no prose structure)");

    // G4: key embedded in prose — the containing window is refused.
    let mut smuggled = prose(128);
    for (slot, byte) in smuggled.iter_mut().skip(64).take(48).zip(&key) {
        *slot = *byte;
    }
    assert!(!gate(&smuggled));
    println!("G4 PASS: key-inside-prose refused (embedding window is noise)");

    // G5: short writes pass (the labeled drip hole — honest, not secret).
    assert!(gate(b"hi"));
    println!("G5 PASS: short writes pass (labeled drip hole)");

    // G6: deterministic — same inputs, same verdicts.
    reset();
    let mut results = Vec::new();
    for run in 0..2 {
        for _ in 0..300 { gate(&prose(64)); }
        force_freeze();
        results.push((gate(&key), gate(&encoded), gate(&prose(128))));
        let _ = run;
        reset();
    }
    assert_eq!(results[0], results[1]);
    println!("G6 PASS: deterministic across retraining");

    let (_, _, _, violations, _) = status();
    println!("violations counted: {violations}");
    println!("\nE24 HOST PASS: entropy ceiling + prose structure — the strainer graduated to bits");
}
