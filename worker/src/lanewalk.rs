//! E25 lanewalk payload — walks a real transit, then condemns
//! itself and proves the lane serves honey to the untrusted.
//!
//! The arc: claim the lane (ceremony), deposit word A, handoff A
//! (REAL transit, kernel prints the receipt), deposit word B, then
//! commit 200 scans of an unused partner — the spider condemns this
//! task, the scheduler trips the lane's revoke latch. After the dial
//! breathes and we're scheduled again: handoff returns HONEY, not B.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::sleep(50); // let the boot settle; the cone is armed
    if rt::call3(SYS_KEYLANE, 0, 0, 0) != 1 {
        rt::print("[Lane] claim refused — lane already walked this boot\n");
        rt::exit(5);
    }
    rt::print_args(format_args!("[Lane] lane claimed by pid {}\n", rt::call(SYS_GETPID, 0, 0)));

    // Legal transit: deposit A, hand it off.
    if rt::call3(SYS_KEYLANE, 1, 0x51FE_F00D, 1) != 1 {
        rt::print("[Lane] deposit A refused\n");
        rt::exit(5);
    }
    let out = rt::call3(SYS_KEYLANE, 2, 0, 0);
    if out != 0x51FE_F00D {
        rt::print_args(format_args!("[Lane] FAIL: expected A, got {out:#x}\n"));
        rt::exit(6);
    }
    rt::print("[Lane] legal transit of A complete (receipt on serial)\n");

    // Deposit B — this word should NEVER leave as real material.
    if rt::call3(SYS_KEYLANE, 1, 0xB10E_0DD5, 2) != 1 {
        rt::print("[Lane] deposit B refused\n");
        rt::exit(5);
    }
    rt::print("[Lane] B holding — now committing 300 scans to draw the spider\n");
    // A SPREAD of targets: a single partner can luck into the normal
    // map (the original rogue's bug); 32 distinct sites cannot hide.
    let message = b"knock\0";
    for i in 0..300u64 {
        let _ = rt::call3(SYS_IPC_SEND, 90 + i % 32, message.as_ptr() as u64, 0);
    }
    // Stay READY so a timer tick polls the sensor while our budget
    // is above threshold — the burst alone can complete between two
    // ticks, and a sleeping task is never polled (the rogue stayed
    // condemned only because it never sleeps). Watch until the
    // budget is observed condemned, then let the dial breathe.
    let my = rt::call(SYS_GETPID, 0, 0);
    let mut condemned = false;
    for round in 0..4000u64 {
        rt::call(SYS_YIELD, 0, 0);
        if round % 25 == 0 {
            let budget = rt::call3(SYS_SENSE, 2, my, 0);
            if budget > 2_000_000 { condemned = true; break; }
        }
    }
    if !condemned {
        rt::print("[Lane] FAIL: the spider never observed my condemnation\n");
        rt::exit(9);
    }
    rt::print("[Lane] the spider has condemned me — the lane's trust dies with it\n");
    // Wait out one breath of the dial so we are scheduled again; the
    // revoke latch is permanent regardless of the budget eroding.
    rt::sleep(400);
    rt::print("[Lane] back from condemnation — attempting handoff as the untrusted\n");
    let served = rt::call3(SYS_KEYLANE, 2, 0, 0);
    if served == 0xB10E_0DD5 || served == ERROR {
        rt::print_args(format_args!("[Lane] FAIL: got {served:#x} — B leaked or empty\n"));
        rt::exit(7);
    }
    rt::print_args(format_args!(
        "[Lane] served {served:#x} = HONEY (B never left) — the lane knew\n"
    ));
    // Audit: log slot 1 is the honey record; status agrees.
    let logged_word = rt::call3(SYS_KEYLANE, 3, 1, 0);
    let logged_kind = rt::call3(SYS_KEYLANE, 5, 1, 0);
    let (_, _, revoked, real, honey) = (0u64, false, false, 0u64, 0u64);
    let status = rt::call3(SYS_KEYLANE, 4, 0, 0);
    let (revoked, real, honey) = (
        (status >> 46) & 1,
        (status >> 23) & 0x7F_FFFF,
        status & 0x7F_FFFF,
    );
    let _ = (revoked, real, honey, revoked);
    rt::print_args(format_args!(
        "[Lane] log[1] kind {} word {:#x}; status real={} honey={} revoked={}\n",
        logged_kind,
        logged_word,
        (status >> 23) & 0x7F_FFFF,
        status & 0x7F_FFFF,
        (status >> 46) & 1
    ));
    rt::print("[Lane] E25 PASS: one way out, watched forever, honey for the untrusted\n");
    rt::exit(0);
}
