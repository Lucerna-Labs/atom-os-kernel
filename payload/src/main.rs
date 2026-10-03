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
fn read_file(path: &str) -> Result<Vec<u8>, ()> { rt::read_file(path).ok_or(()) }

fn size_text(bytes: u64) -> String {
    if bytes < 1024 { format!("{} B", bytes) }
    else if bytes < 1 << 20 { format!("{}.{} KB", bytes >> 10, (bytes & 1023) * 10 >> 10) }
    else if bytes < 1 << 30 { format!("{}.{} MB", bytes >> 20, (bytes & 0xfffff) * 10 >> 20) }
    else { format!("{}.{} GB", bytes >> 30, (bytes & 0x3fff_ffff) * 10 >> 30) }
}

/// The checksum `fill` and `sum` print (FNV-1a, 64-bit).
fn checksum(hash: u64, bytes: &[u8]) -> u64 { bytes.iter().fold(hash, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3)) }

/// Writes `size` bytes of a fixed pattern to `path` in 64 KiB pieces (for testing large files).
fn fill(path: &str, size: u64) {
    let fd = rt::open(path);
    if fd == ERROR || rt::call(SYS_TRUNCATE, fd, 0) != 0 { rt::print("fill: cannot open\n"); if fd != ERROR { rt::close(fd); } return; }
    let mut chunk = alloc::vec![0u8; 65536];
    let (mut done, mut hash) = (0u64, 0xcbf29ce484222325u64);
    while done < size {
        let n = (size - done).min(chunk.len() as u64) as usize;
        for (i, b) in chunk[..n].iter_mut().enumerate() { *b = ((done + i as u64) * 31 % 251) as u8; }
        if let Err(e) = rt::file_write_all(fd, &chunk[..n]) { rt::print(&format!("fill: {}\n", e.message())); rt::close(fd); return; }
        hash = checksum(hash, &chunk[..n]);
        done += n as u64;
    }
    rt::close(fd);
    rt::print(&format!("FILL_OK bytes={} sum={:016x}\n", done, hash));
}

fn sum(path: &str) {
    if rt::stat(path).map_or(true, |e| e.dir) { rt::print("sum: no such file\n"); return; }
    let fd = rt::open(path);
    let mut chunk = alloc::vec![0u8; 65536];
    let (mut done, mut hash) = (0u64, 0xcbf29ce484222325u64);
    loop {
        match rt::file_read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => { hash = checksum(hash, &chunk[..n]); done += n as u64; }
            Err(e) => { rt::print(&format!("sum: {}\n", e.message())); break; }
        }
    }
    rt::close(fd);
    rt::print(&format!("SUM bytes={} sum={:016x}\n", done, hash));
}

fn copy(from: &str, to: &str) -> Result<u64, rt::FsError> {
    let source = rt::stat(from)?;
    if source.dir { return Err(rt::FsError::IsDir); }
    if from == to { return Err(rt::FsError::Exists); }
    let (input, output) = (rt::open(from), rt::open(to));
    let result = (|| {
        if input == ERROR || output == ERROR { return Err(rt::FsError::Other); }
        if rt::call(SYS_TRUNCATE, output, 0) != 0 { return Err(rt::FsError::ReadOnly); }
        let mut chunk = alloc::vec![0u8; 65536];
        let mut done = 0;
        loop {
            let n = rt::file_read(input, &mut chunk)?;
            if n == 0 { return Ok(done); }
            rt::file_write_all(output, &chunk[..n])?;
            done += n as u64;
        }
    })();
    if input != ERROR { rt::close(input); }
    if output != ERROR { rt::close(output); }
    result
}

/// `to` as a destination for `from`: an existing folder means "into that folder".
fn destination(from: &str, to: &str) -> String {
    if rt::stat(to).is_ok_and(|e| e.dir) { rt::path::join(to, rt::path::name(from)) } else { String::from(to) }
}

fn list(path: &str) {
    match rt::read_dir(path) {
        Ok(entries) => {
            for e in entries {
                if e.dir { rt::print(&format!("{}/\n", e.name)); }
                else { rt::print(&format!("{:<32} {}{}\n", e.name, size_text(e.size), if e.unsaved { "  (unsaved)" } else { "" })); }
            }
        }
        Err(e) => rt::print(&format!("ls: {}\n", e.message())),
    }
}

fn disk_report() {
    let info = rt::fs_info();
    if info.disk == 0 { rt::print("No data disk: files are kept in memory only.\n"); }
    else {
        rt::print(&format!("Disk {}  saved {}  files need {} of {}\n", size_text(info.disk_bytes), size_text(info.saved_bytes),
            size_text(info.needed_bytes), size_text(info.capacity_bytes)));
    }
    rt::print(if info.unsaved != 0 { "Unsaved changes: run sync to save them.\n" } else { "Everything is saved.\n" });
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
    if rt::stat(name).is_ok_and(|e| e.dir) { rt::print("cannot edit a folder\n"); return; }
    let mut bytes = read_file(name).unwrap_or_default();
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
    rt::print(if ok { "\nSaved; use sync to save to disk.\n" } else { "\nSave failed\n" });
}
fn execute(command: &str, cwd: &mut String) {
    let (verb, argument) = command.split_once(' ').unwrap_or((command, ""));
    let argument = argument.trim();
    let here = |p: &str| rt::path::join(cwd, p);
    let two = |a: &str| a.split_once(' ').map(|(x, y)| (rt::path::join(cwd, x.trim()), rt::path::join(cwd, y.trim())));
    match verb {
        "" => {}
        "help" => rt::print("commands: help ls cd pwd mkdir rmdir cat edit echo cp rm mv df fill sum clear msg desktop bench heaptest stats spawn wait ps kill run selftest pairtest churn faulttest sync reboot\n"),
        "ls" => list(&here(argument)),
        "pwd" => rt::print(&format!("{}\n", cwd)),
        "cd" => {
            let target = if argument.is_empty() { String::from("/") } else { here(argument) };
            match rt::stat(&target) {
                Ok(e) if e.dir => *cwd = target,
                Ok(_) => rt::print("cd: not a folder\n"),
                Err(e) => rt::print(&format!("cd: {}\n", e.message())),
            }
        }
        "mkdir" => match rt::mkdir(&here(argument)) {
            Ok(()) => rt::print("folder created (sync to save)\n"),
            Err(e) => rt::print(&format!("mkdir failed: {}\n", e.message())),
        },
        "rmdir" => match rt::stat(&here(argument)) {
            Ok(e) if !e.dir => rt::print("rmdir: not a folder\n"),
            _ => match rt::remove_path(&here(argument)) {
                Ok(()) => rt::print("removed (sync to save)\n"),
                Err(e) => rt::print(&format!("rmdir failed: {}\n", e.message())),
            },
        },
        "cp" => match two(argument) {
            Some((from, to)) => match copy(&from, &destination(&from, &to)) {
                Ok(n) => rt::print(&format!("copied {} (sync to save)\n", size_text(n))),
                Err(e) => rt::print(&format!("cp failed: {}\n", e.message())),
            },
            None => rt::print("usage: cp from to\n"),
        },
        "df" => disk_report(),
        "fill" => match argument.split_once(' ').map(|(p, n)| (here(p.trim()), n.trim().parse::<u64>())) {
            Some((path, Ok(size))) => fill(&path, size),
            _ => rt::print("usage: fill file bytes\n"),
        },
        "sum" => sum(&here(argument)),
        "clear" => { rt::call(SYS_CLEAR, 0, 0); }
        "bench" => bench(),
        "heaptest" => heap_test(),
        "cat" => match read_file(&here(argument)) {
            Ok(bytes) => { if let Ok(text) = core::str::from_utf8(&bytes) { rt::print(text); } else { rt::print("binary file"); } rt::print("\n"); }
            Err(()) => rt::print("cannot read file\n"),
        },
        "edit" => edit(&here(argument)),
        "echo" => if let Some((text, name)) = argument.split_once(" > ") {
            let fd = rt::open(&here(name.trim()));
            if fd == ERROR || !rt::write(fd, text.as_bytes()) || !rt::write(fd, b"\n") { rt::print("write failed\n"); }
            rt::close(fd);
        } else { rt::print("usage: echo text > file\n"); },
        "rm" => match rt::remove_path(&here(argument)) {
            Ok(()) => rt::print("removed (sync to save)\n"),
            Err(e) => rt::print(&format!("rm failed: {}\n", e.message())),
        },
        "mv" => match two(argument) {
            Some((from, to)) => match rt::move_path(&from, &destination(&from, &to)) {
                Ok(()) => rt::print("renamed (sync to save)\n"),
                Err(e) => rt::print(&format!("mv failed: {}\n", e.message())),
            },
            None => rt::print("usage: mv old new\n"),
        },
        "ps" => {
            rt::print("PID PARENT STATE    NAME\n");
            for process in rt::processes() {
                rt::print(&format!("{:<3} {:<6} {:<8} {}\n", process.pid, process.parent, process.state_name(), process.name()));
            }
        }
        "kill" => match argument.parse::<u64>() {
            Ok(pid) => rt::print(&if rt::kill(pid) { format!("killed pid {}\n", pid) } else { String::from("kill failed\n") }),
            Err(_) => rt::print("usage: kill pid\n"),
        },
        "msg" => rt::print(if rt::send(2, argument) { "Message sent to Daemon\n" } else { "message rejected or mailbox full\n" }),
        "spawn" => { let pid = rt::spawn(&program(cwd, argument)); if pid == ERROR { rt::print("spawn failed\n"); }
            else { rt::print(&format!("spawned pid {}\n", pid)); } }
        "wait" => match argument.parse::<u64>() {
            Ok(pid) => { let status = rt::wait(pid); if status == ERROR { rt::print("wait failed\n"); }
                else { rt::print(&format!("wait pid={} status={}\n", pid, status)); } }
            Err(_) => rt::print("usage: wait pid\n"),
        },
        "run" => { rt::path_call(SYS_EXEC, &program(cwd, argument)); rt::print("exec failed\n"); }
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
        "desktop" => run_desktop(),
        "sync" => match rt::sync_status() {
            Ok(()) => rt::print("SYNC_OK\n"),
            Err(e) => rt::print(&format!("SYNC_FAILED: {}\n", e.message())),
        },
        "reboot" => { rt::call(SYS_REBOOT, 0, 0); rt::print("reboot failed (sync required)\n"); }
        _ => rt::print("Unknown command\n"),
    }
}
/// A program name as typed: a bare name is looked up by the kernel (in /bin, then /);
/// anything with a `/` is a path relative to the current folder.
fn program(cwd: &str, name: &str) -> String {
    if name.contains('/') { rt::path::join(cwd, name) } else { String::from(name) }
}

fn run_desktop() {
    if rt::call(SYS_DISPLAY_PRESENT, 0, 0) != 1 { rt::print("desktop: no display device\n"); return; }
    let pid = rt::spawn("desktop.elf");
    if pid == ERROR { rt::print("desktop: could not start\n"); return; }
    let status = rt::wait(pid);
    rt::call(SYS_CLEAR, 0, 0);
    rt::print(&format!("Desktop closed (status {}). Type 'desktop' to return.\n", status));
}
fn main() {
    rt::print("ATOM OS kernel shell\n");
    heap_test(); bench();
    // Boot into the desktop when this is the console shell on a machine with
    // a display. A shell inside a desktop terminal has a piped stdin and
    // never gets here with a free display.
    if rt::args().is_empty() && rt::call(SYS_GETPID, 0, 0) == 1 && rt::call(SYS_DISPLAY_PRESENT, 0, 0) == 1 {
        run_desktop();
    }
    let mut cwd = String::from("/");
    loop { rt::print("> "); let command = line(); execute(command.trim(), &mut cwd); }
}
