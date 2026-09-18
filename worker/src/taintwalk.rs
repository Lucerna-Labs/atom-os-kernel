//! E34 taintwalk payload — the zero-click wall demo.
//!
//! Phase 1: this payload marks ITSELF tainted (simulating the network
//! ingress seam — in a real system, opening a network resource would
//! do this automatically). Phase 2: attempt to spawn a child — the
//! exec/spawn gate must REFUSE (derived content stays data). Phase 3:
//! self-register as input-focus (simulating the keyboard handler —
//! in a real system, only the keyboard context can do this), promote
//! the taint, then spawn successfully.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn taint(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_TAINT, sub, a, b)
}

fn main() {
    rt::sleep(30);
    let me = rt::call(SYS_GETPID, 0, 0);

    // Phase 1: mark self tainted (the network-ingress simulation).
    if taint(0, me, 0) != 1 {
        rt::print("[Taint] FAIL: could not mark self\n");
        rt::exit(5);
    }
    let state = taint(3, me, 0);
    rt::print_args(format_args!("[Taint] self marked tainted (state={state})\n"));

    // Phase 2: attempt to spawn — must be REFUSED by the gate.
    let spawn_result = rt::spawn("shell.elf");
    if spawn_result == ERROR {
        rt::print("[Taint] spawn REFUSED — derived content stays data\n");
    } else {
        rt::print("[Taint] FAIL: tainted spawn was allowed!\n");
        rt::exit(6);
    }

    // Phase 3: become input-focus (the keyboard-handler simulation),
    // promote the taint, then spawn succeeds.
    taint(6, 0, 0); // set_input_focus(self)
    let promoted = taint(2, me, 0); // promote(target=self)
    if promoted == 1 {
        rt::print("[Taint] promoted via input-focus event — the human said yes\n");
    } else {
        rt::print("[Taint] FAIL: promotion refused\n");
        rt::exit(7);
    }
    let spawn_after = rt::spawn("shell.elf");
    if spawn_after != ERROR {
        rt::print("[Taint] spawn after promotion: allowed\n");
        rt::print("[Taint] E34 PASS: tainted→refused→promoted→allowed\n");
        rt::exit(0);
    } else {
        rt::print("[Taint] FAIL: promoted spawn still refused\n");
        rt::exit(8);
    }
}
