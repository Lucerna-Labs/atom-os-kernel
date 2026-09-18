//! E36 seamwalk payload — the seam detector and the judge, live.
//!
//! Phase 1 (organic): the pid earns a mature rhythm baseline, clean.
//! Phase 2 (parasite interleave): between organic host beats, a
//! simulated injected parasite bursts densely at foreign conversation
//! sites after a quiet spell — the gap ring goes bimodal (three
//! separated populations: burst zeros, host beats, quiet spells)
//! while the mixture's PE stays organic, so DRIFT stays blind. The
//! seam latch fires the intrusion signal itself; the judge certifies
//! on seam + the foreign-budget trace, and the destruction cascade
//! runs only then.
//!
//! Honest labels: the "parasite" is this payload's own foreign
//! syscalls (SYS_FS_STAT with unknown subcommands — instant, no side
//! effects, spatially foreign); the demo certifies the MECHANISM,
//! not a real injection.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn sense(sub: u64, pid: u64) -> u64 {
    rt::call3(SYS_SENSE, sub, pid, 0)
}

fn main() {
    rt::sleep(100); // let the boot train and freeze the spatial cone
    let me = rt::call(SYS_GETPID, 0, 0);

    // Phase 1: organic maturation — varied gaps, ~56 events.
    for i in 0..56u64 {
        rt::sleep(5 + (i * 13 % 17));
        let _ = rt::call(SYS_GETPID, 0, 0);
    }
    let rhythm = sense(6, me);
    let matured = rhythm >> 63 == 1;
    let drifted = (rhythm >> 62) & 1 == 1;
    let seam = sense(7, me);
    let bimodal0 = seam >> 63 == 1;
    rt::print_args(format_args!(
        "[Seam] organic phase: matured={} drifted={} bimodal={}\n",
        matured, drifted, bimodal0
    ));
    if !matured || drifted || bimodal0 {
        rt::print("[Seam] FAIL: bad organic baseline\n");
        rt::exit(6);
    }

    // Phase 2: the parasite interleave. Structured so the INTENDED
    // gaps dominate the ring: every rt::sleep is itself a syscall
    // event, so each sleep contributes one 0-gap before its wait —
    // bursts must stay small or the zeros flood the window. Per
    // cycle: foreign triple (0-gaps), short host sleep (3-7), foreign
    // triple, long quiet (26-29). The ring settles into three
    // separated populations: zeros, host-scale, quiet-scale.
    let mut certified = false;
    let mut cycles = 0u64;
    while !certified && cycles < 24 {
        // parasite burst: SYS_FS_STAT with unknown subs — instant,
        // side-effect-free, spatially foreign.
        let _ = rt::call3(SYS_FS_STAT, 77, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 88, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 99, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 111, 0, 0);
        rt::sleep(3 + (cycles * 3 % 5)); // host-scale gap
        let _ = rt::call3(SYS_FS_STAT, 77, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 88, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 99, 0, 0);
        let _ = rt::call3(SYS_FS_STAT, 111, 0, 0);
        rt::sleep(34 + (cycles * 7 % 4)); // quiet-scale gap
        cycles += 1;
        if cycles % 4 == 0 {
            // The scheduler consumes the seam's signal each tick and
            // adjudicates; the docket is readable here.
            let docket = sense(8, me);
            certified = docket >> 63 == 1;
        }
    }
    let rhythm2 = sense(6, me);
    let drifted2 = (rhythm2 >> 62) & 1 == 1;
    let seam2 = sense(7, me);
    let bimodal = seam2 >> 63 == 1;
    let docket = sense(8, me);
    let seam_heard = (docket >> 62) & 1 == 1;
    let episodes = docket & 0xFF;
    rt::print_args(format_args!(
        "[Seam] interleave done: cycles={} DRIFTED={} bimodal={} seam_heard={} episodes={}\n",
        cycles, drifted2, bimodal, seam_heard, episodes
    ));
    if !certified || drifted2 || !bimodal || !seam_heard {
        rt::print("[Seam] FAIL: seam path did not certify (or drift fired first)\n");
        rt::exit(7);
    }
    rt::print("[Seam] seam fired; judge certified on seam + foreign trace\n");
    rt::print("[Seam] E36 PASS: the seam sees interleave; the judge destroys only on proof\n");
    rt::exit(0);
}
