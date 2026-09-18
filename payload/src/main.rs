#![no_std]
#![no_main]
extern crate alloc;
use alloc::{string::String, vec::Vec, format};
use user_rt::{self as rt, abi::*};
mod storage_probe;
user_rt::entry!(main);

fn line() -> String {
    let mut bytes = Vec::new();
    loop {
        let byte = rt::call(SYS_READ, 0, 0) as u8;
        match byte {
            0 => rt::sleep(1),
            b'\n' => { rt::print("\n"); return String::from_utf8(bytes).unwrap_or_default(); }
            8 => { if bytes.pop().is_some() { rt::call(SYS_WRITE, 8, 0); } }
            32..=126 => { if bytes.len() < 1024 { bytes.push(byte); rt::call(SYS_WRITE, byte as u64, 0); } }
            _ => {}
        }
    }
}
fn read_file(name: &str) -> Result<Vec<u8>, ()> {
    let fd = rt::open_existing(name); if fd == ERROR { return Err(()); }
    let mut bytes = Vec::new();
    while let Some(byte) = rt::read(fd) {
        if bytes.len() == 65536 { rt::close(fd); return Err(()); }
        bytes.push(byte);
    }
    rt::close(fd); Ok(bytes)
}
fn bench() {
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    for _ in 0..10000 { rt::yield_now(); }
    let cycles = unsafe { core::arch::x86_64::_rdtsc() } - start;
    rt::print(&format!("10,000 SYS_YIELDs took (CPU cycles): {}\n", cycles));
}
fn heap_test() {
    let mut values = Vec::new();
    for n in 0..4096u64 { values.push(n ^ 0x1234); }
    assert!(values.iter().enumerate().all(|(i, &n)| n == i as u64 ^ 0x1234));
    drop(values);
    #[repr(align(8192))]
    struct Aligned([u8; 8192]);
    let block = alloc::boxed::Box::new(Aligned([0x5a; 8192]));
    assert_eq!((&*block as *const Aligned as usize) & 8191, 0);
    assert!(block.0.iter().all(|&b| b == 0x5a));
    drop(block);
    rt::print("HEAP_OK\n");
}
fn worker() -> bool {
    let pid = rt::spawn("worker.elf");
    pid != ERROR && rt::wait(pid) == 37
}
fn churn(rounds: usize) {
    if !(1..=128).contains(&rounds) { rt::print("rounds must be 1..128\n"); return; }
    // Warm the daemon's receive-buffer mapping before comparing frame counts.
    if !worker() { rt::print("CHURN_FAIL warmup\n"); return; }
    rt::sleep(20);
    let before = rt::call(SYS_FREE_FRAMES, 0, 0);
    for round in 0..rounds {
        if !worker() { rt::print(&format!("CHURN_FAIL round={}\n", round)); return; }
    }
    rt::sleep(20);
    let after = rt::call(SYS_FREE_FRAMES, 0, 0);
    let tasks = rt::call(SYS_TASK_COUNT, 0, 0);
    rt::print(&format!("CHURN_{} rounds={} free_before={} free_after={} tasks={}\n",
        if before == after && tasks == 2 { "OK" } else { "FAIL" }, rounds, before, after, tasks));
}
fn edit(name: &str) {
    let mut bytes = match read_file(name) {
        Ok(bytes) => bytes,
        Err(()) if rt::fs_error() == FsError::NotFound as u64 => Vec::new(),
        Err(()) => { fs_error("edit"); return; }
    };
    rt::print("Editor: Esc saves; Backspace deletes.\n");
    if let Ok(text) = core::str::from_utf8(&bytes) { rt::print(text); }
    loop {
        let byte = rt::call(SYS_READ, 0, 0) as u8;
        if byte == 0 { rt::sleep(1); continue; }
        if byte == 27 { break; }
        if byte == 8 { if bytes.pop().is_some() { rt::call(SYS_WRITE, 8, 0); } }
        else if bytes.len() < 65536 { bytes.push(byte); rt::call(SYS_WRITE, byte as u64, 0); }
    }
    let fd = rt::open(name);
    let ok = fd != ERROR && rt::replace(fd, &bytes);
    let error = rt::fs_error(); rt::close(fd);
    if ok { rt::print("\nUpdated in RAM; use sync to save to disk.\n"); }
    else { rt::print_args(format_args!("\nSave rejected: {} (previous contents preserved)\n", fs_error_message(error))); }
}
fn fs_error(operation: &str) {
    let error = rt::fs_error();
    rt::print_args(format_args!("{}: {}\n", operation, fs_error_message(error)));
}
fn fs_status() {
    rt::print_args(format_args!("FS_STATUS {} files={}/{} bytes={}/{} buffers={}/{} generation={} disk={}\n",
        if rt::fs_stat(4) == 0 { "saved" } else { "unsaved" },
        rt::fs_stat(0), rt::fs_stat(1), rt::fs_stat(2), rt::fs_stat(3),
        rt::fs_stat(6), rt::fs_stat(7), rt::fs_stat(5), rt::fs_stat(10)));
}
fn execute(command: &str) {
    let (verb, argument) = command.split_once(' ').unwrap_or((command, ""));
    let argument = argument.trim();
    match verb {
        "" => {}
        "help" => rt::print("commands: help ls clear cat edit echo msg bench heaptest stats spawn wait run ps kill proctest selftest pairtest churn faulttest fstest storageprobe rm mv df status sync reboot\n"),
        "ls" => { rt::call(SYS_LIST_DIR, 0, 0); }
        "clear" => { rt::call(SYS_CLEAR, 0, 0); }
        "bench" => bench(),
        "heaptest" => heap_test(),
        "cat" => match read_file(argument) {
            Ok(bytes) => { if let Ok(text) = core::str::from_utf8(&bytes) { rt::print(text); } else { rt::print("binary file"); } rt::print("\n"); }
            Err(()) => fs_error("cat"),
        },
        "edit" => edit(argument),
        "echo" => if let Some((text, name)) = argument.split_once(" > ") {
            let fd = rt::open(name.trim());
            let line = format!("{}\n", text);
            if fd == ERROR || !rt::write(fd, line.as_bytes()) { fs_error("echo"); }
            rt::close(fd);
        } else { rt::print("usage: echo text > file\n"); },
        "rm" => { if rt::remove(argument) { rt::print("REMOVED\n"); } else { fs_error("rm"); } }
        "mv" => {
            let words: Vec<_> = argument.split_whitespace().collect();
            if words.len() != 2 { rt::print("usage: mv old-name new-name\n"); }
            else if rt::rename(words[0], words[1]) { rt::print("RENAMED\n"); } else { fs_error("mv"); }
        }
        "df" | "status" => fs_status(),
        "fstest" => {
            let pid = rt::spawn("fs-probe.elf");
            if pid != ERROR && rt::wait(pid) == 0 { rt::print("FSTEST_OK\n"); } else { rt::print("FSTEST_FAIL\n"); }
        }
        "storageprobe" => storage_probe::run(argument),
        "msg" => rt::print(if rt::send(2, argument) { "Message sent to Daemon\n" } else { "message rejected or mailbox full\n" }),
        "spawn" | "run" => match rt::arguments::words(argument) {
            Ok(words) => {
                let args: Vec<_> = words[1..].iter().map(String::as_str).collect();
                if verb == "spawn" {
                    let pid = rt::spawn_args(&words[0], &args);
                    if pid == ERROR { rt::print("spawn failed\n"); }
                    else { rt::print_args(format_args!("spawned pid {}\n", pid)); }
                } else { rt::exec_args(&words[0], &args); rt::print("exec failed\n"); }
            }
            Err(()) => rt::print("invalid program arguments or quoting\n"),
        },
        "ps" => match rt::processes() {
            Ok(tasks) => {
                rt::print("PID PPID STATE PROGRAM\n");
                for task in tasks {
                    let end = task.name.iter().position(|&b| b == 0).unwrap_or(task.name.len());
                    let name = core::str::from_utf8(&task.name[..end]).unwrap_or("?");
                    let state = match task.state {
                        PROCESS_READY => "ready", PROCESS_RUNNING => "running",
                        PROCESS_SLEEPING => "sleeping", PROCESS_WAITING => "waiting",
                        PROCESS_EXITED => "exited", _ => "trapped",
                    };
                    rt::print_args(format_args!("{} {} {} {}\n", task.pid, task.parent, state, name));
                }
            }
            Err(()) => rt::print("ps failed\n"),
        },
        "kill" => match argument.parse::<u64>() {
            Ok(pid) => if rt::kill(pid) { rt::print_args(format_args!("killed pid {}\n", pid)); }
                       else { rt::print("kill failed: no live process with that PID\n"); },
            Err(_) => rt::print("usage: kill pid\n"),
        },
        "proctest" => {
            let pid = rt::spawn_args("worker.elf", &["--process-test"]);
            if pid != ERROR && rt::wait(pid) == 0 { rt::print("PROCTEST_OK\n"); }
            else { rt::print("PROCTEST_FAIL\n"); }
        }
        "wait" => match argument.parse::<u64>() {
            Ok(pid) => { let status = rt::wait(pid); if status == ERROR { rt::print("wait failed\n"); }
                else { rt::print(&format!("wait pid={} status={}\n", pid, status)); } }
            Err(_) => rt::print("usage: wait pid\n"),
        },
        "selftest" => rt::print(if worker() { "SELFTEST_OK\n" } else { "SELFTEST_FAIL\n" }),
        "pairtest" => {
            let first = rt::spawn("worker.elf"); let second = rt::spawn("worker.elf");
            let a = if first != ERROR { rt::wait(first) } else { ERROR };
            let b = if second != ERROR { rt::wait(second) } else { ERROR };
            rt::print(if a == 37 && b == 37 { "PAIRTEST_OK\n" } else { "PAIRTEST_FAIL\n" });
        }
        "churn" => churn(if argument.is_empty() { 48 } else { argument.parse().unwrap_or(0) }),
        "faulttest" => {
            let pid = rt::spawn("fault.elf");
            let status = if pid != ERROR { rt::wait(pid) } else { ERROR };
            rt::print(&format!("FAULT_ISOLATION_{} status={}\n", if status == 142 { "OK" } else { "FAIL" }, status));
        }
        "stats" => { let free = rt::call(SYS_FREE_FRAMES, 0, 0); let tasks = rt::call(SYS_TASK_COUNT, 0, 0);
            rt::print(&format!("FREE_FRAMES {} TASKS {}\n", free, tasks)); }
        "sync" => {
            if rt::call(SYS_SYNC, 0, 0) == 0 { rt::print("SYNC_OK\n"); fs_status(); }
            else { rt::print("SYNC_FAILED\n"); fs_error("sync"); }
        },
        "reboot" => { rt::call(SYS_REBOOT, 0, 0); rt::print("reboot failed (sync required)\n"); }
        _ => rt::print("Unknown command\n"),
    }
}
fn main() {
    rt::print("ATOM OS kernel shell\n");
    heap_test(); bench();
    // E21: launch the shadow-web spider probe after the clean boot.
    let spider = rt::spawn("spider.elf");
    if spider == ERROR { rt::print("shell: spider.elf not found\n"); }
    let weave = rt::spawn("weave.elf");
    if weave == ERROR { rt::print("shell: weave.elf not found\n"); }
    loop { rt::print("> "); let command = line(); execute(command.trim()); }
}
