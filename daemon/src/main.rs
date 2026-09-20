#![no_std]
#![no_main]
extern crate alloc;
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);
fn main() {
    rt::print("[Daemon] Started\n");
    let mut heartbeat = rt::call(SYS_TICKS, 0, 0);
    loop {
        let mut buffer = [0u8; rt::ipc::MAX_MESSAGE_BYTES];
        loop {
            match rt::receive_into(&mut buffer) {
                Ok(Some(len)) => {
                    if let Ok(message) = core::str::from_utf8(&buffer[..len]) {
                        rt::print_args(format_args!("[Daemon] Received IPC: {}\n", message));
                    }
                }
                Ok(None) => break,
                Err(_) => { rt::print("[Daemon] IPC receive error\n"); break; }
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
