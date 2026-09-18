//! E31 metronome payload — the takeover demo.
//!
//! Phase 1 (organic): varied sleeps fill the rhythm window; the
//! baseline matures as ORGANIC. Phase 2 (beacon): constant-interval
//! sleeps fill the window with machine rhythm; PE collapses; drift
//! fires the intrusion signal — the destruction cascade the spider
//! owns (keys die, lane revokes) even though this payload's
//! vocabulary may be entirely spatially normal.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn rhythm(pid: u64) -> (bool, bool, u64, u64) {
    let packed = rt::call3(SYS_SENSE, 6, pid, 0);
    (
        packed >> 63 == 1,
        (packed >> 62) & 1 == 1,
        (packed >> 32) & 0x1F_FF,
        packed & 0x1F_FF,
    )
}

fn main() {
    rt::sleep(100); // let the boot train the spatial field
    let me = rt::call(SYS_GETPID, 0, 0);

    // Phase 1: organic gaps — varied sleeps, ~56 events.
    for i in 0..56u64 {
        rt::sleep(5 + (i * 13 % 17));
        let _ = rt::call(SYS_GETPID, 0, 0); // an event
    }
    let (matured, drifted, base, _) = rhythm(me);
    rt::print_args(format_args!(
        "[Metro] organic phase done: matured={} drifted={} baseline={}\n",
        matured, drifted, base
    ));
    if !matured {
        rt::print("[Metro] FAIL: baseline did not mature\n");
        rt::exit(6);
    }
    if drifted {
        rt::print("[Metro] FAIL: drifted during organic phase\n");
        rt::exit(6);
    }

    // Phase 2: beacon — constant 13-tick intervals, enough to fill
    // the 48-gap window with machine rhythm.
    for _ in 0..60u64 {
        rt::sleep(13);
        let _ = rt::call(SYS_GETPID, 0, 0);
    }
    let (matured2, drifted2, _, current) = rhythm(me);
    rt::print_args(format_args!(
        "[Metro] beacon phase done: matured={} DRIFTED={} current={}\n",
        matured2, drifted2, current
    ));
    if drifted2 {
        rt::print("[Metro] RHYTHM DRIFT DETECTED — takeover signal fired\n");
        rt::print("[Metro] the child's heartbeat changed; the mother heard it — E31 PASS\n");
        rt::exit(0);
    }
    rt::print("[Metro] FAIL: drift not detected\n");
    rt::exit(7);
}
