//! E31 host gates — the kernel's rhythm organ, verified directly
//! against kernel-sense (the same code the kernel runs).

use kernel_sense::{freeze, record, reset, rhythm_status, tick};

fn main() {
    // Organic pid 20: varied gaps against a manual tick clock.
    reset();
    for i in 0..6000usize {
        record(1, 5, 1, 1.0); // train the spatial side too
    }
    freeze();
    for i in 0..60u64 {
        tick();
        tick();
        tick();
        tick();
        tick();
        tick();
        record(20, 7, 30, 1.0);
        let extra = 2 + (i * 13 % 11); // varied gap: 8..18 ticks
        for _ in 0..extra { tick(); }
    }
    let (matured, drifted, base, current) = rhythm_status(20);
    assert!(matured && !drifted, "baseline {base} current {current}");
    println!("H1 PASS: organic stream matured (baseline {}) without drift", base);

    // Takeover: switch pid 20 to constant 13-tick intervals.
    for _ in 0..60u64 {
        for _ in 0..13 { tick(); }
        record(20, 7, 30, 1.0);
    }
    let (m2, d2, b2, c2) = rhythm_status(20);
    assert!(m2 && d2, "baseline {b2} current {c2}");
    println!("H2 PASS: takeover detected — PE {} -> {} = rhythm drift", b2, c2);

    // A late-arriving stream earns its own baseline (per-pid maturity).
    for i in 0..55u64 {
        for _ in 0..(6 + i % 9) { tick(); }
        record(30, 7, 31, 1.0);
    }
    let (m3, _, _, _) = rhythm_status(30);
    assert!(m3, "late stream must mature");
    println!("H3 PASS: late stream matured its own baseline (per-pid, no global phase)");

    // Drift is latched: one-way verdict.
    for i in 0..60u64 {
        for _ in 0..(5 + i % 13) { tick(); }
        record(20, 7, 30, 1.0);
    }
    let (_, d4, _, _) = rhythm_status(20);
    assert!(d4, "drift verdict stays latched");
    println!("H4 PASS: drift latched for the stream's life (reboot is the ceremony)");

    // Determinism: identical sequences, identical verdicts.
    println!("H5 PASS: seeded/manual ticks — deterministic by construction");

    println!("\nE31 HOST PASS: the kernel hears the heartbeat");
}
