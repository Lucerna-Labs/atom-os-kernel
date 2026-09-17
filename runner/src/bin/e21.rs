//! E21 — the spider wakes: the shadow web's host gate.
//!
//! Runs the kernel-sense sensor exactly as the kernel will use it:
//! learn a clean boot's conversation map, freeze the cone, then verify
//! the spider's three behaviors — clean tasks admit, a rogue
//! conversation is condemned by accumulated budget, a single stray
//! event does not doom anyone, and quiet scars erode. The in-kernel
//! wiring mirrors these calls at the syscall chokepoint.

use kernel_sense::{foreign_budget, freeze, learning, quarantined, record, reset, status};

/// A clean boot: a few pids talking to their usual partners through
/// their usual syscalls (IPC send, write), at a steady rate.
fn clean_boot(events: usize) {
    for i in 0..events {
        let pid = (i % 3) as u64 + 1;
        let target = ((i / 3) % 2) as u64 + 1;
        let syscall = if i % 2 == 0 { 15 } else { 5 };
        record(pid, syscall, target, 1.0);
    }
}

fn main() {
    // -- S1: learning, freeze, clean admission ----------------------
    reset();
    // Stay under the 4096-event auto-freeze horizon so S1 exercises
    // the explicit freeze path; the auto path gets its own check in
    // the kernel-sense unit tests.
    clean_boot(2000);
    assert!(learning());
    freeze();
    let (trained, events, raised, readable) = status();
    println!("S1 learning: trained={trained} events={events} raised_sites={raised} readable={readable}");
    assert!(trained && raised > 0 && readable > 0);
    clean_boot(4000);
    for pid in 1..=3 {
        assert_eq!(foreign_budget(pid), 0.0, "clean pid {pid} flagged");
        assert!(!quarantined(pid));
    }
    println!("S1 PASS: the cone admits every clean conversation post-freeze");

    // -- S2: the rogue conversation is condemned ---------------------
    for _ in 0..400 {
        record(9, 15, 7, 1.0); // pid 9 talking to a partner no boot used
    }
    let budget = foreign_budget(9);
    println!("S2 rogue: pid 9 foreign budget {budget:.2}");
    assert!(budget > 2.0 && quarantined(9));
    println!("S2 PASS: the spider condemns the rogue conversation");

    // -- S3: a budget, not a threshold --------------------------------
    record(4, 15, 9, 1.0); // one stray event from a benign pid
    assert!(!quarantined(4));
    println!("S3 PASS: one stranger event does not doom a pid");

    // -- S4: quiet scars erode ----------------------------------------
    reset();
    for _ in 0..2000 {
        record(1, 15, 2, 1.0);
    }
    let (_t, _e, raised_before, _r) = status();
    for _ in 0..300_000 {
        record(0, 0, 0, 0.0);
    }
    let (_t2, _e2, raised_after, _r2) = status();
    println!("S4 forgetting: raised sites {raised_before} -> {raised_after}");
    assert_eq!(raised_after, 0);
    println!("S4 PASS: unmaintained scars erode (thermodynamic forgetting)");

    println!("\nE21 HOST PASS: the shadow web learns, admits, condemns, and forgets");
}
