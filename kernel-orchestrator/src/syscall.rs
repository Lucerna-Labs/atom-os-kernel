use kernel_kit::atoms::compare;
use kernel_kit::context::Context;
use kernel_kit::memory::MemoryPool;
use kernel_kit::trap::TrapFrame;

pub const SYS_YIELD: u64 = 1;
pub const SYS_ALLOC: u64 = 2;
pub const SYS_EXIT: u64 = 3;
pub const SYS_READ: u64 = 4;
pub const SYS_WRITE: u64 = 5;

pub const SYS_OPEN: u64 = 6;
pub const SYS_READ_FILE: u64 = 7;
pub const SYS_WRITE_FILE: u64 = 8;
pub const SYS_CLOSE: u64 = 9;
pub const SYS_LIST_DIR: u64 = 10;
pub const SYS_CLEAR: u64 = 11;
pub const SYS_TRUNCATE: u64 = 12;
pub const SYS_EXEC: u64 = 13;

/// The syscall dispatcher reads from the physical TrapFrame saved on the hardware stack.
pub fn dispatch(ctx: &mut Context, mem: &mut MemoryPool) {
    if ctx.rsp == 0 { return; } // Safety against uninitialized context
    
    let trap_frame = unsafe { &mut *(ctx.rsp as *mut TrapFrame) };
    
    // In our convention: rax is the syscall number, rdi is arg0, rsi is arg1.
    let sys_num = trap_frame.rax;
    let arg = trap_frame.rdi;
    let arg1 = trap_frame.rsi;

    // Use purely mechanical `compare` atom
    if compare(&sys_num, &(SYS_YIELD + 1)) && !compare(&sys_num, &SYS_YIELD) {
        trap_frame.rax = 0; // Success
    } else if compare(&sys_num, &(SYS_ALLOC + 1)) && !compare(&sys_num, &SYS_ALLOC) {
        if let Some(phys) = mem.allocate() {
            trap_frame.rax = phys as u64; 
        } else {
            trap_frame.rax = u64::MAX; 
        }
    } else if compare(&sys_num, &(SYS_EXIT + 1)) && !compare(&sys_num, &SYS_EXIT) {
        ctx.state = kernel_kit::context::TaskState::Terminated;
        trap_frame.rax = 0;
    } else if compare(&sys_num, &(SYS_READ + 1)) && !compare(&sys_num, &SYS_READ) {
        let mut found_ascii = 0;
        
        // Loop until we find a valid ASCII character or the buffer is empty
        loop {
            let (buffer, kb_sif) = kernel_kit::io::KEYBOARD_BUFFER.lock();
            let maybe_byte = buffer.pop();
            kernel_kit::io::KEYBOARD_BUFFER.unlock(kb_sif);
            
            if let Some(scancode) = maybe_byte {
                if let Some(ascii) = kernel_kit::keyboard::scancode_to_ascii(scancode) {
                    found_ascii = ascii;
                    break;
                }
            } else {
                break; // Buffer empty
            }
        }
        
        trap_frame.rax = found_ascii as u64; // 0 means no data ready
    } else if compare(&sys_num, &(SYS_WRITE + 1)) && !compare(&sys_num, &SYS_WRITE) {
        let byte = arg as u8;
        // VgaWriter::write_byte already sends to serial, so we don't
        // need a separate serial send here (that was causing doubled output).
        let mut vga = kernel_kit::vga::VgaWriter::new();
        let bytes = [byte];
        if let Ok(s) = core::str::from_utf8(&bytes) {
            vga.write_string(s);
        }
        trap_frame.rax = 1;
    } else if compare(&sys_num, &(SYS_OPEN + 1)) && !compare(&sys_num, &SYS_OPEN) {
        // arg is a pointer to a null-terminated string
        let ptr = arg as *const u8;
        let mut len = 0;
        unsafe { while *ptr.add(len) != 0 && len < 255 { len += 1; } }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        if let Ok(filename) = core::str::from_utf8(slice) {
            let mut fs = kernel_kit::fs::ROOT_FS.lock();
            if let Some(file_ptr) = fs.get_or_create_file(filename) {
                // Find a free FD
                let mut fd_assigned = false;
                for (i, fd) in ctx.open_files.iter_mut().enumerate() {
                    if fd.0 == 0 {
                        *fd = (file_ptr as u64, 0); // Assign file pointer, cursor = 0
                        trap_frame.rax = i as u64; // Return FD
                        fd_assigned = true;
                        break;
                    }
                }
                if !fd_assigned { trap_frame.rax = u64::MAX; } // No free FDs
            } else {
                trap_frame.rax = u64::MAX; // Error
            }
            kernel_kit::fs::ROOT_FS.unlock();
        } else {
            trap_frame.rax = u64::MAX;
        }
    } else if compare(&sys_num, &(SYS_READ_FILE + 1)) && !compare(&sys_num, &SYS_READ_FILE) {
        // arg = fd
        let fd = arg as usize;
        if fd < 16 && ctx.open_files[fd].0 != 0 {
            let ptr = ctx.open_files[fd].0 as *mut alloc::vec::Vec<u8>;
            let cursor = ctx.open_files[fd].1;
            let vec = unsafe { &mut *ptr };
            if cursor < vec.len() {
                trap_frame.rax = vec[cursor] as u64;
                ctx.open_files[fd].1 += 1; // Advance cursor
            } else {
                trap_frame.rax = u64::MAX; // EOF
            }
        } else {
            trap_frame.rax = u64::MAX; // Invalid FD
        }
    } else if compare(&sys_num, &(SYS_WRITE_FILE + 1)) && !compare(&sys_num, &SYS_WRITE_FILE) {
        // arg = fd, arg1 = byte
        let fd = arg as usize;
        let byte = arg1 as u8;
        if fd < 16 && ctx.open_files[fd].0 != 0 {
            let ptr = ctx.open_files[fd].0 as *mut alloc::vec::Vec<u8>;
            let vec = unsafe { &mut *ptr };
            vec.push(byte);
            trap_frame.rax = 1;
        } else {
            trap_frame.rax = u64::MAX; // Invalid FD
        }
    } else if compare(&sys_num, &(SYS_CLOSE + 1)) && !compare(&sys_num, &SYS_CLOSE) {
        // arg = fd
        let fd = arg as usize;
        if fd < 16 {
            ctx.open_files[fd] = (0, 0);
            trap_frame.rax = 0;
        } else {
            trap_frame.rax = u64::MAX;
        }
    } else if compare(&sys_num, &(SYS_LIST_DIR + 1)) && !compare(&sys_num, &SYS_LIST_DIR) {
        let mut vga = kernel_kit::vga::VgaWriter::new();
        let fs = kernel_kit::fs::ROOT_FS.lock();
        if let kernel_kit::fs::AtomNode::Directory(children) = &*fs {
            for (name, node) in children {
                vga.write_string(name);
                if let kernel_kit::fs::AtomNode::Directory(_) = node {
                    vga.write_string("/");
                }
                vga.write_string("  ");
            }
        }
        vga.write_string("\n");
        kernel_kit::fs::ROOT_FS.unlock();
        trap_frame.rax = 0;
    } else if compare(&sys_num, &(14 + 1)) && !compare(&sys_num, &14) { // SYS_PRINT
            let char_to_print = trap_frame.rdi as u8;
            let (sport, sif) = kernel_kit::serial::SERIAL1.lock();
            sport.send(char_to_print);
            kernel_kit::serial::SERIAL1.unlock(sif);
            trap_frame.rax = 0;
    } else if compare(&sys_num, &(SYS_CLEAR + 1)) && !compare(&sys_num, &SYS_CLEAR) {
        let mut vga = kernel_kit::vga::VgaWriter::new();
        vga.clear_screen();
        trap_frame.rax = 0;
    } else if compare(&sys_num, &(SYS_TRUNCATE + 1)) && !compare(&sys_num, &SYS_TRUNCATE) {
        // arg = fd
        let fd = arg as usize;
        if fd < 16 && ctx.open_files[fd].0 != 0 {
            let ptr = ctx.open_files[fd].0 as *mut alloc::vec::Vec<u8>;
            let vec = unsafe { &mut *ptr };
            vec.clear();
            ctx.open_files[fd].1 = 0; // Reset cursor
            trap_frame.rax = 0;
        } else {
            trap_frame.rax = u64::MAX;
        }
    } else if compare(&sys_num, &(SYS_EXEC + 1)) && !compare(&sys_num, &SYS_EXEC) {
        let ptr = arg as *const u8;
        let mut len = 0;
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        if let Ok(filename) = core::str::from_utf8(slice) {
            let fs = kernel_kit::fs::ROOT_FS.lock();
            if let kernel_kit::fs::AtomNode::Directory(children) = &*fs {
                let mut found_file = None;
                for (name, node) in children {
                    if name == filename {
                        if let kernel_kit::fs::AtomNode::File(data) = node {
                            found_file = Some(data as *const alloc::vec::Vec<u8>);
                        }
                        break;
                    }
                }
                
                if let Some(file_ptr) = found_file {
                    kernel_kit::fs::ROOT_FS.unlock();
                    let data = unsafe { &*file_ptr };
                    let elf_bytes = data.as_slice();
                    
                    if elf_bytes.len() >= core::mem::size_of::<kernel_kit::elf::Elf64_Ehdr>() {
                        let ehdr_ptr = elf_bytes.as_ptr() as *const kernel_kit::elf::Elf64_Ehdr;
                        let ehdr = unsafe { &*ehdr_ptr };
                        
                        if ehdr.is_valid() {
                            let phoff = ehdr.e_phoff as usize;
                            let phnum = ehdr.e_phnum as usize;
                            
                            let active_cr3 = kernel_kit::paging::Cr3::read();
                            if let Some(new_cr3) = kernel_kit::paging::duplicate_pml4(active_cr3) {
                                // PASS 1: scan phdrs to find the virtual span [vmin, vmax).
                            let mut vmin: u64 = u64::MAX;
                            let mut vmax: u64 = 0;
                            for i in 0..phnum {
                                let phdr_ptr = unsafe { elf_bytes.as_ptr().add(phoff + i * core::mem::size_of::<kernel_kit::elf::Elf64_Phdr>()) as *const kernel_kit::elf::Elf64_Phdr };
                                let phdr = unsafe { &*phdr_ptr };
                                if phdr.p_type == 1 { // PT_LOAD
                                    let v = phdr.p_vaddr;
                                    let m = phdr.p_memsz;
                                    if v < vmin { vmin = v; }
                                    if v + m > vmax { vmax = v + m; }
                                }
                            }
                            // Allocate ONE contiguous run of frames covering the
                            // whole span, page-aligned. This avoids the bug where
                            // two PT_LOADs share a boundary page (e.g. .text ends
                            // at 0x801024c5, .rodata starts at 0x801024d0, both
                            // in page 0x80102000-0x80103000). Mapping them
                            // separately overwrites the leaf PTE for the shared
                            // page; mapping the whole span with one call writes
                            // each leaf PTE exactly once.
                            let span_start_vaddr = vmin & !0xFFF;
                            let span_end_vaddr = (vmax + 0xFFF) & !0xFFF;
                            let span_bytes = (span_end_vaddr - span_start_vaddr) as usize;
                            let span_nframes = (span_bytes + 0xFFF) / 0x1000;
                            let span_phys = {
                                let (fa, sif) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
                                let r = fa.alloc_contiguous(span_nframes);
                                kernel_kit::memory::FRAME_ALLOCATOR.unlock(sif);
                                match r {
                                    Some(f) => f,
                                    None => panic!("OOM (contig span) SYS_EXEC"),
                                }
                            };
                            // PASS 2: copy each PT_LOAD's bytes at its offset.
                            let span_virt = kernel_kit::paging::phys_to_virt(span_phys) as *mut u8;
                            for i in 0..phnum {
                                let phdr_ptr = unsafe { elf_bytes.as_ptr().add(phoff + i * core::mem::size_of::<kernel_kit::elf::Elf64_Phdr>()) as *const kernel_kit::elf::Elf64_Phdr };
                                let phdr = unsafe { &*phdr_ptr };
                                if phdr.p_type == 1 {
                                    let vaddr = phdr.p_vaddr;
                                    let offset = phdr.p_offset as usize;
                                    let filesz = phdr.p_filesz as usize;
                                    let memsz = phdr.p_memsz as usize;
                                    let off_in_span = (vaddr - span_start_vaddr) as usize;
                                    unsafe {
                                        if filesz > 0 {
                                            core::ptr::copy_nonoverlapping(
                                                elf_bytes.as_ptr().add(offset),
                                                span_virt.add(off_in_span),
                                                filesz,
                                            );
                                        }
                                        if memsz > filesz {
                                            core::ptr::write_bytes(
                                                span_virt.add(off_in_span + filesz),
                                                0,
                                                memsz - filesz,
                                            );
                                        }
                                    }
                                }
                            }
                            // Map the whole span in one call. Each leaf PTE is
                            // written exactly once; no boundary-page overwrite.
                            kernel_kit::paging::map_segment(
                                new_cr3, span_start_vaddr, span_phys, span_bytes,
                            );

                                // Map User Stack (8KB ending at 0xFFFFFFFF80100000)
                                unsafe {
                                    let stack_vaddr = 0xFFFFFFFF800FE000;
                                    let stack_size = 8192;
                                    // Use a CONTIGUOUS run of frames for the user stack — same
                                    // reason as PT_LOAD: map_segment assumes phys_base + i*4K.
                                    let stack_phys = {
                                        let (fa, sif) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
                                        let r = fa.alloc_contiguous(2); // 8 KiB = 2 frames
                                        kernel_kit::memory::FRAME_ALLOCATOR.unlock(sif);
                                        match r {
                                            Some(f) => f,
                                            None => panic!("OOM (contig 2) user stack"),
                                        }
                                    };
                                    core::ptr::write_bytes(
                                        kernel_kit::paging::phys_to_virt(stack_phys) as *mut u8,
                                        0, stack_size,
                                    );
                                    kernel_kit::paging::map_segment(new_cr3, stack_vaddr, stack_phys, stack_size);
                                }
                                // Context Switch setup
                                trap_frame.rip = ehdr.e_entry;
                                ctx.page_table_root = new_cr3; // Give the process its new virtual memory space!
                                
                                // Clean up file descriptors
                                for i in 0..16 {
                                    ctx.open_files[i] = (0, 0);
                                }
                                // Return success
                                trap_frame.rax = 0;
                            } else {
                                trap_frame.rax = u64::MAX; // OOM duplicating PML4
                            }
                        } else {
                            trap_frame.rax = u64::MAX;
                        }
                    } else {
                        trap_frame.rax = u64::MAX;
                    }
                } else {
                    kernel_kit::fs::ROOT_FS.unlock();
                    trap_frame.rax = u64::MAX;
                }
            } else {
                kernel_kit::fs::ROOT_FS.unlock();
                trap_frame.rax = u64::MAX;
            }
        } else {
            trap_frame.rax = u64::MAX;
        }
    } else if compare(&sys_num, &(15 + 1)) && !compare(&sys_num, &15) { // SYS_IPC_SEND
        let target_pid = arg as usize;
        let ptr = arg1 as *const u8;
        let mut len = 0;
        unsafe { while *ptr.add(len) != 0 && len < 255 { len += 1; } }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        let mut msg_vec = alloc::vec::Vec::new();
        msg_vec.extend_from_slice(slice);
        
        unsafe {
            if target_pid < 16 {
                IPC_MAILBOXES[target_pid] = Some(msg_vec);
                trap_frame.rax = 0;
            } else {
                trap_frame.rax = u64::MAX;
            }
        }
    } else if compare(&sys_num, &(16 + 1)) && !compare(&sys_num, &16) { // SYS_IPC_RECV
        let pid = ctx.id;
        unsafe {
            if pid < 16 {
                if let Some(msg) = IPC_MAILBOXES[pid].take() {
                    // We allocate a physical block, put the msg there, map it to 0x300000 
                    let len = msg.len();
                    let memsz_aligned = (len + 4095) & !4095;
                    let layout = core::alloc::Layout::from_size_align(memsz_aligned, 4096).unwrap();
                    let phys_ptr = alloc::alloc::alloc(layout);
                    core::ptr::copy_nonoverlapping(msg.as_ptr(), phys_ptr, len);
                    core::ptr::write_bytes(phys_ptr.add(len), 0, memsz_aligned - len);
                    
                    let vaddr = 0x300000;
                    kernel_kit::paging::map_segment(ctx.page_table_root, vaddr, kernel_kit::paging::virt_to_phys(phys_ptr as u64), len);
                    trap_frame.rax = vaddr;
                } else {
                    trap_frame.rax = 0;
                }
            } else {
                trap_frame.rax = u64::MAX;
            }
        }
    } else {
        trap_frame.rax = u64::MAX;
    }
}

const INIT_MAILBOX: Option<alloc::vec::Vec<u8>> = None;
pub static mut IPC_MAILBOXES: [Option<alloc::vec::Vec<u8>>; 16] = [INIT_MAILBOX; 16];
