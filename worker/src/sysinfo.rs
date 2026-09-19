//! sysinfo — memory, tasks, uptime.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    rt::print_args(format_args!("free frames: {}\n", rt::call(SYS_FREE_FRAMES, 0, 0)));
    rt::print_args(format_args!("tasks: {}\n", rt::call(SYS_TASK_COUNT, 0, 0)));
    let ticks = rt::call(SYS_TICKS, 0, 0);
    rt::print_args(format_args!("ticks: {ticks}\n"));
    rt::print_args(format_args!("uptime: {} seconds (at 100 ticks/s)\n", ticks / 100));
    rt::exit(0);
}
