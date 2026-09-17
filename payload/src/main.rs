#![no_std]
#![no_main]
extern crate alloc;
use alloc::{string::String, vec::Vec, format};
use user_rt::{self as rt, abi::*};
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
    let fd = rt::open(name); if fd == ERROR { return Err(()); }
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
    let Ok(mut bytes) = read_file(name) else { rt::print("cannot open editor\n"); return; };
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
    let ok = fd != ERROR && rt::call(SYS_TRUNCATE, fd, 0) == 0 && rt::write(fd, &bytes);
    rt::close(fd);
    rt::print(if ok { "\nSaved in RAM; use sync to save to disk.\n" } else { "\nSave failed\n" });
}
fn execute(command: &str) {
    let (verb, argument) = command.split_once(' ').unwrap_or((command, ""));
    let argument = argument.trim();
    match verb {
        "" => {}
        "help" => rt::print("commands: help ls clear cat edit echo msg bench heaptest stats spawn wait run selftest pairtest churn faulttest sync reboot\n"),
        "ls" => { rt::call(SYS_LIST_DIR, 0, 0); }
        "clear" => { rt::call(SYS_CLEAR, 0, 0); }
        "bench" => bench(),
        "heaptest" => heap_test(),
        "cat" => match read_file(argument) {
            Ok(bytes) => { if let Ok(text) = core::str::from_utf8(&bytes) { rt::print(text); } else { rt::print("binary file"); } rt::print("\n"); }
            Err(()) => rt::print("cannot read file\n"),
        },
        "edit" => edit(argument),
        "echo" => if let Some((text, name)) = argument.split_once(" > ") {
            let fd = rt::open(name.trim());
            if fd == ERROR || !rt::write(fd, text.as_bytes()) || !rt::write(fd, b"\n") { rt::print("write failed\n"); }
            rt::close(fd);
        } else { rt::print("usage: echo text > file\n"); },
        "msg" => rt::print(if rt::send(2, argument) { "Message sent to Daemon\n" } else { "message rejected or mailbox full\n" }),
        "spawn" => { let pid = rt::spawn(argument); if pid == ERROR { rt::print("spawn failed\n"); }
            else { rt::print(&format!("spawned pid {}\n", pid)); } }
        "wait" => match argument.parse::<u64>() {
            Ok(pid) => { let status = rt::wait(pid); if status == ERROR { rt::print("wait failed\n"); }
                else { rt::print(&format!("wait pid={} status={}\n", pid, status)); } }
            Err(_) => rt::print("usage: wait pid\n"),
        },
        "run" => { rt::path_call(SYS_EXEC, argument); rt::print("exec failed\n"); }
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
        "sync" => rt::print(if rt::call(SYS_SYNC, 0, 0) == 0 { "SYNC_OK\n" } else { "SYNC_FAILED\n" }),
        "reboot" => { rt::call(SYS_REBOOT, 0, 0); rt::print("reboot failed (sync required)\n"); }
        _ => rt::print("Unknown command\n"),
    }
}
fn main() {
    rt::print("ATOM OS kernel shell\n");
    heap_test(); bench();
    loop { rt::print("> "); let command = line(); execute(command.trim()); }
}
