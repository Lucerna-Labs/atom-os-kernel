//! E36 host gates — the seam detector and the judge.
//!
//! Interruption vs drift: a parasite interleaved into a host pid
//! makes the gap ring bimodal; the judge turns that (plus the
//! existing evidence) into a certified destruction verdict, and
//! ABSTAINS when the evidence is a single burst — the false-positive
//! economy that keeps one bad verdict from costing the whole boot.

use kernel_sense::{
    adjudicate, foreign_budget, freeze, judge_status, quarantined, record, reset, seam_analysis,
    seam_status, take_intrusion_signal, tick, QUARANTINE_BUDGET,
};

/// The proven clean-boot shape (e21's S-series): pids 1-3 talking to
/// their usual partners; the rogue triple (9, 15, 7) is foreign under
/// this map by construction.
fn clean_boot(events: usize) {
    for i in 0..events {
        let pid = (i % 3) as u64 + 1;
        let target = ((i / 3) % 2) as u64 + 1;
        let syscall = if i % 2 == 0 { 15 } else { 5 };
        record(pid, syscall, target, 1.0);
    }
}

fn main() {
    // J1: the seam detector separates parasite interleave from one
    // program's honest spread (and from a metronome).
    {
        let mut parasite = Vec::new();
        for i in 0..48usize {
            if i % 3 == 0 {
                parasite.push(20 + (i * 5 % 7) as u64);
            } else {
                parasite.push(2 + (i * 3 % 5) as u64);
            }
        }
        let (bimodal, hole, _) = seam_analysis(&parasite);
        assert!(bimodal && !hole, "interleaved parasite must be bimodal");
        let varied: Vec<u64> = (0..48).map(|i| 5 + (i * 13 % 17) as u64).collect();
        let (b2, _, _) = seam_analysis(&varied);
        assert!(!b2, "an organic spread is one mode");
        let (b3, _, _) = seam_analysis(&[13u64; 48]);
        assert!(!b3, "a metronome is one mode");
        println!("J1 PASS: parasite interleave is bimodal; organic and metronome are not");
    }

    // J2: an interruption is a hole, and one outlier is not a mode.
    {
        let mut gapped = vec![3u64; 47];
        gapped.push(60);
        let (bimodal, hole, _) = seam_analysis(&gapped);
        assert!(hole, "a 20x-median gap is an interruption");
        assert!(!bimodal, "min-cluster rule: one outlier is not a seam");
        println!("J2 PASS: interruption = hole; outlier does not fake a second mode");
    }

    // J3: rhythm drift certifies immediately (the latch already spoke).
    {
        reset();
        for _ in 0..2000 {
            record(1, 15, 2, 1.0);
            record(2, 15, 2, 1.0);
            record(3, 15, 2, 1.0);
        }
        freeze();
        // Mature pid 7 organically: 54 varied gaps.
        for i in 0..54u64 {
            for _ in 0..(5 + (i * 13 % 17)) {
                tick();
            }
            record(7, 5, 2, 1.0);
        }
        let (_matured, drifted_before, _, _) = kernel_sense::rhythm_status(7);
        assert!(!drifted_before, "organic phase must not drift");
        // Beacon: constant gaps collapse PE; drift latches and fires.
        for _ in 0..60u64 {
            for _ in 0..13 {
                tick();
            }
            record(7, 5, 2, 1.0);
        }
        let (_m, drifted, _, _) = kernel_sense::rhythm_status(7);
        assert!(drifted, "beacon must latch drift");
        let signal = take_intrusion_signal();
        assert_eq!(signal, Some(7), "drift must fire the signal");
        assert!(adjudicate(7), "a latched drift verdict certifies");
        let (certified, _, _) = judge_status(7);
        assert!(certified);
        println!("J3 PASS: drift latch fires the signal and certifies");
    }

    // J4: the re-condemning rogue needs three episodes; a single
    // false-positive burst stops at one and never certifies.
    {
        reset();
        clean_boot(2000);
        freeze();
        // Age the sensor clock past EPISODE_GAP so the first hearing
        // can count its episode (the kernel's clock runs from tick 1).
        for _ in 0..40 {
            tick();
        }
        // One benign burst: condemned once, then it stops misbehaving.
        let mut guard = 0;
        while !quarantined(9) {
            record(9, 15, 7, 1.0);
            guard += 1;
            assert!(guard < 100_000, "condemnation must arrive");
        }
        assert!(take_intrusion_signal().is_some());
        assert!(!adjudicate(9), "episode 1 alone must ABSTAIN");
        let (certified, _, episodes) = judge_status(9);
        assert!(!certified && episodes == 1);
        println!("J4a PASS: single burst -> episode 1 -> ABSTAIN (keys would live)");

        // The rogue: condemn, release, re-condemn — twice more. The
        // clock must age EPISODE_GAP between hearings, but ticking
        // during recharge cancels the charge (0.013/tick decay vs
        // 0.02/event) — so age the clock while starved, recharge with
        // only a light tick cadence.
        for cycle in 2..=3u32 {
            for _ in 0..48 {
                tick(); // starved/release: budget erodes, clock ages
            }
            let mut guard = 0;
            while !quarantined(9) {
                record(9, 15, 7, 1.0);
                guard += 1;
                assert!(guard < 100_000);
                if guard % 10 == 0 {
                    tick(); // light cadence: net charge stays positive
                }
            }
            assert!(take_intrusion_signal().is_some());
            let verdict = adjudicate(9);
            let (certified, _, episodes) = judge_status(9);
            assert_eq!(episodes, cycle, "episode must count once per cycle");
            if cycle < 3 {
                assert!(!verdict && !certified, "episodes < 3 must abstain");
            } else {
                assert!(verdict && certified, "the third episode certifies");
            }
        }
        println!("J4b PASS: episodes 2 -> abstain, 3 -> certified (the re-condemner)");
    }

    // J5: abstention is economical — a false positive's single burst
    // holds, never certifies, and the keys it could have killed stay
    // alive.
    {
        reset();
        clean_boot(2000);
        freeze();
        for _ in 0..40 {
            tick();
        }
        kernel_key::reset();
        assert!(kernel_key::init(77, [1, 2, 3, 4]), "keeper init");
        let mut guard = 0;
        while !quarantined(9) {
            record(9, 15, 7, 1.0);
            guard += 1;
            assert!(guard < 100_000, "condemnation must arrive");
        }
        assert!(take_intrusion_signal().is_some());
        assert!(!adjudicate(9), "one burst is one episode: ABSTAIN");
        let (certified, _, episodes) = judge_status(9);
        assert!(!certified && episodes == 1);
        // The misbehavior stops here. No new signal ever comes; even
        // a stale signal re-heard while over bar cannot reach three.
        let (alive, _, _) = kernel_key::status();
        assert!(alive, "a held docket must leave the key alive");
        println!("J5 PASS: hold leaves the perishable key alive (false-positive economy)");
    }

    // J6: the parasite seam certifies on its FIRST condemnation —
    // bimodality upgrades one episode into a verdict — while the
    // same pid's drift latch stays quiet (PE stays organic).
    {
        // Site foreignness is hash-dependent per (pid, target,
        // syscall): probe for a subject whose rhythm triple is clean
        // AND whose burst triple is foreign, so the gate is
        // deterministic under any site_for change.
        let mut subject = 0u64;
        for candidate in 4..64u64 {
            reset();
            clean_boot(2000);
            freeze();
            record(candidate, 5, 2, 1.0);
            let rhythm_clean = foreign_budget(candidate) == 0.0;
            record(candidate, 15, 7, 1.0);
            let burst_foreign = foreign_budget(candidate) > 0.0;
            if rhythm_clean && burst_foreign {
                subject = candidate;
                break;
            }
        }
        assert_ne!(subject, 0, "probe must find a clean-rhythm/foreign-burst pid");

        reset();
        clean_boot(2000);
        freeze();
        // Drain the wire of earlier gates' stale signals: the seam
        // must fire a FRESH one.
        while take_intrusion_signal().is_some() {}
        // Organic maturation.
        for i in 0..54u64 {
            for _ in 0..(5 + (i * 13 % 17)) {
                tick();
            }
            record(subject, 5, 2, 1.0);
        }
        // The parasite interleave, BALANCED so the mixture's PE stays
        // organic (a burst-dominant parasite is DRIFT's prey — the
        // seam's distinct case is the balanced one): per cycle, two
        // host beats at the clean site (gaps 3-6), one quiet spell
        // (~20-23), then a dense foreign triple (0-gap). The gap ring
        // holds three well-separated populations (0s, 2-6s, 20s);
        // the seam latch fires its own signal; the judge certifies on
        // seam + the foreign-budget trace the triple leaves behind.
        let mut certified = false;
        let mut cycles = 0u64;
        while !certified && cycles < 16 {
            for _ in 0..(3 + (cycles * 3 % 4)) {
                tick();
            }
            record(subject, 5, 2, 1.0); // host beat: clean site
            for _ in 0..(3 + (cycles * 5 % 4)) {
                tick();
            }
            record(subject, 5, 2, 1.0); // host beat: clean site
            for _ in 0..(20 + (cycles * 7 % 4)) {
                tick();
            }
            record(subject, 15, 7, 1.0); // parasite quintuple: foreign site
            record(subject, 15, 7, 1.0);
            record(subject, 15, 7, 1.0);
            record(subject, 15, 7, 1.0);
            record(subject, 15, 7, 1.0);
            cycles += 1;
            // The seam fires its own signal on the cadence; the
            // scheduler (here, the gate) consumes and adjudicates.
            if let Some(heard) = take_intrusion_signal() {
                assert_eq!(heard, subject, "the seam must fire for the parasitized pid");
                certified = adjudicate(heard);
            }
        }
        assert!(certified, "seam + budget trace must certify within 16 cycles");
        let (_m, drifted, _, _) = kernel_sense::rhythm_status(subject);
        assert!(!drifted, "mixture PE stays organic: drift is blind here");
        let (bimodal, _hole) = seam_status(subject);
        // NOTE: hole is EXPECTED here — a zero-dominant ring has a
        // zero median, so every quiet spell reads as a pause. The
        // kernel never gates on hole; it is the interruption receipt,
        // not a parasite discriminator.
        assert!(bimodal, "the seam sees the interleave");
        let (certified2, seam_heard, episodes) = judge_status(subject);
        assert!(certified2 && seam_heard && episodes == 0);
        let budget = foreign_budget(subject);
        assert!(budget > 0.05 && budget < QUARANTINE_BUDGET,
            "trace budget {budget} must sit between the seam bar and the quarantine bar");
        println!("J6 PASS: seam fired the signal; seam + trace certified (drift stayed blind)");

    println!("\nE36 HOST PASS: the seam sees interleave; the judge destroys only on proof");
    }
}
