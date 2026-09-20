//! netstat — the wire's receipts.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    let status = rt::call3(SYS_NET, 0, 0, 0);
    if status >> 63 == 1 {
        rt::print("link: up\n");
    } else {
        rt::print("link: down\n");
    }
    rt::print_args(format_args!("ring: {}\n", (status >> 48) & 0xFF));
    rt::print_args(format_args!("arp replied: {}\n", (status >> 32) & 0xFFFF));
    rt::print_args(format_args!("icmp replied: {}\n", (status >> 16) & 0xFFFF));
    rt::print_args(format_args!("cone blocked: {}\n", status & 0xFFFF));
    let sense = rt::call3(SYS_SENSE, 0, 0, 0);
    rt::print_args(format_args!(
        "spider: trained={} events={}\n",
        sense >> 63,
        (sense >> 32) & 0x7FFF_FFFF
    ));
    rt::exit(0);
}
