#![no_std]
#![feature(alloc_error_handler)]
extern crate alloc;
#[path = "../../abi.rs"]
pub mod abi;
use abi::*;
use core::alloc::{GlobalAlloc, Layout};

#[inline]
pub fn call(number: u64, arg: u64, arg1: u64) -> u64 {
    let result: u64;
    unsafe { core::arch::asm!("int 0x80", inout("rax") number => result,
        in("rdi") arg, in("rsi") arg1, options(nostack, preserves_flags)); }
    result
}
#[inline]
pub fn call3(number: u64, arg: u64, arg1: u64, arg2: u64) -> u64 {
    let result: u64;
    unsafe { core::arch::asm!("int 0x80", inout("rax") number => result,
        in("rdi") arg, in("rsi") arg1, in("rdx") arg2, options(nostack, preserves_flags)); }
    result
}
pub fn yield_now() { call(SYS_YIELD, 0, 0); }
pub fn sleep(ticks: u64) { call(SYS_SLEEP, ticks, 0); }
pub fn exit(code: u64) -> ! { call(SYS_EXIT, code, 0); loop { yield_now(); } }
/// Writes to standard output, waiting while a pipe is full. A process whose
/// output pipe has no reader left exits with BROKEN_PIPE_STATUS.
pub fn print(text: &str) {
    let mut bytes = text.as_bytes();
    while !bytes.is_empty() {
        let chunk = &bytes[..bytes.len().min(4096)];
        match call(SYS_WRITE_BUFFER, chunk.as_ptr() as u64, chunk.len() as u64) {
            ERROR => exit(BROKEN_PIPE_STATUS),
            WOULD_BLOCK => yield_now(),
            written => bytes = &bytes[written as usize..],
        }
    }
}
/// Writes straight to the console, bypassing any output redirection.
pub fn console_print(text: &str) {
    for chunk in text.as_bytes().chunks(4096) { call(SYS_CONSOLE_WRITE, chunk.as_ptr() as u64, chunk.len() as u64); }
}
/// This process's argument string.
pub fn args() -> alloc::string::String {
    let mut buffer = [0u8; ARGS_MAX];
    let len = call(SYS_ARGS, buffer.as_mut_ptr() as u64, buffer.len() as u64);
    if len == ERROR { return alloc::string::String::new(); }
    alloc::string::String::from_utf8_lossy(&buffer[..(len as usize).min(ARGS_MAX)]).into_owned()
}
/// Reads standard input: Some(bytes) (empty at end of input), None if no data yet.
pub fn stdin_read(buffer: &mut [u8]) -> Option<usize> {
    match call(SYS_STDIN_READ, buffer.as_mut_ptr() as u64, buffer.len() as u64) {
        WOULD_BLOCK => None, ERROR => Some(0), count => Some(count as usize),
    }
}
/// Pipes owned by this process, addressed by handle.
pub fn pipe() -> Option<u64> { let h = call(SYS_PIPE, 0, 0); (h != ERROR).then_some(h) }
pub fn pipe_close(handle: u64, end: u64) { call(SYS_PIPE_CLOSE, handle, end); }
/// Some(n) bytes read (0 = end of input), None when nothing is buffered yet.
pub fn pipe_read(handle: u64, buffer: &mut [u8]) -> Option<usize> {
    match call3(SYS_PIPE_READ, handle, buffer.as_mut_ptr() as u64, buffer.len() as u64) {
        WOULD_BLOCK => None, ERROR => Some(0), count => Some(count as usize),
    }
}
/// Bytes accepted (possibly 0 when full), or None when no reader remains.
pub fn pipe_write(handle: u64, bytes: &[u8]) -> Option<usize> {
    match call3(SYS_PIPE_WRITE, handle, bytes.as_ptr() as u64, bytes.len() as u64) {
        WOULD_BLOCK => Some(0), ERROR => None, count => Some(count as usize),
    }
}
/// Starts `path` with `args`; stdin/stdout are STDIO_INHERIT, STDIO_CONSOLE or a pipe handle.
pub fn spawn_with(path: &str, args: &str, stdin: u64, stdout: u64) -> u64 {
    if path.is_empty() || path.len() > 63 || args.len() > ARGS_MAX || path.contains('\0') || args.contains('\0') { return ERROR; }
    let mut request = SpawnRequest { path: [0; 64], args: [0; ARGS_MAX + 1], stdin, stdout };
    request.path[..path.len()].copy_from_slice(path.as_bytes());
    request.args[..args.len()].copy_from_slice(args.as_bytes());
    call(SYS_SPAWN_WITH, &request as *const SpawnRequest as u64, 0)
}
pub fn print_args(arguments: core::fmt::Arguments<'_>) { format_with(arguments, print) }
/// Formats into a stack buffer (no heap use, so it works after an OOM).
fn format_with(arguments: core::fmt::Arguments<'_>, sink: fn(&str)) {
    struct Buffer { bytes: [u8; 512], len: usize }
    impl core::fmt::Write for Buffer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            if text.len() > self.bytes.len() - self.len { return Err(core::fmt::Error); }
            self.bytes[self.len..self.len + text.len()].copy_from_slice(text.as_bytes());
            self.len += text.len(); Ok(())
        }
    }
    let mut buffer = Buffer { bytes: [0; 512], len: 0 };
    if core::fmt::write(&mut buffer, arguments).is_ok() {
        sink(core::str::from_utf8(&buffer.bytes[..buffer.len]).unwrap_or("format error\n"));
    } else { sink("message too long\n"); }
}
pub fn path_call(number: u64, path: &str) -> u64 {
    match path_buffer(path) { Some(buffer) => call(number, buffer.as_ptr() as u64, 0), None => ERROR }
}
fn path_buffer(path: &str) -> Option<alloc::vec::Vec<u8>> {
    if path.is_empty() || path.len() > PATH_MAX || path.as_bytes().contains(&0) { return None; }
    let mut buffer = alloc::vec::Vec::with_capacity(path.len() + 1);
    buffer.extend_from_slice(path.as_bytes());
    buffer.push(0);
    Some(buffer)
}
pub fn remove(path: &str) -> bool { remove_path(path).is_ok() }
pub fn rename(from: &str, to: &str) -> bool { move_path(from, to).is_ok() }

/// Why a file operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError { NotFound, Exists, NotDir, IsDir, NotEmpty, Invalid, ReadOnly, Busy, NoSpace, Io, Other }
impl FsError {
    pub fn from_code(code: u64) -> Self {
        match code {
            ERR_NOT_FOUND => Self::NotFound, ERR_EXISTS => Self::Exists, ERR_NOT_DIR => Self::NotDir,
            ERR_IS_DIR => Self::IsDir, ERR_NOT_EMPTY => Self::NotEmpty, ERR_INVALID => Self::Invalid,
            ERR_READ_ONLY => Self::ReadOnly, ERR_BUSY => Self::Busy, ERR_NO_SPACE => Self::NoSpace,
            ERR_IO => Self::Io, _ => Self::Other,
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::NotFound => "not found", Self::Exists => "already exists", Self::NotDir => "not a folder",
            Self::IsDir => "is a folder", Self::NotEmpty => "folder is not empty", Self::Invalid => "invalid name",
            Self::ReadOnly => "read-only", Self::Busy => "in use", Self::NoSpace => "not enough space",
            Self::Io => "disk error", Self::Other => "failed",
        }
    }
}
fn status(code: u64) -> Result<(), FsError> { if code == 0 { Ok(()) } else { Err(FsError::from_code(code)) } }
fn path_status(number: u64, path: &str) -> Result<(), FsError> {
    let buffer = path_buffer(path).ok_or(FsError::Invalid)?;
    status(call(number, buffer.as_ptr() as u64, 0))
}
/// Removes a file or an empty folder.
pub fn remove_path(path: &str) -> Result<(), FsError> { path_status(SYS_REMOVE, path) }
/// Renames or moves a file or folder; the destination must not exist.
pub fn move_path(from: &str, to: &str) -> Result<(), FsError> {
    let (Some(from), Some(to)) = (path_buffer(from), path_buffer(to)) else { return Err(FsError::Invalid) };
    status(call(SYS_RENAME, from.as_ptr() as u64, to.as_ptr() as u64))
}
pub fn mkdir(path: &str) -> Result<(), FsError> { path_status(SYS_MKDIR, path) }

/// A file or folder as a listing reports it.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: alloc::string::String,
    /// Bytes for a file; number of entries for a folder.
    pub size: u64,
    pub modified: u64,
    pub dir: bool, pub builtin: bool, pub unsaved: bool,
}
impl Entry {
    fn from(raw: &DirEntry) -> Self {
        let len = (raw.name_len as usize).min(raw.name.len());
        Entry { name: alloc::string::String::from_utf8_lossy(&raw.name[..len]).into_owned(), size: raw.size,
                modified: raw.modified, dir: raw.flags & ENTRY_DIR != 0, builtin: raw.flags & ENTRY_BUILTIN != 0,
                unsaved: raw.flags & ENTRY_UNSAVED != 0 }
    }
}
pub fn stat(path: &str) -> Result<Entry, FsError> {
    let buffer = path_buffer(path).ok_or(FsError::Invalid)?;
    let mut raw = DirEntry::default();
    status(call(SYS_STAT, buffer.as_ptr() as u64, &mut raw as *mut DirEntry as u64))?;
    Ok(Entry::from(&raw))
}
/// The entries of a folder, folders first, then by name.
pub fn read_dir(path: &str) -> Result<alloc::vec::Vec<Entry>, FsError> {
    let buffer = path_buffer(path).ok_or(FsError::Invalid)?;
    let mut capacity = 64usize;
    loop {
        let mut raw = alloc::vec![DirEntry::default(); capacity];
        let count = call3(SYS_READ_DIR, buffer.as_ptr() as u64, raw.as_mut_ptr() as u64, capacity as u64);
        if count >= ERR_IO { return Err(FsError::from_code(count)); }
        if count as usize > capacity && capacity < 4096 { capacity = (count as usize).min(4096); continue; }
        let mut entries: alloc::vec::Vec<Entry> = raw.iter().take(count as usize).map(Entry::from).collect();
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        return Ok(entries);
    }
}
pub fn fs_info() -> FsInfo {
    let mut info = FsInfo::default();
    call(SYS_FS_INFO, &mut info as *mut FsInfo as u64, 0);
    info
}

/// Path helpers. Paths are absolute; `join` resolves `.` and `..` against a folder.
pub mod path {
    use alloc::string::String;
    use alloc::vec::Vec;
    /// `relative` (or an absolute path) resolved against the folder `base`.
    pub fn join(base: &str, relative: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let start = if relative.starts_with('/') { "" } else { base };
        for part in start.split('/').chain(relative.split('/')) {
            match part { "" | "." => {} ".." => { parts.pop(); } p => parts.push(p) }
        }
        let mut out = String::new();
        for p in &parts { out.push('/'); out.push_str(p); }
        if out.is_empty() { out.push('/'); }
        out
    }
    /// The folder containing `path` ("/" for the root's children and the root).
    pub fn parent(path: &str) -> String {
        match path.trim_end_matches('/').rfind('/') { Some(0) | None => String::from("/"), Some(i) => String::from(&path[..i]) }
    }
    /// `parent` without allocating, with a trailing `/` so it names the folder itself
    /// when joined ("/docs/a.txt" gives "/docs/").
    pub fn parent_str(path: &str) -> &str {
        match path.rfind('/') { Some(i) => &path[..=i], None => "/" }
    }
    /// The last component of `path`.
    pub fn name(path: &str) -> &str { path.trim_end_matches('/').rsplit('/').next().unwrap_or("") }
}
pub fn kill(pid: u64) -> bool { call(SYS_KILL, pid, 0) == 0 }
pub struct Process { pub pid: u32, pub parent: u32, pub state: u8, name: [u8; PROCESS_NAME_BYTES], name_len: u8 }
impl Process {
    pub fn name(&self) -> &str { core::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("?") }
    pub fn state_name(&self) -> &'static str {
        match self.state { STATE_READY => "ready", STATE_RUNNING => "running", STATE_BLOCKED => "blocked", STATE_EXITED => "exited", _ => "?" }
    }
}
pub fn processes() -> alloc::vec::Vec<Process> {
    const MAX: usize = 16;
    let mut buffer = [0u8; MAX * PROCESS_RECORD_BYTES];
    let count = call(SYS_PROCESSES, buffer.as_mut_ptr() as u64, MAX as u64);
    if count == ERROR { return alloc::vec::Vec::new(); }
    buffer.chunks(PROCESS_RECORD_BYTES).take(count as usize).map(|record| {
        let mut name = [0; PROCESS_NAME_BYTES];
        name.copy_from_slice(&record[16..16 + PROCESS_NAME_BYTES]);
        Process { pid: u32::from_le_bytes(record[0..4].try_into().unwrap()),
            parent: u32::from_le_bytes(record[4..8].try_into().unwrap()),
            state: record[8], name, name_len: record[9].min(PROCESS_NAME_BYTES as u8) }
    }).collect()
}
pub struct FileEntry { pub name: alloc::string::String, pub size: u32, pub builtin: bool }
pub fn list_files() -> alloc::vec::Vec<FileEntry> {
    const MAX: usize = 256;
    let mut buffer = alloc::vec![0u8; MAX * FILE_RECORD_BYTES];
    let count = call(SYS_LIST_FILES, buffer.as_mut_ptr() as u64, MAX as u64);
    if count == ERROR { return alloc::vec::Vec::new(); }
    buffer.chunks(FILE_RECORD_BYTES).take(count as usize).map(|record| {
        let len = record[..64].iter().position(|&b| b == 0).unwrap_or(64);
        FileEntry { name: alloc::string::String::from_utf8_lossy(&record[..len]).into_owned(),
            size: u32::from_le_bytes(record[64..68].try_into().unwrap()),
            builtin: u32::from_le_bytes(record[68..72].try_into().unwrap()) & FILE_BUILTIN != 0 }
    }).collect()
}
/// Reads a whole file (an existing one: a missing file is not created).
pub fn read_file(path: &str) -> Option<alloc::vec::Vec<u8>> {
    let entry = stat(path).ok().filter(|e| !e.dir)?;
    let fd = open(path);
    if fd == ERROR { return None; }
    let mut bytes = alloc::vec![0u8; entry.size as usize];
    let mut done = 0;
    while done < bytes.len() {
        match file_read(fd, &mut bytes[done..]) { Ok(0) | Err(_) => break, Ok(n) => done += n }
    }
    bytes.truncate(done);
    close(fd);
    Some(bytes)
}
/// Replaces a file's contents, creating it if needed.
pub fn write_file(path: &str, bytes: &[u8]) -> bool { write_file_status(path, bytes).is_ok() }
pub fn write_file_status(path: &str, bytes: &[u8]) -> Result<(), FsError> {
    let fd = open(path);
    if fd == ERROR { return Err(stat(path).err().unwrap_or(FsError::IsDir)); }
    let result = status(call(SYS_TRUNCATE, fd, 0)).and_then(|_| file_write_all(fd, bytes));
    close(fd);
    result
}
/// Reads at the descriptor's position; Ok(0) at the end of the file.
pub fn file_read(fd: u64, buffer: &mut [u8]) -> Result<usize, FsError> {
    let n = call3(SYS_FILE_READ, fd, buffer.as_mut_ptr() as u64, buffer.len().min(FILE_IO_MAX) as u64);
    if n >= ERR_IO { Err(FsError::from_code(n)) } else { Ok(n as usize) }
}
/// Writes at the descriptor's position.
pub fn file_write(fd: u64, bytes: &[u8]) -> Result<usize, FsError> {
    let n = call3(SYS_FILE_WRITE, fd, bytes.as_ptr() as u64, bytes.len().min(FILE_IO_MAX) as u64);
    if n >= ERR_IO { Err(FsError::from_code(n)) } else { Ok(n as usize) }
}
pub fn file_write_all(fd: u64, mut bytes: &[u8]) -> Result<(), FsError> {
    while !bytes.is_empty() {
        let n = file_write(fd, bytes)?;
        if n == 0 { return Err(FsError::Other); }
        bytes = &bytes[n..];
    }
    Ok(())
}
/// Moves the descriptor's position (clamped to the file size); returns it.
pub fn seek(fd: u64, position: u64) -> u64 { call(SYS_SEEK, fd, position) }
/// Saves every change to the data disk.
pub fn sync() -> bool { sync_status().is_ok() }
pub fn sync_status() -> Result<(), FsError> { status(call(SYS_SYNC, 0, 0)) }
pub fn time() -> u64 { call(SYS_TIME, 0, 0) }
pub fn ticks() -> u64 { call(SYS_TICKS, 0, 0) }
pub fn open(path: &str) -> u64 { path_call(SYS_OPEN, path) }
pub fn close(fd: u64) { call(SYS_CLOSE, fd, 0); }
pub fn read(fd: u64) -> Option<u8> { let byte = call(SYS_READ_FILE, fd, 0); if byte == ERROR { None } else { Some(byte as u8) } }
/// Appends to the end of the file.
pub fn write(fd: u64, bytes: &[u8]) -> bool {
    seek(fd, u64::MAX) != ERROR && file_write_all(fd, bytes).is_ok()
}
pub fn spawn(path: &str) -> u64 { path_call(SYS_SPAWN, path) }
pub fn wait(pid: u64) -> u64 { call(SYS_WAIT, pid, 0) }
pub fn send(pid: u64, text: &str) -> bool {
    if text.len() > 255 || text.as_bytes().contains(&0) { return false; }
    let mut bytes = [0; 256]; bytes[..text.len()].copy_from_slice(text.as_bytes());
    call(SYS_IPC_SEND, pid, bytes.as_ptr() as u64) == 0
}
pub fn receive() -> Option<alloc::string::String> {
    let pointer = call(SYS_IPC_RECV, 0, 0);
    if pointer == 0 || pointer == ERROR { return None; }
    let mut bytes = alloc::vec::Vec::new();
    unsafe {
        for offset in 0..255 {
            let byte = *((pointer + offset) as *const u8);
            if byte == 0 { break; } bytes.push(byte);
        }
    }
    alloc::string::String::from_utf8(bytes).ok()
}

/// Process heap: power-of-two size classes (16 B to 2 KiB) carved from
/// 64 KiB kernel allocations, with larger blocks allocated directly. Each
/// process is single-threaded, so the free lists need no locking.
struct ProcessHeap;
#[global_allocator]
static HEAP: ProcessHeap = ProcessHeap;
const CLASSES: usize = 8; // 16, 32, ..., 2048 bytes
const CHUNK: usize = 64 * 1024;
static mut FREE: [usize; CLASSES] = [0; CLASSES];

fn class_of(layout: &Layout) -> Option<usize> {
    let size = layout.size().max(layout.align()).max(16).next_power_of_two();
    (size <= 2048).then(|| size.trailing_zeros() as usize - 4)
}
unsafe impl GlobalAlloc for ProcessHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(class) = class_of(&layout) else {
            let address = call(SYS_ALLOC, layout.size().max(1) as u64, layout.align() as u64);
            return if address == ERROR { core::ptr::null_mut() } else { address as *mut u8 };
        };
        unsafe {
            let free = &raw mut FREE;
            if (*free)[class] == 0 {
                let chunk = call(SYS_ALLOC, CHUNK as u64, 4096);
                if chunk == ERROR { return core::ptr::null_mut(); }
                // Thread the new chunk's blocks onto the free list.
                let size = 16usize << class;
                for block in (0..CHUNK / size).rev() {
                    let address = chunk as usize + block * size;
                    *(address as *mut usize) = (*free)[class];
                    (*free)[class] = address;
                }
            }
            let block = (*free)[class];
            (*free)[class] = *(block as *const usize);
            block as *mut u8
        }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        match class_of(&layout) {
            Some(class) => unsafe {
                let free = &raw mut FREE;
                *(pointer as *mut usize) = (*free)[class];
                (*free)[class] = pointer as usize;
            },
            None => { call(SYS_FREE, pointer as u64, 0); }
        }
    }
}
#[alloc_error_handler]
fn allocation_failed(_: Layout) -> ! { console_print("USER_OOM\n"); exit(100) }
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    format_with(format_args!("USER_PANIC {info}\n"), console_print);
    exit(101)
}

#[macro_export]
macro_rules! entry {
    ($main:path) => {
        core::arch::global_asm!(".global _start", "_start:", "cld", "and rsp, -16", "call atom_user_entry", "ud2");
        #[unsafe(no_mangle)]
        pub extern "C" fn atom_user_entry() -> ! { $main(); $crate::exit(0) }
    };
}
