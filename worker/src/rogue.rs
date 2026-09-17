//! E21 rogue payload — the intruder (runs only inside the VM).
//!
//! Hammers IPC sends to a partner pid that no clean boot ever used.
//! Every attempt is a vibration on the shadow web — even though the
//! sends fail (the target does not exist), the sensor at the syscall
//! chokepoint has already felt the attempt. Its heartbeat prints
//! make starvation visible: when the spider's cone condemns it, the
//! scheduler stops scheduling it and the heartbeats stop.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::print("[Rogue] infiltrated (inside the VM)\n");
    // A real message buffer: the sends fail only because pid 99 does
    // not exist — the vibration at the chokepoint happens regardless.
    let message = b"knock knock\0";
    let mut attempts = 0u64;
    loop {
        // Scan across partners, the way real lateral movement does:
        // no single-luck conversation site can hide a scanner.
        let target = 90 + attempts % 32;
        let _ = rt::call3(SYS_IPC_SEND, target, message.as_ptr() as u64, 0);
        attempts += 1;
        if attempts % 200 == 0 {
            rt::print_args(format_args!("[Rogue] {} attempts\n", attempts));
        }
    }
}
