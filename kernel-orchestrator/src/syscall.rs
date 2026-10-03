use alloc::{string::String, vec::Vec};
use kernel_kit::address_space::{STACK_TOP, RECV_BASE};
use kernel_kit::context::{Context, TaskState};
use kernel_kit::paging::{Cr3, phys_to_virt};
use kernel_kit::trap::TrapFrame;
use crate::system::System;

pub use crate::abi::*;

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

use kernel_kit::fs::{Fs, FsError, Ino, ROOT_FS};

fn fs_code(error: FsError) -> u64 {
    match error {
        FsError::NotFound => ERR_NOT_FOUND, FsError::Exists => ERR_EXISTS, FsError::NotDir => ERR_NOT_DIR,
        FsError::IsDir => ERR_IS_DIR, FsError::NotEmpty => ERR_NOT_EMPTY, FsError::Invalid => ERR_INVALID,
        FsError::ReadOnly => ERR_READ_ONLY, FsError::Busy => ERR_BUSY, FsError::NoSpace => ERR_NO_SPACE,
        FsError::Io => ERR_IO,
    }
}
fn disk_code(error: kernel_kit::virtio_blk::DiskError) -> u64 {
    use kernel_kit::virtio_blk::DiskError;
    match error { DiskError::Full => ERR_NO_SPACE, DiskError::Missing => ERR_NOT_FOUND, _ => ERR_IO }
}
fn result_code(result: Result<(), FsError>) -> u64 { result.map_or_else(fs_code, |_| 0) }

fn path_arg(context: &Context, address: u64) -> Result<String, ()> { user_string(context, address, kernel_kit::fs::PATH_MAX + 1) }

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
            kernel_kit::pipe::Write::Broken => ERROR,
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
    frame.rax = ERROR;
    let mut switch = false;
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
        } else { kernel_kit::input::console_byte().unwrap_or(0) as u64 };
    } else if is(number, SYS_STDIN_READ) {
        let len = (arg1 as usize).min(4096);
        frame.rax = if let Some(end) = &context.stdin { pipe_read(end, context, arg, len) }
        else if system.display_owner.is_some() { WOULD_BLOCK }
        else {
            let mut bytes = Vec::new();
            let mut ended = false;
            while bytes.len() < len {
                match kernel_kit::input::console_byte() {
                    Some(27) => { ended = true; break; }
                    Some(byte) => bytes.push(byte),
                    None => break,
                }
            }
            if bytes.is_empty() { if ended { 0 } else { WOULD_BLOCK } }
            else if copy_to_user(context, arg, &bytes) { bytes.len() as u64 } else { ERROR }
        };
    } else if is(number, SYS_CONSOLE_WRITE) {
        if arg1 <= 4096 && context.space.as_ref().unwrap().valid_user_range(arg, arg1 as usize, false) {
            if let Some(bytes) = copy_from_user(context, arg, arg1 as usize) {
                let mut writer = kernel_kit::vga::VgaWriter::new();
                for &byte in &bytes { writer.write_byte(byte); }
                frame.rax = arg1;
            }
        }
    } else if is(number, SYS_WRITE) || is(number, 14) {
        frame.rax = stdout_write(context, &[arg as u8]);
    } else if is(number, SYS_WRITE_BUFFER) {
        if arg1 <= 4096 && context.space.as_ref().unwrap().valid_user_range(arg, arg1 as usize, false) {
            if let Some(bytes) = copy_from_user(context, arg, arg1 as usize) { frame.rax = stdout_write(context, &bytes); }
        }
    } else if is(number, SYS_OPEN) {
        if let Ok(path) = path_arg(context, arg) {
            if let Some(fd) = context.open_files.iter().position(|entry| entry.0 == 0) {
                let fs = ROOT_FS.lock();
                let opened = fs.open_or_create(&path, kernel_kit::rtc::now())
                    .and_then(|ino| kernel_kit::storage::ensure_loaded(fs, ino).map(|_| ino));
                if let Ok(ino) = opened {
                    context.open_files[fd] = (ino as u64 + 1, 0);
                    if fs.is_builtin(ino) { context.readonly_files |= 1 << fd; } else { context.readonly_files &= !(1 << fd); }
                    frame.rax = fd as u64;
                }
                ROOT_FS.unlock();
            }
        }
    } else if is(number, SYS_READ_FILE) || is(number, SYS_WRITE_FILE) || is(number, SYS_TRUNCATE) {
        if let Some((ino, cursor)) = descriptor(context, arg) {
            let fs = ROOT_FS.lock();
            if is(number, SYS_READ_FILE) {
                let mut byte = [0u8];
                if fs.read(ino, cursor, &mut byte) == Ok(1) { context.open_files[arg as usize].1 += 1; frame.rax = byte[0] as u64; }
            } else if context.readonly_files & (1 << arg) == 0 {
                let now = kernel_kit::rtc::now();
                if is(number, SYS_TRUNCATE) {
                    if fs.truncate(ino, 0, now).is_ok() { context.open_files[arg as usize].1 = 0; frame.rax = 0; }
                } else {
                    // The byte call appends (its original contract).
                    let end = fs.node(ino).map_or(0, |n| n.size());
                    if fs.write(ino, end, &[arg1 as u8], now) == Ok(1) { frame.rax = 1; }
                }
            }
            ROOT_FS.unlock();
        }
    } else if is(number, SYS_FILE_READ) || is(number, SYS_FILE_WRITE) {
        let len = (frame.rdx as usize).min(FILE_IO_MAX);
        if let Some((ino, cursor)) = descriptor(context, arg) {
            let reading = is(number, SYS_FILE_READ);
            let writable = context.readonly_files & (1 << arg) == 0;
            if context.space.as_ref().unwrap().valid_user_range(arg1, len, reading) && (reading || writable) {
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
                            Ok(got) => { if !copy_to_user(context, arg1 + done as u64, &buffer[..got]) { failure = Some(ERROR); break; } done += got; }
                            Err(e) => { failure = Some(fs_code(e)); break; }
                        }
                    } else {
                        let Some(bytes) = copy_from_user(context, arg1 + done as u64, n) else { failure = Some(ERROR); break };
                        match fs.write(ino, at, &bytes, now) { Ok(w) => done += w, Err(e) => { failure = Some(fs_code(e)); break; } }
                    }
                }
                ROOT_FS.unlock();
                context.open_files[arg as usize].1 += done;
                frame.rax = match failure { Some(code) if done == 0 => code, _ => done as u64 };
            }
        }
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
        if arg < 16 { context.open_files[arg as usize] = (0, 0); context.readonly_files &= !(1 << arg); frame.rax = 0; }
    } else if is(number, SYS_STAT) {
        if let Ok(path) = path_arg(context, arg) {
            let fs = ROOT_FS.lock();
            let entry = fs.resolve(&path).map(|ino| dir_entry(fs, ino));
            ROOT_FS.unlock();
            frame.rax = match entry {
                Ok(entry) => if copy_to_user(context, arg1, as_bytes(&entry)) { 0 } else { ERROR },
                Err(e) => fs_code(e),
            };
        }
    } else if is(number, SYS_MKDIR) {
        if let Ok(path) = path_arg(context, arg) {
            let fs = ROOT_FS.lock();
            frame.rax = result_code(fs.mkdir(&path, kernel_kit::rtc::now()).map(|_| ()));
            ROOT_FS.unlock();
        }
    } else if is(number, SYS_READ_DIR) {
        let capacity = (frame.rdx as usize).min(4096);
        if let Ok(path) = path_arg(context, arg) {
            let fs = ROOT_FS.lock();
            let listing = fs.resolve(&path).and_then(|dir| {
                let children = fs.children(dir)?;
                let mut bytes = Vec::new();
                for &child in children.iter().take(capacity) { bytes.extend_from_slice(as_bytes(&dir_entry(fs, child))); }
                Ok((children.len(), bytes))
            });
            ROOT_FS.unlock();
            frame.rax = match listing {
                Ok((count, bytes)) => if copy_to_user(context, arg1, &bytes) { count as u64 } else { ERROR },
                Err(e) => fs_code(e),
            };
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
    } else if is(number, SYS_LIST_DIR) {
        let fs = ROOT_FS.lock();
        let mut listing = String::new();
        if let Ok(children) = fs.children(kernel_kit::fs::ROOT) {
            for &child in children {
                let Some(node) = fs.node(child) else { continue };
                listing.push_str(&node.name);
                if node.is_dir() { listing.push('/'); }
                listing.push_str("  ");
            }
        }
        listing.push('\n');
        ROOT_FS.unlock();
        stdout_write(context, listing.as_bytes()); frame.rax = 0;
    } else if is(number, SYS_CLEAR) {
        // A terminal on the other end of a pipe clears on form feed.
        if context.stdout.is_some() { stdout_write(context, b"\x0c"); }
        else { kernel_kit::vga::VgaWriter::new().clear_screen(); }
        frame.rax = 0;
    } else if is(number, SYS_ARGS) {
        let args = context.args.clone();
        let count = args.len().min(arg1 as usize);
        if copy_to_user(context, arg, &args.as_bytes()[..count]) { frame.rax = args.len() as u64; }
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
        let len = (frame.rdx as usize).min(4096);
        if let Some((Some(end), _)) = context.pipes.get(arg as usize) { frame.rax = pipe_read(end, context, arg1, len); }
    } else if is(number, SYS_PIPE_WRITE) {
        let len = (frame.rdx as usize).min(4096);
        if let Some((_, Some(end))) = context.pipes.get(arg as usize) {
            if let Some(bytes) = copy_from_user(context, arg1, len) {
                frame.rax = match end.write(&bytes) {
                    kernel_kit::pipe::Write::Wrote(count) => count as u64,
                    kernel_kit::pipe::Write::WouldBlock => WOULD_BLOCK,
                    kernel_kit::pipe::Write::Broken => ERROR,
                };
            }
        }
    } else if is(number, SYS_SPAWN_WITH) {
        let size = core::mem::size_of::<SpawnRequest>();
        if let Some(bytes) = copy_from_user(context, arg, size) {
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
            if let (Some(name), Some(args), Ok(stdin), Ok(stdout)) = (nul_string(&request.path), nul_string(&request.args),
                stdio(request.stdin, &context.stdin, true), stdio(request.stdout, &context.stdout, false)) {
                frame.rax = system.spawn_with(pid, &name, &args, stdin, stdout).map(|n| n as u64).unwrap_or(ERROR);
            }
        }
    } else if is(number, SYS_EXEC) {
        let args = if arg1 == 0 { Ok(String::new()) } else { user_string(context, arg1, ARGS_MAX + 1) };
        if let (Ok(name), Ok(args)) = (path_arg(context, arg), args) {
            if let Ok((space, entry)) = crate::process::load_image(&name, system.kernel_root) {
                let context = system.scheduler.current_task_mut().unwrap();
                // The new root keeps the same kernel mappings and stack. Switch
                // before dropping the old owner, so no live CR3 is reclaimed.
                unsafe { Cr3::load(space.root); }
                context.page_table_root = space.root;
                context.space = Some(space);
                context.name = name;
                context.args = args;
                context.open_files = [(0, 0); 16]; context.readonly_files = 0;
                *frame = TrapFrame::new_user(entry, STACK_TOP);
                unsafe { crate::process::reset_fpu(rsp); }
            }
        }
    } else if is(number, SYS_SPAWN) {
        if let Ok(name) = path_arg(context, arg) {
            frame.rax = system.spawn_program(pid, &name).map(|n| n as u64).unwrap_or(ERROR);
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
            if let Some(target) = system.scheduler.task_mut(arg as usize) {
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
    } else if is(number, SYS_REMOVE) {
        if let Ok(path) = path_arg(context, arg) {
            let fs = ROOT_FS.lock();
            frame.rax = result_code(fs.remove(&path, |ino| file_open(system, ino)));
            ROOT_FS.unlock();
        }
    } else if is(number, SYS_RENAME) {
        if let (Ok(from), Ok(to)) = (path_arg(context, arg), path_arg(context, arg1)) {
            let fs = ROOT_FS.lock();
            frame.rax = result_code(fs.rename(&from, &to, kernel_kit::rtc::now()));
            ROOT_FS.unlock();
        }
    } else if is(number, SYS_PROCESSES) {
        let capacity = (arg1 as usize).min(crate::scheduler::MAX_TASKS);
        let mut records = Vec::new();
        for task in system.scheduler.tasks.iter().flatten().take(capacity) {
            let mut record = [0u8; PROCESS_RECORD_BYTES];
            record[0..4].copy_from_slice(&(task.id as u32).to_le_bytes());
            record[4..8].copy_from_slice(&(task.parent as u32).to_le_bytes());
            record[8] = match task.state {
                TaskState::Ready => STATE_READY,
                TaskState::Running => STATE_RUNNING,
                TaskState::Blocked | TaskState::Trapped => STATE_BLOCKED,
                TaskState::Terminated => STATE_EXITED,
            };
            let name = &task.name.as_bytes()[..task.name.len().min(PROCESS_NAME_BYTES)];
            record[9] = name.len() as u8;
            record[16..16 + name.len()].copy_from_slice(name);
            records.extend_from_slice(&record);
        }
        let context = system.scheduler.current_task().unwrap();
        if copy_to_user(context, arg, &records) { frame.rax = (records.len() / PROCESS_RECORD_BYTES) as u64; }
    } else if is(number, SYS_KILL) {
        if system.kill(arg as usize).is_ok() {
            frame.rax = 0;
            if arg as usize == pid { switch = true; }
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
                        let bytes = unsafe { core::slice::from_raw_parts(&info as *const DisplayInfo as *const u8, core::mem::size_of::<DisplayInfo>()) };
                        if copy_to_user(context, arg, bytes) {
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
        let context = system.scheduler.current_task().unwrap();
        if copy_to_user(context, arg, &records) { frame.rax = (records.len() / FILE_RECORD_BYTES) as u64; }
    } else if is(number, SYS_MEMORY_TOTAL) {
        let (pool, flags) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        frame.rax = pool.total_count() as u64;
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(flags);
    } else if is(number, SYS_TIME) {
        frame.rax = kernel_kit::rtc::now();
    } else if is(number, SYS_SYNC) {
        frame.rax = kernel_kit::storage::sync().map_or_else(disk_code, |_| 0);
    } else if is(number, SYS_FREE_FRAMES) {
        let (pool, flags) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        frame.rax = pool.free_count() as u64;
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(flags);
    } else if is(number, SYS_TASK_COUNT) {
        frame.rax = system.scheduler.tasks.iter().flatten().filter(|t| t.state != TaskState::Terminated).count() as u64;
    } else if is(number, SYS_TICKS) {
        frame.rax = system.scheduler.ticks;
    } else if is(number, SYS_REBOOT) {
        if kernel_kit::storage::sync().is_ok() {
            let status = kernel_kit::io::Port::new(0x64);
            for _ in 0..100000 { if status.read() & 2 == 0 { break; } core::hint::spin_loop(); }
            kernel_kit::io::Port::new(0x64).write(0xfe);
        }
    }
    system.scheduler.collect();
    if switch { system.scheduler.switch_context(rsp) } else { rsp }
}
