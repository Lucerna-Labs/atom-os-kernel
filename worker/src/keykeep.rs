//! E22 keeper payload — holds the fail-dead key and reports its life.
//!
//! Initializes the key (becoming its keeper), maintains it in a loop,
//! and reads a key word every round. When the spider condemns the
//! rogue, the destruction signal fires: keepership is revoked and the
//! material zeroes. The keeper's next read prints the death — the
//! visible proof that detection killed the key.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::print("[Keeper] initializing the fail-dead key\n");
    if rt::call3(SYS_KEY, 0, 0x51EED_1, 0x51EED_2) != 1 {
        rt::print("[Keeper] init refused (key already lived once)\n");
        rt::exit(5);
    }
    let word0 = rt::call3(SYS_KEY, 2, 0, 0);
    rt::print_args(format_args!("[Keeper] key word 0 = {word0:#x} — alive and maintained\n"));
    let mut rounds = 0u64;
    loop {
        rt::sleep(20);
        let maintained = rt::call3(SYS_KEY, 1, 0, 0);
        let word = rt::call3(SYS_KEY, 2, 0, 0);
        let status = rt::call3(SYS_KEY, 3, 0, 0);
        let alive = status >> 63 == 1;
        let cause = (status >> 60) & 7;
        if !alive {
            rt::print_args(format_args!(
                "[Keeper] KEY DEAD (cause {cause}) — last word read {word:#x}, maintain={maintained}, round {rounds}\n"
            ));
            rt::print("[Keeper] the key is alive only while it is trusted — E22 PASS\n");
            rt::exit(0);
        }
        if rounds % 12 == 0 {
            rt::print_args(format_args!("[Keeper] round {rounds}: alive, word0={word:#x}\n"));
        }
        rounds += 1;
    }
}
