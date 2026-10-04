use alloc::{string::String, vec::Vec};
use kernel_kit::address_space::{STACK_TOP, RECV_BASE};
use kernel_kit::context::{Context, TaskState};
use kernel_kit::paging::{Cr3, phys_to_virt};
use kernel_kit::trap::TrapFrame;
use crate::system::System;

pub use crate::abi::*;

use core::sync::atomic::{AtomicU64, Ordering};
static SENSOR_CYCLES: AtomicU64 = AtomicU64::new(0);
static SENSOR_CALLS: AtomicU64 = AtomicU64::new(0);

fn is(value: u64, code: u64) -> bool {
    !kernel_kit::atoms::compare(&value, &code) && !kernel_kit::atoms::compare(&code, &value)
}

pub fn user_string(context: &Context, address: u64, limit: usize) -> Result<String, ()> {
    let space = context.space.as_ref().ok_or(())?;
    let mut bytes = Vec::new();
    for offset in 0..limit {
        let ptr = address.checked_add(offset as u64).ok_or(())?;
        let phys = space.translate_user(ptr, false).ok_or(())?;
        let byte = unsafe { *(phys_to_virt(phys) as *const u8) };
        if byte == 0 { return String::from_utf8(bytes).map_err(|_| ()); }
        bytes.push(byte);
    }
    Err(())
}

/// The lane's transit line — the observation point made visible.
/// Written straight to serial (kernel-side), so lane traffic never
/// passes through the cone-gated write channels: this IS the one way
/// out, and the log line is its receipt.
fn lane_transit_line(transit: u64, word: u64, real: bool) {
    let line = alloc::format!(
        "[LANE] transit #{} word {:#x} ({})\n",
        transit,
        word,
        if real { "real" } else { "HONEY" }
    );
    let (mut serial, flags) = kernel_kit::serial::SERIAL1.lock();
    for byte in line.as_bytes() {
        serial.send(*byte);
    }
    kernel_kit::serial::SERIAL1.unlock(flags);
}

use kernel_kit::fs::{Fs, FsError as TreeError, Ino, ROOT_FS};
/// The ABI's numbered filesystem errors (SYS_FS_ERROR).
use crate::abi::FsError as FsCode;

fn code(error: TreeError) -> FsCode {
    match error {
        TreeError::NotFound => FsCode::NotFound, TreeError::Exists => FsCode::Exists, TreeError::NotDir => FsCode::NotDir,
        TreeError::IsDir => FsCode::IsDir, TreeError::NotEmpty => FsCode::NotEmpty, TreeError::Invalid => FsCode::InvalidName,
        TreeError::ReadOnly => FsCode::ReadOnly, TreeError::Busy => FsCode::Busy, TreeError::NoSpace => FsCode::NoSpace,
        TreeError::Io => FsCode::Io,
    }
}
/// The specific error results of the page-backed file calls (SYS_FILE_READ ...).
fn err_code(error: TreeError) -> u64 {
    match error {
        TreeError::NotFound => ERR_NOT_FOUND, TreeError::Exists => ERR_EXISTS, TreeError::NotDir => ERR_NOT_DIR,
        TreeError::IsDir => ERR_IS_DIR, TreeError::NotEmpty => ERR_NOT_EMPTY, TreeError::Invalid => ERR_INVALID,
        TreeError::ReadOnly => ERR_READ_ONLY, TreeError::Busy => ERR_BUSY, TreeError::NoSpace => ERR_NO_SPACE,
        TreeError::Io => ERR_IO,
    }
}
fn disk_code(error: kernel_kit::virtio_blk::DiskError) -> FsCode {
    match error { kernel_kit::virtio_blk::DiskError::Full => FsCode::NoSpace, _ => FsCode::Io }
}
/// Records why a page-backed file call failed and returns its specific result.
fn failed(context: &mut Context, error: TreeError) -> u64 {
    context.fs_error = code(error) as u64;
    err_code(error)
}

/// The original filesystem convention: a value, or ERROR with the reason left
/// for SYS_FS_ERROR.
fn fs_result(context: &mut Context, frame: &mut TrapFrame, result: Result<u64, FsCode>) {
    match result {
        Ok(value) => { frame.rax = value; context.fs_error = 0; }
        Err(error) => { frame.rax = ERROR; context.fs_error = error as u64; }
    }
}

/// A path argument resolved against the caller's working folder.
fn path_at(context: &Context, address: u64) -> Result<String, FsCode> {
    let raw = user_string(context, address, kernel_kit::fs::PATH_MAX + 1).map_err(|_| FsCode::BadBuffer)?;
    kernel_kit::fs::join(&context.cwd, &raw).map_err(code)
}

/// A program name: a bare name is looked up in /bin then the root; a name with a
/// '/' is a path from the caller's working folder.
fn program_at(context: &Context, address: u64) -> Result<String, ()> {
    let name = user_string(context, address, 64)?;
    if !name.contains('/') { return Ok(name); }
    kernel_kit::fs::join(&context.cwd, &name).map_err(|_| ())
}

/// A descriptor's file: entries hold the inode number plus one (0 = free slot).
fn descriptor(context: &Context, fd: u64) -> Option<(Ino, u64)> {
    let (slot, cursor) = *context.open_files.get(fd as usize)?;
    (slot != 0).then_some(((slot - 1) as Ino, cursor as u64))
}

fn dir_entry(fs: &Fs, ino: Ino) -> DirEntry {
    let mut entry = DirEntry::default();
    if let Some(node) = fs.node(ino) {
        let name = node.name.as_bytes();
        entry.name[..name.len()].copy_from_slice(name);
        entry.name_len = name.len() as u32;
        entry.size = node.size();
        entry.modified = node.modified;
        if node.is_dir() { entry.flags |= ENTRY_DIR; }
        if fs.is_builtin(ino) { entry.flags |= ENTRY_BUILTIN; }
        if node.file().is_some_and(|f| f.dirty) { entry.flags |= ENTRY_UNSAVED; }
    }
    entry
}
fn as_bytes<T>(value: &T) -> &[u8] { unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) } }

/// Copies kernel bytes into the current process through its own page tables,
/// after checking the whole destination is mapped, user-accessible and writable.
fn copy_to_user(context: &Context, address: u64, bytes: &[u8]) -> bool {
    let Some(space) = context.space.as_ref() else { return false; };
    if !space.valid_user_range(address, bytes.len(), true) { return false; }
    let mut done = 0;
    while done < bytes.len() {
        let virt = address + done as u64;
        let count = (4096 - (virt & 4095) as usize).min(bytes.len() - done);
        let Some(phys) = space.translate_user(virt, true) else { return false; };
        unsafe { core::ptr::copy_nonoverlapping(bytes[done..].as_ptr(), phys_to_virt(phys) as *mut u8, count); }
        done += count;
    }
    true
}

/// Copies `len` bytes out of the current process, checking every page.
fn copy_from_user(context: &Context, address: u64, len: usize) -> Option<Vec<u8>> {
    let space = context.space.as_ref()?;
    if !space.valid_user_range(address, len, false) { return None; }
    let mut bytes = alloc::vec![0u8; len];
    let mut done = 0;
    while done < len {
        let virt = address + done as u64;
        let count = (4096 - (virt & 4095) as usize).min(len - done);
        let phys = space.translate_user(virt, false)?;
        unsafe { core::ptr::copy_nonoverlapping(phys_to_virt(phys) as *const u8, bytes[done..].as_mut_ptr(), count); }
        done += count;
    }
    Some(bytes)
}

/// Writes to the process's standard output: the console, or its pipe.
fn stdout_write(context: &Context, bytes: &[u8]) -> u64 {
    match &context.stdout {
        None => {
            let mut writer = kernel_kit::vga::VgaWriter::new();
            for &byte in bytes { writer.write_byte(byte); }
            bytes.len() as u64
        }
        Some(end) => match end.write(bytes) {
            kernel_kit::pipe::Write::Wrote(count) => count as u64,
            kernel_kit::pipe::Write::WouldBlock => WOULD_BLOCK,
            kernel_kit::pipe::Write::Broken => BROKEN_PIPE,
        },
    }
}

fn pipe_read(end: &kernel_kit::pipe::PipeEnd, context: &Context, address: u64, len: usize) -> u64 {
    if !context.space.as_ref().unwrap().valid_user_range(address, len, true) { return ERROR; }
    let mut buffer = alloc::vec![0u8; len];
    match end.read(&mut buffer) {
        kernel_kit::pipe::Read::Data(count) => if copy_to_user(context, address, &buffer[..count]) { count as u64 } else { ERROR },
        kernel_kit::pipe::Read::WouldBlock => WOULD_BLOCK,
        kernel_kit::pipe::Read::End => 0,
    }
}

/// Next console character: the serial line first (CR is newline, DEL is
/// backspace), then the keyboard.
fn console_byte() -> Option<u8> {
    let (serial, flags) = kernel_kit::serial::SERIAL1.lock();
    let byte = serial.receive();
    kernel_kit::serial::SERIAL1.unlock(flags);
    if let Some(byte) = byte { return Some(match byte { b'\r' => b'\n', 127 => 8, b => b }); }
    kernel_kit::input::console_byte()
}

fn nul_string(bytes: &[u8]) -> Option<String> {
    let len = bytes.iter().position(|&b| b == 0)?;
    String::from_utf8(bytes[..len].to_vec()).ok()
}

/// True when any live descriptor in any process references this file object.
fn file_open(system: &System, ino: Ino) -> bool {
    system.scheduler.tasks.iter().flatten().any(|task| task.open_files.iter().any(|entry| entry.0 == ino as u64 + 1))
}

/// Called with interrupts disabled and the actual interrupted frame registered
/// in the current context. Never dispatch through an earlier saved frame.
pub fn dispatch(system: &mut System, rsp: u64) -> u64 {
    system.scheduler.collect();
    let ticks = system.scheduler.ticks;
    let Some(context) = system.scheduler.current_task_mut() else { return rsp; };
    context.rsp = rsp;
    let pid = context.id;
    let frame = unsafe { &mut *(rsp as *mut TrapFrame) };
    let number = frame.rax;
    let arg = frame.rdi;
    let arg1 = frame.rsi;
    let arg2 = frame.rdx;
    frame.rax = ERROR;
    let mut switch = false;
    // E21 shadow web: every syscall is a vibration at the kernel's
    // single chokepoint. The sensor learns who talks to whom; after
    // its cone freezes, foreign conversations spend quarantine budget.
    // Never a payload — class, pid, target, weight only. The rdtsc
    // pair is the T3 measurement the design doc requires before any
    // primitive is trusted live: the honest cost of feeling.
    {
        let started = unsafe { core::arch::x86_64::_rdtsc() };
        kernel_sense::record(pid as u64, number, arg, 1.0);
        SENSOR_CYCLES.fetch_add(unsafe { core::arch::x86_64::_rdtsc() } - started, Ordering::Relaxed);
        SENSOR_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    if is(number, SYS_YIELD) {
        frame.rax = 0; switch = true;
    } else if is(number, SYS_ALLOC) {
        frame.rax = context.space.as_mut().unwrap().user_alloc_aligned(arg as usize, arg1.max(1) as usize).unwrap_or(ERROR);
    } else if is(number, SYS_FREE) {
        if context.space.as_mut().unwrap().user_free(arg) { frame.rax = 0; }
    } else if is(number, SYS_EXIT) {
        system.exit_current(arg); switch = true;
    } else if is(number, SYS_READ) {
        frame.rax = if let Some(end) = &context.stdin {
            let mut byte = [0u8];
            match end.read(&mut byte) { kernel_kit::pipe::Read::Data(1) => byte[0] as u64, _ => 0 }
        } else if system.display_owner.is_some() {
            0 // While a desktop owns the display, keystrokes belong to it alone.
        } else { console_byte().unwrap_or(0) as u64 };
    } else if is(number, SYS_STDIN_READ) {
        let len = (arg1 as usize).min(4096);
        frame.rax = if let Some(end) = &context.stdin { pipe_read(end, context, arg, len) }
        else if system.display_owner.is_some() { WOULD_BLOCK }
        else {
            let mut bytes = Vec::new();
            let mut ended = false;
            while bytes.len() < len {
                match console_byte() {
                    Some(27) => { ended = true; break; }
                    Some(byte) => bytes.push(byte),
                    None => break,
                }
            }
            if bytes.is_empty() { if ended { 0 } else { WOULD_BLOCK } }
            else if copy_to_user(context, arg, &bytes) { bytes.len() as u64 } else { ERROR }
        };
    } else if is(number, SYS_CONSOLE_WRITE) {
        if arg1 <= 4096 {
            if let Some(bytes) = copy_from_user(context, arg, arg1 as usize) {
                // E24 egress cone: the console is the display channel too.
                if kernel_egress::gate(&bytes) {
                    let mut writer = kernel_kit::vga::VgaWriter::new();
                    for &byte in &bytes { writer.write_byte(byte); }
                    frame.rax = arg1;
                }
            }
        }
    } else if is(number, SYS_WRITE) || is(number, 14) {
        frame.rax = stdout_write(context, &[arg as u8]);
    } else if is(number, SYS_WRITE_BUFFER) {
        if arg1 <= 4096 {
            if let Some(bytes) = copy_from_user(context, arg, arg1 as usize) {
                // E24 egress cone: the entropy gate on the outbound
                // display channel — prose passes, key-shaped data does
                // not, encoded keys fail the structure profile. It holds
                // whether stdout is the console or a terminal's pipe.
                if kernel_egress::gate(&bytes) { frame.rax = stdout_write(context, &bytes); }
            }
        }
    } else if is(number, SYS_OPEN) || is(number, SYS_OPEN_EXISTING) {
        let result = (|| {
            let path = path_at(context, arg)?;
            let fd = context.open_files.iter().position(|entry| entry.0 == 0).ok_or(FsCode::BadHandle)?;
            let fs = ROOT_FS.lock();
            let opened = if is(number, SYS_OPEN_EXISTING) {
                fs.resolve(&path).and_then(|ino| if fs.node(ino).is_some_and(|n| n.is_dir()) { Err(TreeError::IsDir) } else { Ok(ino) })
            } else { fs.open_or_create(&path, kernel_kit::rtc::now()) }
                .and_then(|ino| kernel_kit::storage::ensure_loaded(fs, ino).map(|_| ino));
            let builtin = opened.is_ok_and(|ino| fs.is_builtin(ino));
            ROOT_FS.unlock();
            let ino = opened.map_err(code)?;
            context.open_files[fd] = (ino as u64 + 1, 0);
            if builtin { context.readonly_files |= 1 << fd; } else { context.readonly_files &= !(1 << fd); }
            Ok(fd as u64)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_READ_FILE) || is(number, SYS_WRITE_FILE) || is(number, SYS_TRUNCATE) {
        let result = (|| {
            let (ino, cursor) = descriptor(context, arg).ok_or(FsCode::BadHandle)?;
            if is(number, SYS_READ_FILE) {
                let mut byte = [0u8];
                let fs = ROOT_FS.lock();
                let read = fs.read(ino, cursor, &mut byte);
                ROOT_FS.unlock();
                return match read.map_err(code)? {
                    1 => { context.open_files[arg as usize].1 += 1; Ok(byte[0] as u64) }
                    _ => Ok(ERROR), // EOF preserves the original byte-I/O ABI.
                };
            }
            if context.readonly_files & (1 << arg) != 0 { return Err(FsCode::ReadOnly); }
            let now = kernel_kit::rtc::now();
            let fs = ROOT_FS.lock();
            let result = if is(number, SYS_TRUNCATE) { fs.truncate(ino, 0, now).map(|_| 0) } else {
                // The byte call appends (its original contract).
                let end = fs.node(ino).map_or(0, |n| n.size());
                fs.write(ino, end, &[arg1 as u8], now).map(|_| 1)
            };
            ROOT_FS.unlock();
            let value = result.map_err(code)?;
            if is(number, SYS_TRUNCATE) { context.open_files[arg as usize].1 = 0; }
            Ok(value)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_WRITE_FILE_BUFFER) || is(number, SYS_REPLACE_FILE) {
        // Append (or replace the whole contents) from one buffer.
        let result = (|| {
            if arg2 > FILE_IO_MAX as u64 { return Err(FsCode::FileTooLarge); }
            let (ino, _) = descriptor(context, arg).ok_or(FsCode::BadHandle)?;
            if context.readonly_files & (1 << arg) != 0 { return Err(FsCode::ReadOnly); }
            let input = if arg2 == 0 { Vec::new() } else { copy_from_user(context, arg1, arg2 as usize).ok_or(FsCode::BadBuffer)? };
            let now = kernel_kit::rtc::now();
            let fs = ROOT_FS.lock();
            let result = if is(number, SYS_REPLACE_FILE) { fs.replace(ino, &input, now) } else {
                let end = fs.node(ino).map_or(0, |n| n.size());
                fs.write(ino, end, &input, now).map(|_| ())
            };
            ROOT_FS.unlock();
            result.map_err(code)?;
            if is(number, SYS_REPLACE_FILE) { context.open_files[arg as usize].1 = 0; }
            Ok(arg2)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_FILE_READ) || is(number, SYS_FILE_WRITE) {
        let len = (arg2 as usize).min(FILE_IO_MAX);
        if let Some((ino, cursor)) = descriptor(context, arg) {
            let reading = is(number, SYS_FILE_READ);
            let writable = context.readonly_files & (1 << arg) == 0;
            if !reading && !writable {
                frame.rax = failed(context, TreeError::ReadOnly);
            } else if context.space.as_ref().unwrap().valid_user_range(arg1, len, reading) {
                let fs = ROOT_FS.lock();
                let now = kernel_kit::rtc::now();
                let mut done = 0usize;
                let mut failure = None;
                // Page-sized steps between the file's frames and the caller's pages.
                let mut buffer = alloc::vec![0u8; 4096];
                while done < len {
                    let n = (len - done).min(4096);
                    let at = cursor + done as u64;
                    if reading {
                        match fs.read(ino, at, &mut buffer[..n]) {
                            Ok(0) => break,
                            Ok(got) => { if !copy_to_user(context, arg1 + done as u64, &buffer[..got]) { failure = Some(None); break; } done += got; }
                            Err(e) => { failure = Some(Some(e)); break; }
                        }
                    } else {
                        let Some(bytes) = copy_from_user(context, arg1 + done as u64, n) else { failure = Some(None); break };
                        match fs.write(ino, at, &bytes, now) { Ok(w) => done += w, Err(e) => { failure = Some(Some(e)); break; } }
                    }
                }
                ROOT_FS.unlock();
                context.open_files[arg as usize].1 += done;
                frame.rax = match failure {
                    Some(Some(error)) if done == 0 => failed(context, error),
                    Some(None) if done == 0 => { context.fs_error = FsCode::BadBuffer as u64; ERROR }
                    _ => { context.fs_error = 0; done as u64 }
                };
            } else { context.fs_error = FsCode::BadBuffer as u64; }
        } else { context.fs_error = FsCode::BadHandle as u64; }
    } else if is(number, SYS_SEEK) {
        if let Some((ino, _)) = descriptor(context, arg) {
            let fs = ROOT_FS.lock();
            let size = fs.node(ino).map_or(0, |n| n.size());
            ROOT_FS.unlock();
            let position = arg1.min(size);
            context.open_files[arg as usize].1 = position as usize;
            frame.rax = position;
        }
    } else if is(number, SYS_CLOSE) {
        let result = match context.open_files.get(arg as usize) {
            Some(&(slot, _)) if slot != 0 => {
                context.open_files[arg as usize] = (0, 0);
                context.readonly_files &= !(1 << arg);
                Ok(0)
            }
            _ => Err(FsCode::BadHandle),
        };
        fs_result(context, frame, result);
    } else if is(number, SYS_STAT) {
        match path_at(context, arg) {
            Ok(path) => {
                let fs = ROOT_FS.lock();
                let entry = fs.resolve(&path).map(|ino| dir_entry(fs, ino));
                ROOT_FS.unlock();
                frame.rax = match entry {
                    Ok(entry) => if copy_to_user(context, arg1, as_bytes(&entry)) { context.fs_error = 0; 0 } else { ERROR },
                    Err(e) => failed(context, e),
                };
            }
            Err(error) => context.fs_error = error as u64,
        }
    } else if is(number, SYS_READ_DIR) {
        let capacity = (arg2 as usize).min(4096);
        match path_at(context, arg) {
            Ok(path) => {
                let fs = ROOT_FS.lock();
                let listing = fs.resolve(&path).and_then(|dir| {
                    let children = fs.children(dir)?;
                    let mut bytes = Vec::new();
                    for &child in children.iter().take(capacity) { bytes.extend_from_slice(as_bytes(&dir_entry(fs, child))); }
                    Ok((children.len(), bytes))
                });
                ROOT_FS.unlock();
                frame.rax = match listing {
                    Ok((count, bytes)) => if copy_to_user(context, arg1, &bytes) { context.fs_error = 0; count as u64 } else { ERROR },
                    Err(e) => failed(context, e),
                };
            }
            Err(error) => context.fs_error = error as u64,
        }
    } else if is(number, SYS_FS_INFO) {
        let fs = ROOT_FS.lock();
        let mut info = FsInfo { needed_bytes: fs.blocks * 4096, capacity_bytes: fs.capacity.unwrap_or(0) * 4096,
                                unsaved: fs.unsaved() as u32, ..FsInfo::default() };
        ROOT_FS.unlock();
        if let Some((total, used)) = kernel_kit::storage::info() {
            info.disk = 1; info.disk_bytes = total * 4096; info.saved_bytes = used * 4096;
        }
        if copy_to_user(context, arg, as_bytes(&info)) { frame.rax = 0; }
    } else if is(number, SYS_FS_ERROR) {
        frame.rax = context.fs_error;
    } else if is(number, SYS_FS_STAT) {
        let fs = ROOT_FS.lock();
        let figures = [fs.user_files() as u64, 0, fs.blocks * 4096, fs.capacity.unwrap_or(0) * 4096,
                       fs.unsaved() as u64, 0, fs.loaded_bytes(), kernel_kit::fs::FILE_MAX, fs.revision, fs.saved_revision, 0];
        ROOT_FS.unlock();
        frame.rax = match arg {
            5 => kernel_kit::storage::generation(),
            10 => kernel_kit::storage::available() as u64,
            index if index < figures.len() as u64 => figures[index as usize],
            _ => ERROR,
        };
    } else if is(number, SYS_LIST_DIR) {
        // arg = optional path pointer (0 or an empty string lists the
        // caller's working folder, which is what the original argless
        // callers pass). Folders print with a trailing '/'.
        let result = (|| {
            let path = if arg == 0 { kernel_kit::fs::join(&context.cwd, ".").map_err(code)? } else { path_at(context, arg)? };
            let fs = ROOT_FS.lock();
            let listing = fs.resolve(&path).and_then(|dir| {
                let mut listing = String::new();
                for &child in fs.children(dir)? {
                    let Some(node) = fs.node(child) else { continue };
                    listing.push_str(&node.name);
                    if node.is_dir() { listing.push('/'); }
                    listing.push_str("  ");
                }
                listing.push('\n');
                Ok(listing)
            });
            ROOT_FS.unlock();
            listing.map_err(code)
        })();
        match result {
            Ok(listing) => { stdout_write(context, listing.as_bytes()); fs_result(context, frame, Ok(0)); }
            Err(error) => fs_result(context, frame, Err(error)),
        }
    } else if is(number, SYS_LIST_FILES) {
        // The original flat listing: the root folder only.
        let capacity = (arg1 as usize).min(256);
        let mut records = Vec::new();
        let fs = ROOT_FS.lock();
        if let Ok(children) = fs.children(kernel_kit::fs::ROOT) {
            for &child in children.iter().take(capacity) {
                let Some(node) = fs.node(child) else { continue };
                let mut record = [0u8; FILE_RECORD_BYTES];
                let name = &node.name.as_bytes()[..node.name.len().min(63)];
                record[..name.len()].copy_from_slice(name);
                record[64..68].copy_from_slice(&(node.size().min(u32::MAX as u64) as u32).to_le_bytes());
                let flags = if fs.is_builtin(child) { FILE_BUILTIN } else { 0 } | if node.is_dir() { FILE_DIR } else { 0 };
                record[68..72].copy_from_slice(&flags.to_le_bytes());
                records.extend_from_slice(&record);
            }
        }
        ROOT_FS.unlock();
        if copy_to_user(context, arg, &records) { frame.rax = (records.len() / FILE_RECORD_BYTES) as u64; }
    } else if is(number, SYS_REMOVE) {
        let result = path_at(context, arg);
        let result = result.and_then(|path| {
            let fs = ROOT_FS.lock();
            let removed = fs.remove(&path, |ino| file_open(system, ino));
            ROOT_FS.unlock();
            removed.map(|()| 0).map_err(code)
        });
        let context = system.scheduler.current_task_mut().unwrap();
        fs_result(context, frame, result);
    } else if is(number, SYS_RENAME) {
        let result = (|| {
            let from = path_at(context, arg)?;
            let to = path_at(context, arg1)?;
            let fs = ROOT_FS.lock();
            let moved = fs.rename(&from, &to, kernel_kit::rtc::now());
            ROOT_FS.unlock();
            moved.map(|()| 0).map_err(code)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_MKDIR) {
        // Strict mkdir: every parent must already exist (no mkdir -p).
        let result = (|| {
            let path = path_at(context, arg)?;
            let fs = ROOT_FS.lock();
            let made = fs.mkdir(&path, kernel_kit::rtc::now());
            ROOT_FS.unlock();
            made.map(|_| 0).map_err(code)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_CHDIR) {
        let result = (|| {
            let path = path_at(context, arg)?;
            let fs = ROOT_FS.lock();
            let target = fs.resolve(&path).and_then(|ino| if fs.node(ino).is_some_and(|n| n.is_dir()) { Ok(()) } else { Err(TreeError::NotDir) });
            ROOT_FS.unlock();
            target.map_err(code)?;
            let len = path.len() as u64;
            context.cwd = path;
            Ok(len)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_PWD) {
        // arg = buffer pointer, arg1 = capacity; copies cwd + trailing NUL
        // and returns the byte count written (SYS_ARGS copy-out style).
        let result = (|| {
            let pwd = if context.cwd.is_empty() { String::from("/") } else { context.cwd.clone() };
            let mut bytes = pwd.into_bytes();
            bytes.push(0);
            if arg1 < bytes.len() as u64 || !copy_to_user(context, arg, &bytes) { return Err(FsCode::BadBuffer); }
            Ok(bytes.len() as u64)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_SYNC) {
        let result = kernel_kit::storage::sync().map(|()| 0).map_err(disk_code);
        fs_result(context, frame, result);
    } else if is(number, SYS_CLEAR) {
        // A terminal on the other end of a pipe clears on form feed.
        if context.stdout.is_some() { stdout_write(context, b"\x0c"); }
        else { kernel_kit::vga::VgaWriter::new().clear_screen(); }
        frame.rax = 0;
    } else if is(number, SYS_EXEC) || is(number, SYS_EXEC_ARGS) || is(number, SYS_SPAWN) || is(number, SYS_SPAWN_ARGS) {
        // Copy all input before creating a child or replacing the caller's CR3.
        // E34 taint wall: a TAINTED context cannot spawn or exec —
        // derived content stays data. (This is the exec/spawn half of
        // the gate; PROT_EXEC-equivalent for the atom OS.)
        if !kernel_taint::gate_exec_spawn(pid as u64) {
            frame.rax = ERROR;
        } else {
            let request = (|| {
                let name = program_at(context, arg)?;
                let extra = if is(number, SYS_EXEC_ARGS) || is(number, SYS_SPAWN_ARGS) {
                    if arg2 > MAX_ARG_BYTES as u64 { return Err(()); }
                    if arg2 == 0 { Vec::new() } else { copy_from_user(context, arg1, arg2 as usize).ok_or(())? }
                } else if is(number, SYS_EXEC) && arg1 != 0 {
                    // SYS_EXEC's optional argument string, split like a shell line.
                    kernel_kit::arguments::extra_from_string(&user_string(context, arg1, ARGS_MAX + 1)?)
                } else { Vec::new() };
                let packed = kernel_kit::arguments::pack(&name, &extra)?;
                Ok((name, extra, packed))
            })();
            if let Ok((name, extra, packed)) = request {
                if is(number, SYS_SPAWN) || is(number, SYS_SPAWN_ARGS) {
                    match system.spawn_with_args(pid, &name, &extra) {
                        Ok(child) => {
                            // E34: children inherit taint from their parent.
                            kernel_taint::propagate_taint(pid as u64, child as u64);
                            frame.rax = child as u64;
                        }
                        Err(()) => frame.rax = ERROR,
                    }
                } else if let Ok((space, entry)) = crate::process::load_image(&name, system.kernel_root) {
                    let context = system.scheduler.current_task_mut().unwrap();
                    // The new root keeps the same kernel mappings and stack. Switch
                    // before dropping the old owner, so no live CR3 is reclaimed.
                    unsafe { Cr3::load(space.root); }
                    context.page_table_root = space.root;
                    context.space = Some(space);
                    context.arguments = packed;
                    context.name = name;
                    context.open_files = [(0, 0); 16]; context.readonly_files = 0; context.fs_error = 0;
                    *frame = TrapFrame::new_user(entry, STACK_TOP);
                    unsafe { crate::process::reset_fpu(rsp); }
                }
            }
        }
    } else if is(number, SYS_SPAWN_WITH) {
        if !kernel_taint::gate_exec_spawn(pid as u64) {
            frame.rax = ERROR;
        } else if let Some(bytes) = copy_from_user(context, arg, core::mem::size_of::<SpawnRequest>()) {
            let request = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const SpawnRequest) };
            let stdio = |selector: u64, current: &Option<kernel_kit::pipe::PipeEnd>, read: bool| -> Result<Option<kernel_kit::pipe::PipeEnd>, ()> {
                match selector {
                    STDIO_INHERIT => Ok(current.clone()),
                    STDIO_CONSOLE => Ok(None),
                    handle => match context.pipes.get(handle as usize) {
                        Some((Some(end), _)) if read => Ok(Some(end.clone())),
                        Some((_, Some(end))) if !read => Ok(Some(end.clone())),
                        _ => Err(()),
                    },
                }
            };
            let name = nul_string(&request.path).and_then(|name| if name.contains('/') { kernel_kit::fs::join(&context.cwd, &name).ok() } else { Some(name) });
            if let (Some(name), Some(args), Ok(stdin), Ok(stdout)) = (name, nul_string(&request.args),
                stdio(request.stdin, &context.stdin, true), stdio(request.stdout, &context.stdout, false)) {
                frame.rax = match system.spawn_with(pid, &name, &args, stdin, stdout) {
                    Ok(child) => { kernel_taint::propagate_taint(pid as u64, child as u64); child as u64 }
                    Err(()) => ERROR,
                };
            }
        }
    } else if is(number, SYS_ARGS) {
        // The packed argv: name NUL arg NUL ...
        let len = context.arguments.len();
        if arg1 >= len as u64 && copy_to_user(context, arg, &context.arguments) { frame.rax = len as u64; }
    } else if is(number, SYS_PIPE) {
        if let Some(slot) = context.pipes.iter().position(|p| p.0.is_none() && p.1.is_none()) {
            let (read, write) = kernel_kit::pipe::PipeEnd::pair();
            context.pipes[slot] = (Some(read), Some(write));
            frame.rax = slot as u64;
        }
    } else if is(number, SYS_PIPE_CLOSE) {
        if let Some(pipe) = context.pipes.get_mut(arg as usize) {
            if arg1 == PIPE_BOTH || arg1 == PIPE_READ_END { pipe.0 = None; }
            if arg1 == PIPE_BOTH || arg1 == PIPE_WRITE_END { pipe.1 = None; }
            frame.rax = 0;
        }
    } else if is(number, SYS_PIPE_READ) {
        let len = (arg2 as usize).min(4096);
        if let Some((Some(end), _)) = context.pipes.get(arg as usize) { frame.rax = pipe_read(end, context, arg1, len); }
    } else if is(number, SYS_PIPE_WRITE) {
        let len = (arg2 as usize).min(4096);
        if let Some((_, Some(end))) = context.pipes.get(arg as usize) {
            if let Some(bytes) = copy_from_user(context, arg1, len) {
                frame.rax = match end.write(&bytes) {
                    kernel_kit::pipe::Write::Wrote(count) => count as u64,
                    kernel_kit::pipe::Write::WouldBlock => WOULD_BLOCK,
                    kernel_kit::pipe::Write::Broken => ERROR,
                };
            }
        }
    } else if is(number, SYS_PROCESSES) {
        // A fixed-capacity, single-call snapshot avoids races between queries.
        let bytes = MAX_PROCESSES * core::mem::size_of::<ProcessInfo>();
        if arg1 == MAX_PROCESSES as u64 && context.space.as_ref().unwrap().valid_user_range(arg, bytes, true) {
            let (records, count) = system.scheduler.snapshot();
            let context = system.scheduler.current_task().unwrap();
            let raw = unsafe { core::slice::from_raw_parts(records.as_ptr() as *const u8, bytes) };
            if copy_to_user(context, arg, raw) { frame.rax = count as u64; }
        }
    } else if is(number, SYS_KILL) {
        if system.kill(arg as usize).is_ok() {
            frame.rax = 0; switch = arg == pid as u64;
        }
    } else if is(number, SYS_WAIT) {
        match system.wait(arg as usize) {
            Ok(Some(status)) => frame.rax = status,
            Ok(None) => switch = true,
            Err(()) => {}
        }
    } else if is(number, SYS_SLEEP) {
        context.sleep_until = ticks.saturating_add(arg.min(10000).max(1));
        context.state = TaskState::Blocked; context.wait_for = None;
        frame.rax = 0; switch = true;
    } else if is(number, SYS_GETPID) {
        frame.rax = pid as u64;
    } else if is(number, SYS_IPC_SEND) {
        if let Ok(message) = user_string(context, arg1, 256) {
            // E24 egress cone: IPC is an outbound channel too.
            if !kernel_egress::gate(message.as_bytes()) {
            } else if let Some(target) = system.scheduler.task_mut(arg as usize) {
                if target.state != TaskState::Terminated && target.mailbox.len() < 4 {
                    target.mailbox.push_back(message.into_bytes()); frame.rax = 0;
                }
            }
        }
    } else if is(number, SYS_IPC_RECV) {
        frame.rax = 0;
        if let Some(message) = context.mailbox.pop_front() {
            let phys = context.space.as_ref().unwrap().translate_user(RECV_BASE, true).unwrap();
            unsafe {
                let target = phys_to_virt(phys) as *mut u8;
                core::ptr::write_bytes(target, 0, 4096);
                core::ptr::copy_nonoverlapping(message.as_ptr(), target, message.len());
            }
            frame.rax = RECV_BASE;
        }
    } else if is(number, SYS_DISPLAY_PRESENT) {
        frame.rax = kernel_kit::display::find().is_some() as u64;
    } else if is(number, SYS_DISPLAY_OPEN) {
        if system.display_owner.is_none_or(|owner| owner == pid) {
            let context = system.scheduler.current_task_mut().unwrap();
            let info_ok = context.space.as_ref().unwrap().valid_user_range(arg, core::mem::size_of::<DisplayInfo>(), true);
            if info_ok {
                if let Some(fb) = kernel_kit::display::enable() {
                    let space = context.space.as_mut().unwrap();
                    space.unmap_device(kernel_kit::address_space::FRAMEBUFFER_BASE);
                    if space.map_device(kernel_kit::address_space::FRAMEBUFFER_BASE, fb.phys, kernel_kit::display::bytes(&fb)).is_ok() {
                        let info = DisplayInfo { width: fb.width, height: fb.height, pitch: fb.pitch, bpp: 32 };
                        if copy_to_user(context, arg, as_bytes(&info)) {
                            system.display_owner = Some(pid);
                            kernel_kit::input::clear();
                            frame.rax = kernel_kit::address_space::FRAMEBUFFER_BASE;
                        }
                    } else { kernel_kit::display::disable(); }
                }
            }
        }
    } else if is(number, SYS_DISPLAY_CLOSE) {
        if system.display_owner == Some(pid) {
            system.scheduler.current_task_mut().unwrap().space.as_mut().unwrap()
                .unmap_device(kernel_kit::address_space::FRAMEBUFFER_BASE);
            system.release_display();
            frame.rax = 0;
        }
    } else if is(number, SYS_INPUT_POLL) {
        if system.display_owner == Some(pid) {
            let size = core::mem::size_of::<InputEvent>();
            let capacity = (arg1 as usize).min(256);
            let context = system.scheduler.current_task().unwrap();
            if context.space.as_ref().unwrap().valid_user_range(arg, capacity * size, true) {
                let events = kernel_kit::input::poll(capacity);
                let bytes = unsafe { core::slice::from_raw_parts(events.as_ptr() as *const u8, events.len() * size) };
                if copy_to_user(context, arg, bytes) { frame.rax = events.len() as u64; }
            }
        }
    } else if is(number, SYS_MEMORY_TOTAL) {
        let (pool, flags) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        frame.rax = pool.total_count() as u64;
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(flags);
    } else if is(number, SYS_TIME) {
        frame.rax = kernel_kit::rtc::now();
    } else if is(number, SYS_FREE_FRAMES) {
        let (pool, flags) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        frame.rax = pool.free_count() as u64;
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(flags);
    } else if is(number, SYS_TASK_COUNT) {
        frame.rax = system.scheduler.tasks.iter().flatten().filter(|t| t.state != TaskState::Terminated).count() as u64;
    } else if is(number, SYS_TICKS) {
        frame.rax = system.scheduler.ticks;
    } else if is(number, SYS_SENSE) {
        // E21 shadow-web control surface. Subcommands arrive in arg
        // (rdi) — user-rt's call3(number, arg, arg1, arg2):
        //   0 = status: (trained<<63) | (events<<32) | raised_sites
        //   1 = freeze the normality cone now
        //   2 = foreign budget of pid `arg1` (x 1e6, truncated)
        //   11 = disarm (condemned pids are scheduled again), 12 = armed?
        //   13 = reset the web to pre-learning (the demos replay a clean boot)
        let sub = arg;
        if sub == 0 {
            let (trained, events, raised, _readable) = kernel_sense::status();
            frame.rax = (u64::from(trained) << 63) | ((events & 0x7FFF_FFFF) << 32) | raised as u64;
        } else if sub == 1 {
            kernel_sense::freeze();
            frame.rax = 0;
        } else if sub == 2 {
            frame.rax = (kernel_sense::foreign_budget(arg1) * 1e6) as u64;
        } else if sub == 6 {
            // E31: rhythm status for pid arg1 — (matured<<63 |
            // drifted<<62 | baseline_q<<32 | current_q).
            let (matured, drifted, baseline, current) = kernel_sense::rhythm_status(arg1);
            frame.rax = (u64::from(matured) << 63)
                | (u64::from(drifted) << 62)
                | (baseline << 32)
                | current;
        } else if sub == 7 {
            // E36: seam status for pid arg1 — (bimodal<<63 | hole<<62).
            let (bimodal, hole) = kernel_sense::seam_status(arg1);
            frame.rax = (u64::from(bimodal) << 63) | (u64::from(hole) << 62);
        } else if sub == 8 {
            // E36: judge docket for pid arg1 — (certified<<63 |
            // bimodal<<62 | episodes).
            let (certified, bimodal, episodes) = kernel_sense::judge_status(arg1);
            frame.rax = (u64::from(certified) << 63)
                | (u64::from(bimodal) << 62)
                | (episodes as u64 & 0xFF);
        } else if sub == 9 {
            // E36 diagnostic: chronological gap at index arg2 of pid
            // arg1's rhythm ring (0 when absent).
            frame.rax = kernel_sense::gap_at(arg1, arg2);
        } else if sub == 10 {
            // E36 diagnostic: seam internals for pid arg1 —
            // (latched<<63 | strikes<<32 | foreign budget x1e6).
            frame.rax = kernel_sense::seam_probe(arg1);
        } else if sub == 11 {
            kernel_sense::disarm();
            frame.rax = 0;
        } else if sub == 12 {
            frame.rax = kernel_sense::armed() as u64;
        } else if sub == 13 {
            kernel_sense::reset();
            frame.rax = 0;
        } else if sub == 3 {
            // T3: average cycles per sensor record() call.
            let calls = SENSOR_CALLS.load(Ordering::Relaxed);
            frame.rax = if calls == 0 { 0 } else { SENSOR_CYCLES.load(Ordering::Relaxed) / calls };
        }
    } else if is(number, SYS_KEY) {
        // E22 fail-dead key. sub = arg: 0=init (seed arg1^arg2 pairs),
        // 1=maintain, 2=read word arg1, 3=status. The caller's pid is
        // the keeper; the spider's signal revokes keepership forever.
        let sub = arg;
        if sub == 0 {
            let seed = [arg1 ^ 0xE22_1, arg2 ^ 0xE22_2, arg1.rotate_left(17), arg2.rotate_left(23)];
            frame.rax = u64::from(kernel_key::init(pid as u64, seed));
        } else if sub == 1 {
            frame.rax = u64::from(kernel_key::maintain(pid as u64));
        } else if sub == 2 {
            let words = kernel_key::read(pid as u64);
            let index = (arg1 & 3) as usize;
            frame.rax = words[index];
        } else if sub == 3 {
            let (alive, cause, energy) = kernel_key::status();
            frame.rax = (u64::from(alive) << 63) | ((cause as u64) << 60) | energy;
        }
    } else if is(number, SYS_INSTANT) {
        // E23 instant-key. sub = arg: 0=init (seed arg1), 1=maintain,
        // 2=transform word arg1 (returns ERROR on refusal), 3=status.
        // The key exists for one instruction inside this handler;
        // userspace never sees key material — only its behavior.
        let sub = arg;
        if sub == 0 {
            frame.rax = u64::from(kernel_instant::init(pid as u64, arg1 ^ 0xE23_5EED));
        } else if sub == 1 {
            frame.rax = u64::from(kernel_instant::maintain(pid as u64));
        } else if sub == 2 {
            match kernel_instant::transform(pid as u64, arg1) {
                Some(out) => frame.rax = out,
                None => frame.rax = ERROR,
            }
        } else if sub == 3 {
            let (alive, cause, energy) = kernel_instant::status();
            frame.rax = (u64::from(alive) << 63) | ((cause as u64) << 60) | energy;
        }
    } else if is(number, SYS_KEYLANE) {
        // E25 the key lane — the one owned way out. sub = arg:
        //   0=claim, 1=deposit(word=arg1,tag=arg2),
        //   2=handoff -> word (ERROR if lane empty), 3=log_get(arg1)
        //     -> kind<<32|word, 4=status.
        // Real transits print a marked line to serial: the lane IS
        // the observation point, and its traffic is legal precisely
        // because it goes through here, not through the cone-gated
        // write channels.
        let sub = arg;
        if sub == 0 {
            frame.rax = u64::from(kernel_lane::claim(pid as u64));
        } else if sub == 1 {
            frame.rax = u64::from(kernel_lane::deposit(pid as u64, arg1, arg2));
        } else if sub == 2 {
            match kernel_lane::handoff(pid as u64) {
                kernel_lane::Handoff::Real { word, transit, .. } => {
                    lane_transit_line(transit, word, true);
                    frame.rax = word;
                }
                kernel_lane::Handoff::Honey { word, transit } => {
                    lane_transit_line(transit, word, false);
                    frame.rax = word;
                }
                kernel_lane::Handoff::Nothing => frame.rax = ERROR,
            }
        } else if sub == 3 {
            // A 64-bit word and a kind cannot share one 64-bit return
            // without loss: the word comes back whole; the kind has
            // its own query (sub 5).
            let (_kind, word) = kernel_lane::log_get(arg1);
            frame.rax = word;
        } else if sub == 5 {
            let (kind, _word) = kernel_lane::log_get(arg1);
            frame.rax = kind;
        } else if sub == 4 {
            let (lane, holding, revoked, real, honey) = kernel_lane::status();
            frame.rax = (lane << 48) | (u64::from(holding) << 47)
                | (u64::from(revoked) << 46) | (real << 23) | honey;
        }
    } else if is(number, SYS_TAINT) {
        // E34 taint layer. sub = arg:
        //   0=mark_tainted(pid), 1=gate(pid), 2=promote(target),
        //   3=status(pid), 4=forget(pid), 5=propagate(from,to),
        //   6=set_input_focus (caller self-registers as the input
        //      context — in a real system the keyboard handler at
        //      boot does this; here the first caller at boot wins,
        //      labeled honestly).
        let sub = arg;
        if sub == 0 {
            frame.rax = u64::from(kernel_taint::mark_tainted(arg1));
        } else if sub == 1 {
            frame.rax = u64::from(kernel_taint::gate_exec_spawn(arg1));
        } else if sub == 2 {
            frame.rax = u64::from(kernel_taint::promote(pid as u64, arg1));
        } else if sub == 3 {
            frame.rax = match kernel_taint::status(arg1) {
                kernel_taint::TaintState::Clean => 0,
                kernel_taint::TaintState::Tainted => 1,
                kernel_taint::TaintState::Promoted => 2,
            };
        } else if sub == 4 {
            kernel_taint::forget(arg1);
            frame.rax = 0;
        } else if sub == 5 {
            frame.rax = u64::from(kernel_taint::propagate_taint(arg1, arg2));
        } else if sub == 6 {
            kernel_taint::set_input_focus(pid as u64);
            frame.rax = 0;
        }
    } else if is(number, SYS_CRYPT) {
        // E35 crypt layer — encryption at rest with temporal scramble.
        // sub = arg:
        //   0=encrypt_word(word=arg1) -> ct; the temporal handle lands
        //     in the single handle slot (read it with sub 2 before any
        //     other crypt call — v1 single-slot, labeled),
        //   1=decrypt_word(ct=arg1, handle=arg2) -> word (noise if the
        //     handle is wrong or the key is dead),
        //   2=take_handle -> the last encryption's temporal handle,
        //   3=status -> (ready<<63) | nonce_counter,
        //   4=destroy (the intrusion cascade fires this too).
        // The field tick is the scheduler's own clock: ciphertext is a
        // function of WHEN it was written, which a spatial DRAM
        // snapshot cannot replay — the 4th dimension as a gate. v1
        // scope is anti-snapshot, not inter-process secrecy (that is
        // the lane's doctrine).
        let sub = arg;
        if sub == 0 {
            let (ct, nonce) = kernel_crypt::encrypt_word(arg1, system.scheduler.ticks);
            kernel_crypt::stash_handle(nonce);
            frame.rax = ct;
        } else if sub == 1 {
            frame.rax = kernel_crypt::decrypt_word(arg1, arg2);
        } else if sub == 2 {
            frame.rax = kernel_crypt::take_handle();
        } else if sub == 3 {
            frame.rax = kernel_crypt::status();
        } else if sub == 4 {
            kernel_crypt::destroy();
            frame.rax = 0;
        }
    } else if is(number, SYS_LIGHTCONE) {
        if let Some(space) = context.space.as_ref() {
            if let Some(phys) = space.translate_user(RECV_BASE, true) {
                let target = phys_to_virt(phys) as *mut u8;
                unsafe { core::ptr::write_bytes(target, 0, 4096); }
                let page = unsafe { core::slice::from_raw_parts_mut(target, 4096) };
                frame.rax = kernel_net::lightcone::read(arg, arg1, page).map(|n| n as u64).unwrap_or(ERROR);
            }
        }
    } else if is(number, SYS_NET) {
        // E37 the network ingress. sub = arg:
        //   0=status, 1=recv (RECV_BASE; MARKS THE CALLER TAINTED —
        //   reading network data is handling foreign content),
        //   2=send(bytes=arg1,len=arg2) through the E24 egress cone,
        //   out as an ICMP echo request, 3=heartbeat now,
        //   4=ARP bootstrap probe.
        let sub = arg;
        if sub == 0 {
            frame.rax = kernel_net::status();
        } else if sub == 1 {
            if let Some(space) = context.space.as_ref() {
                if let Some(phys) = space.translate_user(RECV_BASE, true) {
                    let target = phys_to_virt(phys) as *mut u8;
                    unsafe { core::ptr::write_bytes(target, 0, 4096); }
                    let page = unsafe { core::slice::from_raw_parts_mut(target, 4096) };
                    match kernel_net::recv_into(page) {
                        Some(len) => {
                            // The ingress ceremony: this task now carries
                            // foreign content. Derived stays data.
                            kernel_taint::mark_tainted(pid as u64);
                            frame.rax = len as u64;
                        }
                        None => frame.rax = ERROR,
                    }
                }
            }
        } else if sub == 2 {
            if arg2 <= 1500 && context.space.as_ref().unwrap().valid_user_range(arg1, arg2 as usize, false) {
                let bytes = unsafe { core::slice::from_raw_parts(arg1 as *const u8, arg2 as usize) };
                let transmit = &mut |out: &[u8]| kernel_net::send_raw(out);
                frame.rax = match kernel_net::send_probed(bytes, transmit) {
                    Ok(()) => 1,
                    Err(kernel_net::SendCause::Cone) => 2,
                    Err(kernel_net::SendCause::NoRoute) => 3,
                    Err(kernel_net::SendCause::Driver) => 4,
                };
            }
        } else if sub == 3 {
            kernel_net::heartbeat();
            frame.rax = 0;
        } else if sub == 4 {
            frame.rax = kernel_net::arp_probe();
        } else if sub == 5 {
            // E38: bind a listener port (the ceremony: a place you
            // chose to receive foreign material).
            frame.rax = u64::from(kernel_net::bind(arg1 as u16));
        } else if sub == 6 {
            // E38: udp_send. Buffer = [4B dst ip BE | 2B dst port BE
            // | 2B src port BE | data...]. The cone judges the DATA
            // (never the addressing header); taint applies on recv.
            if arg2 >= 8 && arg2 <= 8 + 512
                && context.space.as_ref().unwrap().valid_user_range(arg1, arg2 as usize, false) {
                let bytes = unsafe { core::slice::from_raw_parts(arg1 as *const u8, arg2 as usize) };
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&bytes[0..4]);
                let dst_port = u16::from_be_bytes([bytes[4], bytes[5]]);
                let src_port = u16::from_be_bytes([bytes[6], bytes[7]]);
                let transmit = &mut |out: &[u8]| kernel_net::send_raw(out);
                frame.rax = match kernel_net::udp_send(ip, dst_port, src_port, &bytes[8..], transmit) {
                    Ok(()) => 1,
                    Err(kernel_net::UdpCause::Cone) => 2,
                    Err(kernel_net::UdpCause::NoRoute) => 3,
                    Err(kernel_net::UdpCause::Driver) => 4,
                };
            }
        } else if sub == 7 {
            // E38: udp_recv(port) -> datagram at RECV_BASE; the
            // caller is TAINTED (same ingress ceremony as raw recv).
            if let Some(space) = context.space.as_ref() {
                if let Some(phys) = space.translate_user(RECV_BASE, true) {
                    let target = phys_to_virt(phys) as *mut u8;
                    unsafe { core::ptr::write_bytes(target, 0, 4096); }
                    let page = unsafe { core::slice::from_raw_parts_mut(target, 4096) };
                    match kernel_net::udp_recv_into(arg1 as u16, page) {
                        Some(len) => {
                            kernel_taint::mark_tainted(pid as u64);
                            frame.rax = len as u64;
                        }
                        None => frame.rax = ERROR,
                    }
                }
            }
        } else if sub == 8 {
            // E38: last_sender(port) -> (ip<<16) | port, for replies.
            frame.rax = match kernel_net::udp_last_sender(arg1 as u16) {
                Some((ip, port)) => (u32::from_be_bytes(ip) as u64) << 16 | port as u64,
                None => ERROR,
            };
        }
    } else if is(number, SYS_VGA) {
        // E40: the visible cursor. sub=arg: 0=move by arg cells
        // (negative = left, line-local clamp), 1=read (col | row<<8).
        let sub = arg;
        if sub == 0 {
            kernel_kit::vga::move_cursor(arg1 as i64);
            frame.rax = 0;
        } else if sub == 1 {
            let (col, row) = kernel_kit::vga::cursor_position();
            frame.rax = (col as u64) | ((row as u64) << 8);
        }
    } else if is(number, SYS_REBOOT) {
        if kernel_kit::storage::sync().is_ok() {
            let status = kernel_kit::io::Port::new(0x64);
            for _ in 0..100000 { if status.read() & 2 == 0 { break; } core::hint::spin_loop(); }
            kernel_kit::io::Port::new(0x64).write(0xfe);
        } else if let Some(context) = system.scheduler.current_task_mut() { context.fs_error = FsCode::Io as u64; }
    }
    system.scheduler.collect();
    if switch { system.scheduler.switch_context(rsp) } else { rsp }
}
