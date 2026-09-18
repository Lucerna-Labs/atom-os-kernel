//! E23 instant-key keeper — proves stability without persistence.
//!
//! Initializes the share-set, transforms a fixed word twice at a
//! distance (same output = same key BEHAVIOR, though the key thing
//! exists only for single instructions between), maintains, and
//! reports death when the spider's signal kills the species.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::print("[Instant] initializing the instant-key (four shares, one behavior)\n");
    if rt::call3(SYS_INSTANT, 0, 0x0E23, 0) != 1 {
        rt::print("[Instant] init refused\n");
        rt::exit(5);
    }
    let first = rt::call3(SYS_INSTANT, 2, 0xFEED_FACE, 0);
    rt::print_args(format_args!("[Instant] transform(0xFEED_FACE) = {first:#x}\n"));
    let mut rounds = 0u64;
    loop {
        rt::sleep(20);
        rt::call3(SYS_INSTANT, 1, 0, 0);
        let again = rt::call3(SYS_INSTANT, 2, 0xFEED_FACE, 0);
        let status = rt::call3(SYS_INSTANT, 3, 0, 0);
        let alive = status >> 63 == 1;
        let cause = (status >> 60) & 7;
        if !alive || again == ERROR {
            rt::print_args(format_args!(
                "[Instant] KEY UNBORN (cause {cause}) — transform refused at round {rounds}\n"
            ));
            rt::print("[Instant] same behavior while alive, nothing between calls — E23 PASS\n");
            rt::exit(0);
        }
        if again != first {
            rt::print_args(format_args!(
                "[Instant] FAIL: transform unstable ({again:#x} != {first:#x})\n"
            ));
            rt::exit(6);
        }
        if rounds % 12 == 0 {
            rt::print_args(format_args!("[Instant] round {rounds}: stable at {again:#x}\n"));
        }
        rounds += 1;
    }
}
