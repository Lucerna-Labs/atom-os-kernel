//! ping — ICMP echo probe over the kernel VirtIO network stack.
//!
//! Bootstraps ARP, sends four echo requests, reads replies out of the
//! packet ring, and prints the counts.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn net(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_NET, sub, a, b)
}

fn main() {
    rt::sleep(10);
    if net(0, 0, 0) >> 63 != 1 {
        rt::print("[ping] no NIC on this machine — nothing to ask\n");
        rt::exit(0);
    }
    rt::print("[ping] bootstrap: asking the wire for the gateway\n");
    let _ = net(4, 0, 0);
    rt::sleep(6);
    let prose = b"ping from the atom shell";
    let mut replies = 0u64;
    for i in 0..4u64 {
        let cause = net(2, prose.as_ptr() as u64, prose.len() as u64);
        if cause != 1 {
            let why = match cause { 3 => "no route yet", _ => "driver" };
            rt::print_args(format_args!("[ping] send {i} refused ({why})\n"));
            let _ = net(4, 0, 0); // re-bootstrap
            rt::sleep(6);
            continue;
        }
        rt::sleep(8);
        net(3, 0, 0); // heartbeat now
        let got = net(1, 0, 0);
        if got != ERROR && got > 0 {
            replies += 1;
            rt::print_args(format_args!("[ping] reply {}: {} bytes back from the wire\n", i + 1, got));
        } else {
            rt::print_args(format_args!("[ping] send {i}: no reply this round\n"));
        }
    }
    let status = net(0, 0, 0);
    rt::print_args(format_args!(
        "[ping] done: {replies}/4 replies, arp={} icmp={} rx={}\n",
        (status >> 32) & 0xFFFF, (status >> 16) & 0xFFFF, (status >> 48) & 0xFF
    ));
    rt::exit(0);
}
