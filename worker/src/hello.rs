//! hello — the first citizen of the atom userspace.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::print("hello from the atom userspace\n");
    rt::print_args(format_args!("pid {}\n", rt::call(SYS_GETPID, 0, 0)));
    let argv = rt::args();
    if argv.is_empty() {
        rt::print("no argv\n");
    } else {
        for arg in &argv {
            rt::print_args(format_args!("arg: {arg}\n"));
        }
    }
    rt::exit(0);
}
