#![no_std]
#![no_main]
extern crate alloc;
use alloc::{string::String, vec::Vec, format};
use user_rt::{self as rt, abi::*};
mod storage_probe;
user_rt::entry!(main);

/// E40: the line editor. The buffer holds the line, `pos` is the
/// insertion index; arrows (PS/2 control codes 0x12/0x13, or ANSI
/// ESC [ D / ESC [ C over serial) move within it; printable keys
/// insert at the cursor; backspace deletes before it. Every edit
/// redraws the tail from the cursor and parks the VGA hardware
/// cursor at the insertion point — you can SEE where you type.
fn put(byte: u8) {
    let _ = rt::call(SYS_WRITE, byte as u64, 0);
}

/// E40: the prompt shows where you are — the tree is visible.
fn prompt() {
    rt::print_args(format_args!("{}> ", rt::cwd()));
}

fn redraw_tail(bytes: &[u8], from: usize, park_at: usize) {
    for &b in &bytes[from..] {
        put(b);
    }
    put(b' '); // erase one char of any older, longer tail
    // Cursor now sits at (len+1) cells past `from`; park it.
    let back = (bytes.len() + 1 - from).saturating_sub(park_at - from) as i64;
    rt::vga_move(-back);
}

fn line() -> String {
    let mut bytes: Vec<u8> = Vec::new();
    let mut pos: usize = 0;
    let mut esc = 0u8; // ANSI state for the serial console: 0 none, 1 ESC, 2 '['
    loop {
        let mut byte = rt::call(SYS_READ, 0, 0) as u8;
        if byte == 0 {
            rt::sleep(1);
            continue;
        }
        // Serial arrow keys arrive as ESC [ D / ESC [ C.
        if esc == 1 {
            esc = if byte == b'[' { 2 } else { 0 };
            continue;
        }
        if esc == 2 {
            esc = 0;
            match byte {
                b'D' => byte = 0x12, // Left
                b'C' => byte = 0x13, // Right
                _ => continue,
            }
        } else if byte == 0x1b {
            esc = 1;
            continue;
        }
        match byte {
            b'\n' => {
                rt::print("\n");
                return String::from_utf8(bytes).unwrap_or_default();
            }
            8 => {
                // Delete before the cursor; redraw the shortened tail.
                if pos > 0 {
                    bytes.remove(pos - 1);
                    pos -= 1;
                    rt::vga_move(-1);
                    redraw_tail(&bytes, pos, pos);
                }
            }
            0x12 => {
                if pos > 0 {
                    pos -= 1;
                    rt::vga_move(-1);
                }
            }
            0x13 => {
                if pos < bytes.len() {
                    pos += 1;
                    rt::vga_move(1);
                }
            }
            0x10 | 0x11 => {} // Up/Down: reserved (history's seat)
            32..=126 => {
                if bytes.len() < 1024 {
                    bytes.insert(pos, byte);
                    redraw_tail(&bytes, pos, pos + 1);
                    pos += 1;
                }
            }
            _ => {}
        }
    }
}
fn read_file(path: &str) -> Result<Vec<u8>, rt::FsError> {
    match rt::stat(path) {
        Ok(entry) if entry.dir => Err(rt::FsError::IsDir),
        Ok(_) => rt::read_file(path).ok_or(rt::FsError::Io),
        Err(error) => Err(error),
    }
}

/// The reason the last filesystem call returned ERROR.
fn last_error() -> rt::FsError { rt::FsError::from_reason(rt::fs_error()) }
fn failed(operation: &str, error: rt::FsError) {
    rt::print_args(format_args!("{} failed: {}\n", operation, error.message()));
}

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
    let fd = rt::open_existing(path);
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
    let (input, output) = (rt::open_existing(from), rt::open(to));
    let result = (|| {
        if input == ERROR || output == ERROR { return Err(last_error()); }
        if rt::call(SYS_TRUNCATE, output, 0) != 0 { return Err(last_error()); }
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

/// Two whitespace-separated paths.
fn two(argument: &str) -> Option<(&str, &str)> {
    argument.split_once(' ').map(|(a, b)| (a.trim(), b.trim())).filter(|(a, b)| !a.is_empty() && !b.is_empty())
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

fn run_desktop() {
    if rt::call(SYS_DISPLAY_PRESENT, 0, 0) != 1 { rt::print("desktop: no display device\n"); return; }
    let pid = rt::spawn("desktop.elf");
    if pid == ERROR { rt::print("desktop: could not start\n"); return; }
    let status = rt::wait(pid);
    rt::call(SYS_CLEAR, 0, 0);
    rt::print(&format!("Desktop closed (status {}). Type 'desktop' to return.\n", status));
}
fn bench() {
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    for _ in 0..10000 { rt::yield_now(); }
    let cycles = unsafe { core::arch::x86_64::_rdtsc() } - start;
    rt::print(&format!("10,000 SYS_YIELDs took (CPU cycles): {}\n", cycles));
}
fn perf_bench() {
    let mut tsc_khz = 0u64;
    for _ in 0..10 {
        tsc_khz = rt::perf().map_or(0, |p| p.tsc_khz);
        if tsc_khz > 0 { break; }
        rt::sleep(2);
    }
    let cyc_to_ns = |cyc: u64, ops: u64| -> u64 {
        if tsc_khz > 0 && ops > 0 { ((cyc as u128 * 1_000_000) / (tsc_khz as u128 * ops as u128)) as u64 } else { 0 }
    };
    let cyc_to_us = |cyc: u64, ops: u64| -> u64 {
        if tsc_khz > 0 && ops > 0 { ((cyc as u128 * 1_000) / (tsc_khz as u128 * ops as u128)) as u64 } else { 0 }
    };
    let bytes_to_kibs = |bytes: u64, cyc: u64| -> u64 {
        if cyc > 0 && tsc_khz > 0 { ((bytes as u128 * tsc_khz as u128 * 1000) / (1024u128 * cyc as u128)) as u64 } else { 0 }
    };

    const N: u64 = 10_000;
    let t0 = rt::rdtsc();
    for _ in 0..N { let _ = rt::int80_call(SYS_GETPID, 0, 0); }
    let int80_ns = cyc_to_ns(rt::rdtsc().saturating_sub(t0), N);

    let t0 = rt::rdtsc();
    for _ in 0..N { let _ = rt::call(SYS_GETPID, 0, 0); }
    let syscall_ns = cyc_to_ns(rt::rdtsc().saturating_sub(t0), N);

    let t0 = rt::rdtsc();
    for _ in 0..N { let _ = rt::int80_call(SYS_YIELD, 0, 0); }
    let yield_int80_ns = cyc_to_ns(rt::rdtsc().saturating_sub(t0), N);

    let t0 = rt::rdtsc();
    for _ in 0..N { let _ = rt::call(SYS_YIELD, 0, 0); }
    let yield_syscall_ns = cyc_to_ns(rt::rdtsc().saturating_sub(t0), N);

    let t0 = rt::rdtsc();
    for _ in 0..32 {
        let ptr = rt::call(SYS_ALLOC, 256 * 1024, 4096);
        assert_ne!(ptr, ERROR);
        for page in 0..64 {
            unsafe { core::ptr::write_volatile((ptr as *mut u8).add(page * 4096), page as u8); }
        }
        assert_eq!(rt::call(SYS_FREE, ptr, 0), 0);
    }
    let alloc_us = cyc_to_us(rt::rdtsc().saturating_sub(t0), 32);

    let t0 = rt::rdtsc();
    for _ in 0..10 {
        let handle = rt::pipe().unwrap();
        let pid = rt::spawn_with("hello.elf", "", STDIO_CONSOLE, handle);
        assert_ne!(pid, ERROR);
        rt::pipe_close(handle, PIPE_WRITE_END);
        let mut sink = [0u8; 64];
        loop {
            match rt::pipe_read(handle, &mut sink) {
                Some(0) => break,
                Some(_) => {}
                None => rt::yield_now(),
            }
        }
        rt::wait(pid);
        rt::pipe_close(handle, PIPE_BOTH);
    }
    let spawn_us = cyc_to_us(rt::rdtsc().saturating_sub(t0), 10);

    let chunk = alloc::vec![0x5au8; 16 * 1024];
    let mut rbuf = alloc::vec![0u8; 16 * 1024];
    let fd = rt::open("/perf.tmp");
    assert_ne!(fd, ERROR);
    let _ = rt::call(SYS_TRUNCATE, fd, 0);
    let tw = rt::rdtsc();
    for _ in 0..16 { assert!(rt::file_write_all(fd, &chunk).is_ok()); }
    let file_w_kibs = bytes_to_kibs(256 * 1024, rt::rdtsc().saturating_sub(tw));
    let _ = rt::seek(fd, 0);
    let tr = rt::rdtsc();
    let mut got = 0usize;
    while got < 256 * 1024 {
        let n = rt::file_read(fd, &mut rbuf).unwrap_or(0);
        if n == 0 { break; }
        got += n;
    }
    let file_r_kibs = bytes_to_kibs(got as u64, rt::rdtsc().saturating_sub(tr));
    rt::close(fd);
    let _ = rt::remove("/perf.tmp");

    let stats = rt::perf().unwrap_or_default();
    let phys_used_kib = stats.phys_total_frames.saturating_sub(stats.phys_free_frames) * 4;
    let phys_total_kib = stats.phys_total_frames * 4;
    let slab_total = stats.slab_hits + stats.slab_misses;
    let slab_hit_pct = if slab_total > 0 { stats.slab_hits * 100 / slab_total } else { 100 };
    rt::print_args(format_args!(
        "PERF_CPU tsc_mhz={} busy_ticks={} idle_ticks={} syscalls={} ctx_switches={}\n",
        stats.tsc_khz / 1000, stats.busy_ticks, stats.idle_ticks, stats.syscalls, stats.context_switches
    ));
    rt::print_args(format_args!(
        "PERF_MEM phys_used_kib={} phys_total_kib={} heap_used_kib={} heap_cap_kib={} slab_hit_pct={}\n",
        phys_used_kib, phys_total_kib, stats.heap_used_bytes / 1024, stats.heap_capacity_bytes / 1024, slab_hit_pct
    ));
    rt::print_args(format_args!(
        "PERF_IO disk_reads={} disk_writes={} disk_flushes={} disk_read_kib={} disk_write_kib={}\n",
        stats.disk_reads, stats.disk_writes, stats.disk_flushes, stats.disk_read_bytes / 1024, stats.disk_write_bytes / 1024
    ));
    rt::print_args(format_args!(
        "PERF_BENCH int80_ns={} syscall_ns={} yield_int80_ns={} yield_syscall_ns={} alloc_us={} spawn_us={} file_w_kibs={} file_r_kibs={}\n",
        int80_ns, syscall_ns, yield_int80_ns, yield_syscall_ns, alloc_us, spawn_us, file_w_kibs, file_r_kibs
    ));
    rt::print("PERF_OK\n");
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
        Err(rt::FsError::NotFound) => Vec::new(),
        Err(rt::FsError::IsDir) => { rt::print("cannot edit a folder\n"); return; }
        Err(error) => { failed("edit", error); return; }
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
fn fs_status() {
    rt::print_args(format_args!("FS_STATUS {} files={}/{} bytes={}/{} buffers={}/{} generation={} disk={}\n",
        if rt::fs_stat(4) == 0 { "saved" } else { "unsaved" },
        rt::fs_stat(0), rt::fs_stat(1), rt::fs_stat(2), rt::fs_stat(3),
        rt::fs_stat(6), rt::fs_stat(7), rt::fs_stat(5), rt::fs_stat(10)));
}
fn execute(command: &str) {
    let (verb, argument) = command.split_once(' ').unwrap_or((command, ""));
    let argument = argument.trim();
    // Paths are relative to the working folder; the kernel resolves them.
    let here = |p: &str| if p.is_empty() { String::from(".") } else { String::from(p) };
    match verb {
        "" => {}
        "help" => rt::print("commands: help ls cd pwd mkdir rmdir cat edit echo cp rm mv df status fill sum clear msg desktop bench perf heaptest stats spawn run exec wait ps kill proctest selftest pairtest churn faulttest fstest storageprobe sync reboot ping\nuserspace: run hello.elf / sysinfo.elf / netstat.elf / calc.elf 2+3*4 / udpsend.elf <msg>\n"),
        "ls" => list(&here(argument)),
        "clear" => { rt::call(SYS_CLEAR, 0, 0); }
        "bench" => bench(),
        "perf" => perf_bench(),
        "heaptest" => heap_test(),
        "cat" => match read_file(&here(argument)) {
            Ok(bytes) => { if let Ok(text) = core::str::from_utf8(&bytes) { rt::print(text); } else { rt::print("binary file"); } rt::print("\n"); }
            Err(error) => rt::print_args(format_args!("cat: {}\n", error.message())),
        },
        "edit" => edit(&here(argument)),
        "echo" => if let Some((text, name)) = argument.split_once(" > ") {
            let fd = rt::open(name.trim());
            let line = format!("{}\n", text);
            if fd == ERROR || !rt::write(fd, line.as_bytes()) { failed("echo", last_error()); }
            if fd != ERROR { rt::close(fd); }
        } else { rt::print("usage: echo text > file\n"); },
        "rm" => match rt::remove_path(&here(argument)) {
            Ok(()) => rt::print("removed (sync to save)\n"),
            Err(e) => failed("rm", e),
        },
        // E40: the tree. mkdir/cd/pwd ride SYS_MKDIR/CHDIR/PWD; every
        // path-taking command resolves against the shell's cwd.
        "mkdir" => match rt::mkdir(&here(argument)) {
            Ok(()) => rt::print("folder created (sync to save)\n"),
            Err(e) => failed("mkdir", e),
        },
        "rmdir" => match rt::stat(&here(argument)) {
            Ok(e) if !e.dir => rt::print("rmdir: not a folder\n"),
            _ => match rt::remove_path(&here(argument)) {
                Ok(()) => rt::print("removed (sync to save)\n"),
                Err(e) => failed("rmdir", e),
            },
        },
        "cd" => {
            let target = if argument.is_empty() { "/" } else { argument };
            if let Err(error) = rt::chdir(target) { rt::print_args(format_args!("cd: {}\n", error.message())); }
        }
        "pwd" => rt::print_args(format_args!("{}\n", rt::cwd())),
        "mv" => match two(argument) {
            Some((from, to)) => match rt::move_path(from, &destination(from, to)) {
                Ok(()) => rt::print("renamed (sync to save)\n"),
                Err(e) => failed("mv", e),
            },
            None => rt::print("usage: mv old new\n"),
        },
        "cp" => match two(argument) {
            Some((from, to)) => match copy(from, &destination(from, to)) {
                Ok(n) => rt::print(&format!("copied {} (sync to save)\n", size_text(n))),
                Err(e) => failed("cp", e),
            },
            None => rt::print("usage: cp from to\n"),
        },
        "df" => disk_report(),
        "status" => fs_status(),
        "fill" => match argument.split_once(' ').map(|(p, n)| (p.trim(), n.trim().parse::<u64>())) {
            Some((path, Ok(size))) => fill(path, size),
            _ => rt::print("usage: fill file bytes\n"),
        },
        "sum" => sum(&here(argument)),
        "fstest" => {
            let pid = rt::spawn("fs-probe.elf");
            if pid != ERROR && rt::wait(pid) == 0 { rt::print("FSTEST_OK\n"); } else { rt::print("FSTEST_FAIL\n"); }
        }
        "storageprobe" => storage_probe::run(argument),
        "msg" => rt::print(if rt::send(2, argument) { "Message sent to Daemon\n" } else { "message rejected or mailbox full\n" }),
        "spawn" | "run" => match rt::arguments::words(argument) {
            Ok(words) => {
                let args: Vec<_> = words[1..].iter().map(String::as_str).collect();
                // run = spawn-and-keep-the-session: exec semantics
                // replaced the shell with the program, and an
                // interactive OS that dies after one command is not
                // playable. The shell survives its children. (`exec`
                // keeps the replacing form.)
                let pid = rt::spawn_args(&words[0], &args);
                if pid == ERROR { rt::print("spawn failed\n"); }
                else { rt::print_args(format_args!("spawned pid {}\n", pid)); }
            }
            Err(()) => rt::print("invalid program arguments or quoting\n"),
        },
        "exec" => match rt::arguments::words(argument) {
            Ok(words) => {
                let args: Vec<_> = words[1..].iter().map(String::as_str).collect();
                rt::exec_args(&words[0], &args);
                rt::print("exec failed\n");
            }
            Err(()) => rt::print("invalid program arguments or quoting\n"),
        },
        "ps" => match rt::processes() {
            Ok(tasks) => {
                rt::print("PID PARENT STATE    NAME\n");
                for task in tasks {
                    rt::print_args(format_args!("{:<3} {:<6} {:<8} {}\n", task.pid, task.parent, task.state_name(), task.name()));
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
        "desktop" => run_desktop(),
        "sync" => match rt::sync_status() {
            Ok(()) => { rt::print("SYNC_OK\n"); fs_status(); }
            Err(e) => { rt::print_args(format_args!("SYNC_FAILED: {}\n", e.message())); }
        },
        "reboot" => { rt::call(SYS_REBOOT, 0, 0); rt::print("reboot failed (sync required)\n"); }
        // Run ping in a child process.
        "ping" => {
            let child = rt::spawn("ping.elf");
            if child == ERROR { rt::print("ping: spawn refused (slot or image)\n"); }
        }
        _ => rt::print("Unknown command\n"),
    }
}

fn main() {
    rt::print("ATOM OS kernel shell\n");
    heap_test(); bench();
    rt::print("shell: ready\n");
    // Boot into the desktop when this is the console shell on a machine with
    // a display. A shell inside a desktop terminal has a piped stdin and
    // never gets here with a free display.
    if rt::args().len() == 1 && rt::call(SYS_GETPID, 0, 0) == 1 && rt::call(SYS_DISPLAY_PRESENT, 0, 0) == 1 {
        run_desktop();
    }
    loop { prompt(); let command = line(); execute(command.trim()); }
}
