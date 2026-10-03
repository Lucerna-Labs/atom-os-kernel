#![no_std]
#![no_main]
extern crate alloc;
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn main() {
    // Child mode for the pipe check below: echo the arguments to stdout.
    let args = rt::args();
    if let Some(text) = args.strip_prefix("pipe-child ") {
        rt::print(text);
        rt::print("\n");
        rt::exit(7);
    }
    let pid = rt::call(SYS_GETPID, 0, 0);
    // Reject untrusted pointers without taking a kernel exception.
    assert_eq!(rt::call(SYS_OPEN, 0, 0), ERROR);
    assert_eq!(rt::call(SYS_OPEN, 0x200000, 0), ERROR);
    assert_eq!(rt::call(SYS_WRITE_BUFFER, u64::MAX, 8), ERROR);
    let page = rt::call(SYS_ALLOC, 4096, 4096);
    assert_ne!(page, ERROR);
    unsafe { *((page + 4095) as *mut u8) = b'x'; }
    assert_eq!(rt::call(SYS_WRITE_BUFFER, page + 4095, 2), ERROR);
    assert_eq!(rt::call(SYS_OPEN, page + 4095, 0), ERROR);
    assert_eq!(rt::call(SYS_FREE, page, 0), 0);
    assert_eq!(rt::call(SYS_FREE, page, 0), ERROR);

    let mut bytes = alloc::vec![0u8; 65536];
    for (index, value) in bytes.iter_mut().enumerate() { *value = ((index as u64 ^ pid) % 251) as u8; }
    for _ in 0..100 { rt::yield_now(); }
    assert!(bytes.iter().enumerate().all(|(i, &v)| v == ((i as u64 ^ pid) % 251) as u8));
    drop(bytes);

    let held = alloc::format!("held{}.txt", pid % 16);
    let fd = rt::open(&held); assert_ne!(fd, ERROR);
    assert_eq!(rt::call(SYS_TRUNCATE, fd, 0), 0);
    for i in 0..32 {
        let name = alloc::format!("growth{}-{}.txt", pid % 16, i);
        let temporary = rt::open(&name); assert_ne!(temporary, ERROR); rt::close(temporary);
    }
    assert!(rt::write(fd, b"stable handle")); rt::close(fd);
    let fd = rt::open(&held);
    for &expected in b"stable handle" { assert_eq!(rt::read(fd), Some(expected)); }
    assert_eq!(rt::read(fd), None);
    // An open descriptor pins its file; built-ins can never be removed or renamed.
    assert!(!rt::remove(&held));
    rt::close(fd);
    let moved = alloc::format!("moved{}.txt", pid % 16);
    rt::remove(&moved);
    assert!(rt::rename(&held, &moved));
    assert!(!rt::rename(&moved, "/bin/worker.elf"));
    assert!(!rt::remove("/bin/worker.elf") && !rt::rename("/bin/worker.elf", "renamed.elf"));
    assert!(!rt::rename("/bin", "/programs"));
    let fd = rt::open(&moved);
    for &expected in b"stable handle" { assert_eq!(rt::read(fd), Some(expected)); }
    rt::close(fd);
    assert!(rt::remove(&moved) && !rt::remove(&moved));
    for i in 0..32 { assert!(rt::remove(&alloc::format!("growth{}-{}.txt", pid % 16, i))); }
    let mut ours = false;
    for process in rt::processes() { if process.pid as u64 == pid && process.name() == "worker.elf" { ours = true; } }
    assert!(ours);
    assert!(!rt::kill(0));

    let fast_pid: u64;
    unsafe {
        core::arch::asm!("syscall", inout("rax") SYS_GETPID => fast_pid,
            lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    assert_eq!(pid, fast_pid);
    // A fast yield can resume through another task's int-0x80 frame.
    for _ in 0..32 {
        unsafe { core::arch::asm!("syscall", inout("rax") SYS_YIELD => _, lateout("rcx") _, lateout("r11") _, options(nostack)); }
    }
    let pattern = [0x123456789abcdef0u64, 0xfedcba9876543210];
    let mut observed = [0u64; 4];
    unsafe {
        core::arch::asm!(
            "movdqu xmm0, [{input}]", "movdqu xmm15, [{input}]", "mov rcx, 2000",
            "2:", "mov rax, 1", "int 0x80", "loop 2b",
            "movdqu [{output}], xmm0", "movdqu [{output} + 16], xmm15",
            input = in(reg) pattern.as_ptr(), output = in(reg) observed.as_mut_ptr(),
            out("rax") _, out("rcx") _, out("xmm0") _, out("xmm15") _, options(nostack));
    }
    assert_eq!(&observed[..2], &pattern); assert_eq!(&observed[2..], &pattern);
    // Wait for a real timer tick while SIMD and MXCSR contain process-specific
    // state. Timer IRQs can only advance this counter outside the syscall gate.
    let mxcsr = if pid & 1 == 0 { 0x3f80u32 } else { 0x5f80u32 };
    let default_mxcsr = 0x1f80u32;
    let mut observed_mxcsr = 0u32;
    let ticks: u64;
    unsafe {
        core::arch::asm!(
            "movdqu xmm0, [{input}]", "ldmxcsr [{control}]",
            "mov rax, 31", "int 0x80", "mov r12, rax", "mov rcx, 1000000",
            "2:", "pause", "mov rax, 31", "int 0x80", "cmp rax, r12", "jne 3f", "loop 2b",
            "3:", "sub rax, r12", "movdqu [{output}], xmm0", "stmxcsr [{observed_control}]", "ldmxcsr [{default_control}]",
            input = in(reg) pattern.as_ptr(), control = in(reg) &mxcsr,
            output = in(reg) observed.as_mut_ptr(), observed_control = in(reg) &mut observed_mxcsr,
            default_control = in(reg) &default_mxcsr,
            out("rax") ticks, out("rcx") _, out("r12") _, out("xmm0") _, options(nostack));
    }
    assert!(ticks > 0); assert_eq!(&observed[..2], &pattern); assert_eq!(observed_mxcsr, mxcsr);
    // Arguments and pipes: the child's stdout arrives here, then end of input.
    let handle = rt::pipe().unwrap();
    let child = rt::spawn_with("worker.elf", "pipe-child piped hello", STDIO_CONSOLE, handle);
    assert_ne!(child, ERROR);
    rt::pipe_close(handle, PIPE_WRITE_END);
    let mut output = alloc::vec::Vec::new();
    let mut buffer = [0u8; 64];
    loop {
        match rt::pipe_read(handle, &mut buffer) {
            Some(0) => break,
            Some(count) => output.extend_from_slice(&buffer[..count]),
            None => rt::yield_now(),
        }
    }
    assert_eq!(output, b"piped hello\n");
    assert_eq!(rt::wait(child), 7);
    rt::pipe_close(handle, PIPE_BOTH);
    assert_eq!(rt::spawn_with("worker.elf", "", 99, STDIO_INHERIT), ERROR);
    let mut sent = false;
    for _ in 0..200 { if rt::send(2, "worker delivered") { sent = true; break; } rt::sleep(1); }
    assert!(sent);
    rt::print("WORKER_OK heap pointers fd fast-syscall xmm ipc\n");
    rt::exit(37)
}
