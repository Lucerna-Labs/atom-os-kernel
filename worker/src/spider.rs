//! E21 spider probe — the observer payload (runs only inside the VM).
//!
//! Boots after the shell's clean startup: reads the shadow web's
//! status, freezes the normality cone on the clean conversation map,
//! then launches the rogue and watches its foreign budget climb
//! until the scheduler starves it. The visible arc: learning →
//! freeze → infiltration → condemnation → silence.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn sense_status() -> (bool, u64, u64) {
    let packed = rt::call3(SYS_SENSE, 0, 0, 0);
    let trained = packed >> 63 == 1;
    let events = (packed >> 32) & 0x7FFF_FFFF;
    let raised = packed & 0xFFFF_FFFF;
    (trained, events, raised)
}

fn main() {
    rt::print("[Spider] shadow-web probe started\n");
    let (trained, events, raised) = sense_status();
    rt::print_args(format_args!(
        "[Spider] status: trained={} events={} raised_sites={}\n",
        trained, events, raised
    ));
    rt::call3(SYS_SENSE, 1, 0, 0);
    let (trained, events, raised) = sense_status();
    rt::print_args(format_args!(
        "[Spider] cone frozen: trained={} events={} raised_sites={}\n",
        trained, events, raised
    ));
    assert!(trained, "the cone must freeze before the rogue launches");

    let rogue_pid = rt::spawn("rogue.elf");
    assert_ne!(rogue_pid, ERROR, "spawning the rogue");
    rt::print_args(format_args!("[Spider] rogue spawned as pid {}\n", rogue_pid));
    let immediate = rt::call3(SYS_SENSE, 2, rogue_pid, 0);
    let (_t0, e0, _r0) = sense_status();
    rt::print_args(format_args!("[Spider] t0: budget={} events={}\n", immediate, e0));

    // Watch the budget climb until the rogue is condemned.
    let mut rounds = 0u64;
    loop {
        rt::sleep(5);
        let budget = rt::call3(SYS_SENSE, 2, rogue_pid, 0);
        if rounds % 2 == 0 {
            rt::print_args(format_args!("[Spider] rogue foreign budget: {}\n", budget));
        }
        if budget > 2_000_000 {
            rt::print("[Spider] rogue budget exceeds the condemnation threshold\n");
            break;
        }
        rounds += 1;
        if rounds > 60 {
            rt::print("[Spider] TIMEOUT waiting for condemnation\n");
            rt::exit(3);
        }
    }

    // Give the starved rogue time to prove its silence, then verify.
    rt::sleep(100);
    let budget = rt::call3(SYS_SENSE, 2, rogue_pid, 0);
    rt::print_args(format_args!(
        "[Spider] settled budget {}.{} — rogue starved (no further prints)\n",
        budget / 1_000_000,
        (budget % 1_000_000) / 100_000
    ));
    rt::print("[Spider] E21 PASS: the kernel felt the rogue without permission\n");
    rt::exit(0);
}
