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

pub fn builtin(name: &str) -> bool { matches!(name, "shell.elf" | "daemon.elf" | "worker.elf" | "fault.elf") }

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
        loop {
            let (buffer, flags) = kernel_kit::io::KEYBOARD_BUFFER.lock();
            let code = buffer.pop();
            kernel_kit::io::KEYBOARD_BUFFER.unlock(flags);
            match code {
                Some(code) => if let Some(byte) = kernel_kit::keyboard::scancode_to_ascii(code) { frame.rax = byte as u64; break; },
                None => { frame.rax = 0; break; }
            }
        }
    } else if is(number, SYS_WRITE) || is(number, 14) {
        kernel_kit::vga::VgaWriter::new().write_byte(arg as u8); frame.rax = 1;
    } else if is(number, SYS_WRITE_BUFFER) {
        if arg1 <= 4096 && context.space.as_ref().unwrap().valid_user_range(arg, arg1 as usize, false) {
            let bytes = unsafe { core::slice::from_raw_parts(arg as *const u8, arg1 as usize) };
            let mut writer = kernel_kit::vga::VgaWriter::new();
            for &byte in bytes { writer.write_byte(byte); }
            frame.rax = arg1;
        }
    } else if is(number, SYS_OPEN) {
        if let Ok(name) = user_string(context, arg, 64) {
            if let Some(fd) = context.open_files.iter().position(|entry| entry.0 == 0) {
                let fs = kernel_kit::fs::ROOT_FS.lock();
                if let Some(data) = fs.get_or_create_file(&name) {
                    context.open_files[fd] = (data as u64, 0);
                    if builtin(&name) { context.readonly_files |= 1 << fd; } else { context.readonly_files &= !(1 << fd); }
                    frame.rax = fd as u64;
                }
                kernel_kit::fs::ROOT_FS.unlock();
            }
        }
    } else if is(number, SYS_READ_FILE) || is(number, SYS_WRITE_FILE) || is(number, SYS_TRUNCATE) {
        if arg < 16 && context.open_files[arg as usize].0 != 0 {
            let (pointer, cursor) = context.open_files[arg as usize];
            let data = unsafe { &mut *(pointer as *mut Vec<u8>) };
            if is(number, SYS_READ_FILE) {
                if let Some(&byte) = data.get(cursor) { context.open_files[arg as usize].1 += 1; frame.rax = byte as u64; }
            } else if context.readonly_files & (1 << arg) == 0 {
                if is(number, SYS_TRUNCATE) { data.clear(); context.open_files[arg as usize].1 = 0; frame.rax = 0; }
                else if data.len() < 65536 { data.push(arg1 as u8); frame.rax = 1; }
            }
        }
    } else if is(number, SYS_CLOSE) {
        if arg < 16 { context.open_files[arg as usize] = (0, 0); context.readonly_files &= !(1 << arg); frame.rax = 0; }
    } else if is(number, SYS_LIST_DIR) {
        let fs = kernel_kit::fs::ROOT_FS.lock();
        let mut writer = kernel_kit::vga::VgaWriter::new();
        if let kernel_kit::fs::AtomNode::Directory(children) = fs {
            for (name, _) in children { writer.write_string(name); writer.write_string("  "); }
        }
        writer.write_string("\n"); kernel_kit::fs::ROOT_FS.unlock(); frame.rax = 0;
    } else if is(number, SYS_CLEAR) {
        kernel_kit::vga::VgaWriter::new().clear_screen(); frame.rax = 0;
    } else if is(number, SYS_EXEC) {
        if let Ok(name) = user_string(context, arg, 64) {
            if let Ok((space, entry)) = crate::process::load_image(&name, system.kernel_root) {
                let context = system.scheduler.current_task_mut().unwrap();
                // The new root keeps the same kernel mappings and stack. Switch
                // before dropping the old owner, so no live CR3 is reclaimed.
                unsafe { Cr3::load(space.root); }
                context.page_table_root = space.root;
                context.space = Some(space);
                context.open_files = [(0, 0); 16]; context.readonly_files = 0;
                *frame = TrapFrame::new_user(entry, STACK_TOP);
                unsafe { crate::process::reset_fpu(rsp); }
            }
        }
    } else if is(number, SYS_SPAWN) {
        if let Ok(name) = user_string(context, arg, 64) {
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
    } else if is(number, SYS_SYNC) {
        if kernel_kit::storage::sync().is_ok() { frame.rax = 0; }
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
