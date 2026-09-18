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

fn fs_result(context: &mut Context, frame: &mut TrapFrame, result: Result<u64, FsError>) {
    match result {
        Ok(value) => { frame.rax = value; context.fs_error = 0; }
        Err(error) => { frame.rax = ERROR; context.fs_error = error as u64; }
    }
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
        let (serial, flags) = kernel_kit::serial::SERIAL1.lock();
        let byte = serial.receive();
        kernel_kit::serial::SERIAL1.unlock(flags);
        if let Some(byte) = byte {
            frame.rax = match byte { b'\r' => b'\n', 127 => 8, b => b } as u64;
        } else { loop {
            let (buffer, flags) = kernel_kit::io::KEYBOARD_BUFFER.lock();
            let code = buffer.pop();
            kernel_kit::io::KEYBOARD_BUFFER.unlock(flags);
            match code {
                Some(code) => if let Some(byte) = kernel_kit::keyboard::scancode_to_ascii(code) { frame.rax = byte as u64; break; },
                None => { frame.rax = 0; break; }
            }
        }
        }
    } else if is(number, SYS_WRITE) || is(number, 14) {
        kernel_kit::vga::VgaWriter::new().write_byte(arg as u8); frame.rax = 1;
    } else if is(number, SYS_WRITE_BUFFER) {
        if arg1 <= 4096 && context.space.as_ref().unwrap().valid_user_range(arg, arg1 as usize, false) {
            let bytes = unsafe { core::slice::from_raw_parts(arg as *const u8, arg1 as usize) };
            // E24 egress cone: the entropy gate on the outbound
            // display channel — prose passes, key-shaped data does
            // not, encoded keys fail the structure profile.
            if !kernel_egress::gate(bytes) {
                frame.rax = ERROR;
            } else {
            let mut writer = kernel_kit::vga::VgaWriter::new();
            for &byte in bytes { writer.write_byte(byte); }
            frame.rax = arg1;
            }
        }
    } else if is(number, SYS_OPEN) || is(number, SYS_OPEN_EXISTING) {
        let result = (|| {
            let name = user_string(context, arg, 64).map_err(|_| FsError::BadBuffer)?;
            let fd = context.open_files.iter().position(|entry| entry.is_none()).ok_or(FsError::BadHandle)?;
            let fs = kernel_kit::fs::ROOT_FS.lock();
            let file = if is(number, SYS_OPEN_EXISTING) { fs.open_existing(&name) } else { fs.open(&name) };
            kernel_kit::fs::ROOT_FS.unlock();
            context.open_files[fd] = Some(kernel_kit::fs::OpenFile { file: file?, cursor: 0 });
            Ok(fd as u64)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_READ_FILE) || is(number, SYS_WRITE_FILE) || is(number, SYS_TRUNCATE) {
        let result = (|| {
            let opened = context.open_files.get_mut(arg as usize).and_then(Option::as_mut).ok_or(FsError::BadHandle)?;
            if is(number, SYS_READ_FILE) {
                if let Some(byte) = opened.file.byte(opened.cursor) { opened.cursor += 1; Ok(byte as u64) }
                else { Ok(ERROR) } // EOF preserves the original byte-I/O ABI.
            } else {
                let fs = kernel_kit::fs::ROOT_FS.lock();
                let result = if is(number, SYS_TRUNCATE) { fs.replace(&opened.file, &[]) }
                             else { fs.append(&opened.file, &[arg1 as u8]) };
                kernel_kit::fs::ROOT_FS.unlock();
                result?;
                if is(number, SYS_TRUNCATE) { opened.cursor = 0; Ok(0) } else { Ok(1) }
            }
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_WRITE_FILE_BUFFER) || is(number, SYS_REPLACE_FILE) {
        let result = (|| {
            if arg2 > kernel_kit::fs::MAX_FILE_BYTES as u64 { return Err(FsError::FileTooLarge); }
            if arg2 != 0 && !context.space.as_ref().unwrap().valid_user_range(arg1, arg2 as usize, false) {
                return Err(FsError::BadBuffer);
            }
            let input = if arg2 == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(arg1 as *const u8, arg2 as usize) } };
            let opened = context.open_files.get_mut(arg as usize).and_then(Option::as_mut).ok_or(FsError::BadHandle)?;
            let fs = kernel_kit::fs::ROOT_FS.lock();
            let result = if is(number, SYS_REPLACE_FILE) { fs.replace(&opened.file, input) } else { fs.append(&opened.file, input) };
            kernel_kit::fs::ROOT_FS.unlock();
            result?;
            if is(number, SYS_REPLACE_FILE) { opened.cursor = 0; }
            Ok(arg2)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_REMOVE) || is(number, SYS_RENAME) {
        let result = (|| {
            let old = user_string(context, arg, 64).map_err(|_| FsError::BadBuffer)?;
            let new = if is(number, SYS_RENAME) { Some(user_string(context, arg1, 64).map_err(|_| FsError::BadBuffer)?) } else { None };
            let fs = kernel_kit::fs::ROOT_FS.lock();
            let result = if let Some(new) = new { fs.rename(&old, &new) } else { fs.remove(&old) };
            kernel_kit::fs::ROOT_FS.unlock();
            result.map(|()| 0)
        })();
        fs_result(context, frame, result);
    } else if is(number, SYS_CLOSE) {
        let result = if let Some(slot) = context.open_files.get_mut(arg as usize) {
            if slot.take().is_some() { Ok(0) } else { Err(FsError::BadHandle) }
        } else { Err(FsError::BadHandle) };
        fs_result(context, frame, result);
    } else if is(number, SYS_FS_ERROR) {
        frame.rax = context.fs_error;
    } else if is(number, SYS_FS_STAT) {
        let usage = kernel_kit::fs::ROOT_FS.lock().usage();
        kernel_kit::fs::ROOT_FS.unlock();
        frame.rax = match arg {
            0 => usage.files as u64, 1 => kernel_kit::fs::MAX_FILES as u64,
            2 => usage.serialized_bytes as u64, 3 => kernel_kit::fs::MAX_SNAPSHOT_BYTES as u64,
            4 => usage.dirty as u64, 5 => usage.generation,
            6 => usage.live_bytes as u64, 7 => kernel_kit::fs::MAX_LIVE_BYTES as u64,
            8 => usage.revision, 9 => usage.saved_revision,
            10 => kernel_kit::storage::available() as u64, _ => ERROR,
        };
    } else if is(number, SYS_LIST_DIR) {
        let fs = kernel_kit::fs::ROOT_FS.lock();
        let mut writer = kernel_kit::vga::VgaWriter::new();
        for (name, _) in fs.entries() { writer.write_string(name); writer.write_string("  "); }
        writer.write_string("\n"); kernel_kit::fs::ROOT_FS.unlock(); frame.rax = 0;
    } else if is(number, SYS_CLEAR) {
        kernel_kit::vga::VgaWriter::new().clear_screen(); frame.rax = 0;
    } else if is(number, SYS_EXEC) || is(number, SYS_EXEC_ARGS) || is(number, SYS_SPAWN) || is(number, SYS_SPAWN_ARGS) {
        // Copy all input before creating a child or replacing the caller's CR3.
        // E34 taint wall: a TAINTED context cannot spawn or exec —
        // derived content stays data. (This is the exec/spawn half of
        // the gate; PROT_EXEC-equivalent for the atom OS.)
        if !kernel_taint::gate_exec_spawn(pid as u64) {
            frame.rax = ERROR;
        } else
        {
        let request = (|| {
            let name = user_string(context, arg, 64)?;
            let extra = if is(number, SYS_EXEC_ARGS) || is(number, SYS_SPAWN_ARGS) {
                if arg2 > MAX_ARG_BYTES as u64 { return Err(()); }
                if arg2 == 0 { Vec::new() } else {
                    if !context.space.as_ref().unwrap().valid_user_range(arg1, arg2 as usize, false) { return Err(()); }
                    unsafe { core::slice::from_raw_parts(arg1 as *const u8, arg2 as usize) }.to_vec()
                }
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
                // Switch before dropping the old address-space owner.
                unsafe { Cr3::load(space.root); }
                context.page_table_root = space.root;
                context.space = Some(space);
                context.arguments = packed;
                context.open_files = [const { None }; 16]; context.fs_error = 0;
                *frame = TrapFrame::new_user(entry, STACK_TOP);
                unsafe { crate::process::reset_fpu(rsp); }
            }
        }
        } // E34 taint gate close
    } else if is(number, SYS_ARGS) {
        let len = context.arguments.len();
        if arg1 >= len as u64 && context.space.as_ref().unwrap().valid_user_range(arg, len, true) {
            unsafe { core::ptr::copy_nonoverlapping(context.arguments.as_ptr(), arg as *mut u8, len); }
            frame.rax = len as u64;
        }
    } else if is(number, SYS_PROCESSES) {
        // A fixed-capacity, single-call snapshot avoids races between queries.
        let bytes = MAX_PROCESSES * core::mem::size_of::<ProcessInfo>();
        if arg1 == MAX_PROCESSES as u64 && context.space.as_ref().unwrap().valid_user_range(arg, bytes, true) {
            let (records, count) = system.scheduler.snapshot();
            unsafe { core::ptr::copy_nonoverlapping(records.as_ptr() as *const u8, arg as *mut u8, bytes); }
            frame.rax = count as u64;
        }
    } else if is(number, SYS_KILL) {
        if system.terminate(arg as usize, KILLED_STATUS).is_ok() {
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
    } else if is(number, SYS_SYNC) {
        let result = kernel_kit::storage::sync().map(|()| 0).map_err(|_| FsError::Io);
        fs_result(context, frame, result);
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
    } else if is(number, SYS_REBOOT) {
        if kernel_kit::storage::sync().is_ok() {
            let status = kernel_kit::io::Port::new(0x64);
            for _ in 0..100000 { if status.read() & 2 == 0 { break; } core::hint::spin_loop(); }
            kernel_kit::io::Port::new(0x64).write(0xfe);
        } else { context.fs_error = FsError::Io as u64; }
    }
    system.scheduler.collect();
    if switch { system.scheduler.switch_context(rsp) } else { rsp }
}
