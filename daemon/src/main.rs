#![no_std]
#![no_main]
extern crate alloc;
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);
fn main() {
    rt::print("[Daemon] Started\n");
    let mut heartbeat = rt::call(SYS_TICKS, 0, 0);
    loop {
        loop {
            let pointer = rt::call(SYS_IPC_RECV, 0, 0);
            if pointer == 0 || pointer == ERROR { break; }
            let bytes = unsafe { core::slice::from_raw_parts(pointer as *const u8, 256) };
            let len = bytes.iter().position(|&b| b == 0).unwrap_or(255);
            if let Ok(message) = core::str::from_utf8(&bytes[..len]) {
                rt::print_args(format_args!("[Daemon] Received IPC: {}\n", message));
            }
        }
        let ticks = rt::call(SYS_TICKS, 0, 0);
        if ticks.saturating_sub(heartbeat) >= 100 {
            let tasks = rt::call(SYS_TASK_COUNT, 0, 0);
            // Keep the interactive prompt quiet. When the shell exits, the
            // heartbeat still proves the daemon survives as the remaining task.
            if tasks == 1 { rt::print_args(format_args!("[Daemon] Heartbeat... tasks={}\n", tasks)); }
            heartbeat = ticks;
        }
        rt::sleep(5);
    }
}
