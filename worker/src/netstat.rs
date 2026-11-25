//! netstat — network stack status.

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
    rt::exit(0);
}
