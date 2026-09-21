#![no_std]
#![feature(alloc_error_handler)]
extern crate alloc;
#[path = "../../abi.rs"]
pub mod abi;
use abi::*;
use core::alloc::{GlobalAlloc, Layout};

#[inline]
pub fn call(number: u64, arg: u64, arg1: u64) -> u64 {
    call3(number, arg, arg1, 0)
}
#[inline]
pub fn call3(number: u64, arg: u64, arg1: u64, arg2: u64) -> u64 {
    let result: u64;
    unsafe {
        core::arch::asm!("int 0x80", inout("rax") number => result,
        in("rdi") arg, in("rsi") arg1, in("rdx") arg2, options(nostack, preserves_flags));
    }
    result
}
pub fn yield_now() {
    call(SYS_YIELD, 0, 0);
}
pub fn sleep(ticks: u64) {
    call(SYS_SLEEP, ticks, 0);
}
pub fn exit(code: u64) -> ! {
    call(SYS_EXIT, code, 0);
    loop {
        yield_now();
    }
}
pub fn try_print(text: &str) -> bool {
    for chunk in text.as_bytes().chunks(4096) {
        if call(SYS_WRITE_BUFFER, chunk.as_ptr() as u64, chunk.len() as u64) != chunk.len() as u64 {
            return false;
        }
    }
    true
}
pub fn print(text: &str) {
    let _ = try_print(text);
}
pub fn print_args(arguments: core::fmt::Arguments<'_>) {
    struct Buffer {
        bytes: [u8; 512],
        len: usize,
    }
    impl core::fmt::Write for Buffer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            if text.len() > self.bytes.len() - self.len {
                return Err(core::fmt::Error);
            }
            self.bytes[self.len..self.len + text.len()].copy_from_slice(text.as_bytes());
            self.len += text.len();
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: [0; 512],
        len: 0,
    };
    if core::fmt::write(&mut buffer, arguments).is_ok() {
        print(core::str::from_utf8(&buffer.bytes[..buffer.len]).unwrap_or("format error\n"));
    } else {
        print("message too long\n");
    }
}
pub fn path_call(number: u64, path: &str) -> u64 {
    if path.is_empty() || path.len() > 63 || path.as_bytes().contains(&0) {
        return ERROR;
    }
    let mut buffer = [0; 64];
    buffer[..path.len()].copy_from_slice(path.as_bytes());
    call(number, buffer.as_ptr() as u64, 0)
}
pub fn open(path: &str) -> u64 {
    path_call(SYS_OPEN, path)
}
pub fn open_existing(path: &str) -> u64 {
    path_call(SYS_OPEN_EXISTING, path)
}
pub fn close(fd: u64) {
    call(SYS_CLOSE, fd, 0);
}
pub fn read(fd: u64) -> Option<u8> {
    let byte = call(SYS_READ_FILE, fd, 0);
    if byte == ERROR {
        None
    } else {
        Some(byte as u8)
    }
}
pub fn write(fd: u64, bytes: &[u8]) -> bool {
    call3(
        SYS_WRITE_FILE_BUFFER,
        fd,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
    ) == bytes.len() as u64
}
pub fn replace(fd: u64, bytes: &[u8]) -> bool {
    call3(
        SYS_REPLACE_FILE,
        fd,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
    ) == bytes.len() as u64
}
pub fn remove(path: &str) -> bool {
    path_call(SYS_REMOVE, path) == 0
}
pub fn rename(old: &str, new: &str) -> bool {
    if old.is_empty() || old.len() > 63 || new.is_empty() || new.len() > 63 {
        return false;
    }
    let mut from = [0; 64];
    let mut to = [0; 64];
    from[..old.len()].copy_from_slice(old.as_bytes());
    to[..new.len()].copy_from_slice(new.as_bytes());
    call(SYS_RENAME, from.as_ptr() as u64, to.as_ptr() as u64) == 0
}
pub fn fs_error() -> u64 {
    call(SYS_FS_ERROR, 0, 0)
}
pub fn fs_stat(which: u64) -> u64 {
    call(SYS_FS_STAT, which, 0)
}
pub fn spawn(path: &str) -> u64 {
    path_call(SYS_SPAWN, path)
}

/// E40: move the VGA cursor by `delta` cells (negative = left).
pub fn vga_move(delta: i64) {
    call3(SYS_VGA, 0, delta as u64, 0);
}

/// E40: the VGA cursor position, packed (col | row << 8).
pub fn vga_pos() -> u64 {
    call3(SYS_VGA, 1, 0, 0)
}
pub fn wait(pid: u64) -> u64 {
    call(SYS_WAIT, pid, 0)
}
pub fn send(pid: u64, text: &str) -> bool {
    if text.len() > 255 || text.as_bytes().contains(&0) {
        return false;
    }
    let mut bytes = [0; 256];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    call(SYS_IPC_SEND, pid, bytes.as_ptr() as u64) == 0
}
pub mod ipc;
pub use ipc::ReceiveError;

/// Receive one complete message into caller-owned storage without allocation.
/// Buffers must hold MAX_MESSAGE_BYTES, even for shorter messages. Too-small
/// buffers are rejected BEFORE the syscall, leaving the pending message queued.
/// The returned length excludes the NUL; bytes after it are untouched.
/// Ok(None) means an empty mailbox; Ok(Some(0)) is an actual empty message.
pub fn receive_into(buffer: &mut [u8]) -> Result<Option<usize>, ReceiveError> {
    ipc::check_capacity(buffer)?;
    let pointer = call(SYS_IPC_RECV, 0, 0);
    if pointer == 0 {
        return Ok(None);
    }
    if pointer == ERROR {
        return Err(ReceiveError::Syscall);
    }
    // SYS_IPC_RECV maps a process-private 4 KiB receive page and writes a
    // NUL-terminated message of <=255 bytes. Copy before another receive can
    // replace it; no reference to that page escapes this function.
    let source = unsafe { core::slice::from_raw_parts(pointer as *const u8, ipc::RECEIVE_BYTES) };
    ipc::copy_message(source, buffer).map(Some)
}

/// Compatibility convenience API. Persistent receivers should use receive_into.
pub fn receive() -> Option<alloc::string::String> {
    let mut buffer = [0; ipc::MAX_MESSAGE_BYTES];
    let len = receive_into(&mut buffer).ok()??;
    alloc::string::String::from_utf8(buffer[..len].to_vec()).ok()
}

struct ProcessHeap;
#[global_allocator]
static HEAP: ProcessHeap = ProcessHeap;
unsafe impl GlobalAlloc for ProcessHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let address = call(
            SYS_ALLOC,
            layout.size().max(1) as u64,
            layout.align() as u64,
        );
        if address == ERROR {
            core::ptr::null_mut()
        } else {
            address as *mut u8
        }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, _: Layout) {
        call(SYS_FREE, pointer as u64, 0);
    }
}
#[alloc_error_handler]
fn allocation_failed(_: Layout) -> ! {
    print("USER_OOM\n");
    exit(100)
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print_args(format_args!("USER_PANIC {info}\n"));
    exit(101)
}

#[macro_export]
macro_rules! entry {
    ($main:path) => {
        core::arch::global_asm!(
            ".global _start",
            "_start:",
            "cld",
            "and rsp, -16",
            "call atom_user_entry",
            "ud2"
        );
        #[unsafe(no_mangle)]
        pub extern "C" fn atom_user_entry() -> ! {
            $main();
            $crate::exit(0)
        }
    };
}

#[path = "../../arguments.rs"]
pub mod arguments;

/// Arguments include the executable name at index zero, including legacy spawn.
pub fn args() -> alloc::vec::Vec<alloc::string::String> {
    let mut bytes = [0; MAX_ARG_BYTES];
    let len = call(SYS_ARGS, bytes.as_mut_ptr() as u64, bytes.len() as u64);
    assert!(len != ERROR && len > 0 && len <= bytes.len() as u64);
    bytes[..len as usize - 1]
        .split(|&b| b == 0)
        .map(|arg| alloc::string::String::from_utf8(arg.to_vec()).unwrap())
        .collect()
}
fn launch(number: u64, path: &str, args: &[&str]) -> u64 {
    let mut extra = alloc::vec::Vec::new();
    if args.len() >= MAX_ARGS {
        return ERROR;
    }
    for arg in args {
        if arg.as_bytes().contains(&0) || arg.len() + 1 > MAX_ARG_BYTES.saturating_sub(extra.len())
        {
            return ERROR;
        }
        extra.extend_from_slice(arg.as_bytes());
        extra.push(0);
    }
    if arguments::pack(path, &extra).is_err() {
        return ERROR;
    }
    let mut name = [0; 64];
    name[..path.len()].copy_from_slice(path.as_bytes());
    call3(
        number,
        name.as_ptr() as u64,
        extra.as_ptr() as u64,
        extra.len() as u64,
    )
}
pub fn spawn_args(path: &str, args: &[&str]) -> u64 {
    launch(SYS_SPAWN_ARGS, path, args)
}
pub fn exec_args(path: &str, args: &[&str]) -> u64 {
    launch(SYS_EXEC_ARGS, path, args)
}
pub fn kill(pid: u64) -> bool {
    call(SYS_KILL, pid, 0) == 0
}
pub fn processes() -> Result<alloc::vec::Vec<ProcessInfo>, ()> {
    let mut records = [ProcessInfo::EMPTY; MAX_PROCESSES];
    let count = call(
        SYS_PROCESSES,
        records.as_mut_ptr() as u64,
        MAX_PROCESSES as u64,
    );
    if count > MAX_PROCESSES as u64 {
        return Err(());
    }
    Ok(records[..count as usize].to_vec())
}
