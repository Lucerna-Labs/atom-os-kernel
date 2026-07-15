#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[global_allocator]
static ALLOCATOR: DummyAllocator = DummyAllocator;

struct DummyAllocator;

unsafe impl core::alloc::GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        // Since payload runs in Ring 3 but the kernel hasn't given it a real heap,
        // using alloc::string::String without an allocator will fail.
        // Wait, String requires an allocator! 
        // We either implement a bump allocator for the payload, or we ask the kernel for pages.
        // For simplicity, let's just make a statically sized string or array instead of alloc::string::String for the payload!
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[derive(PartialEq)]
enum ShellMode {
    Prompt,
    Editor,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut buffer_len: usize = 0;
        let mut mode = ShellMode::Prompt;
        let mut current_fd: u64 = core::u64::MAX;

        // Automatically run bench on boot for CI
        let start = core::arch::x86_64::_rdtsc();
        for _ in 0..10_000 {
            core::arch::asm!("int 0x80", in("rax") 1, options(nostack, preserves_flags)); // SYS_YIELD
        }
        let end = core::arch::x86_64::_rdtsc();
        let diff = end - start;
        
        let success = b"10,000 SYS_YIELDs took (CPU cycles): \0";
        let mut i = 0;
        while success[i] != 0 {
            core::arch::asm!("int 0x80", in("rax") 5, in("rdi") success[i] as u64, options(nostack, preserves_flags));
            i += 1;
        }
        
        // Print u64
        let mut n = diff;
        if n == 0 {
            core::arch::asm!("int 0x80", in("rax") 5, in("rdi") b'0' as u64, options(nostack, preserves_flags));
        } else {
            let mut num_buf = [0u8; 20];
            let mut i = 0;
            while n > 0 {
                num_buf[i] = (n % 10) as u8 + b'0';
                n /= 10;
                i += 1;
            }
            while i > 0 {
                i -= 1;
                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") num_buf[i] as u64, options(nostack, preserves_flags));
            }
        }
        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") b'\n' as u64, options(nostack, preserves_flags));

        // Print initial prompt
        let prompt = b"> \0";
        let mut i = 0;
        while prompt[i] != 0 {
            core::arch::asm!("int 0x80", in("rax") 5, in("rdi") prompt[i] as u64, options(nostack, preserves_flags));
            i += 1;
        }

        loop {
            let mut scancode: u64;
            core::arch::asm!(
                "int 0x80",
                inout("rax") 4u64 => scancode, // SYS_READ
                options(nostack, preserves_flags)
            );

            if scancode != 0 {
                let c = scancode as u8;
                
                if c != 0 {
                    if mode == ShellMode::Editor {
                        if c == 0x1B { // ESC
                            // Save and Exit Editor
                            if current_fd != core::u64::MAX {
                                core::arch::asm!("int 0x80", in("rax") 12, in("rdi") current_fd, options(nostack, preserves_flags)); // SYS_TRUNCATE
                                for b in 0..buffer_len {
                                    core::arch::asm!("int 0x80", in("rax") 8, in("rdi") current_fd, in("rsi") buffer[b] as u64, options(nostack, preserves_flags));
                                }
                                core::arch::asm!("int 0x80", in("rax") 9, in("rdi") current_fd, options(nostack, preserves_flags)); // SYS_CLOSE
                                current_fd = core::u64::MAX;
                            }
                            mode = ShellMode::Prompt;
                            buffer_len = 0;
                            core::arch::asm!("int 0x80", in("rax") 11, options(nostack, preserves_flags)); // SYS_CLEAR
                            
                            let prompt = b"> \0";
                            let mut i = 0;
                            while prompt[i] != 0 {
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") prompt[i] as u64, options(nostack, preserves_flags));
                                i += 1;
                            }
                        } else if c == 0x08 { // Backspace
                            if buffer_len > 0 {
                                buffer_len -= 1;
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") c as u64, options(nostack, preserves_flags));
                            }
                        } else {
                            if buffer_len < 1024 {
                                buffer[buffer_len] = c;
                                buffer_len += 1;
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") c as u64, options(nostack, preserves_flags));
                            }
                        }
                    } else {
                        // Prompt Mode
                        if c == 0x08 { // Backspace
                            if buffer_len > 0 {
                                buffer_len -= 1;
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") c as u64, options(nostack, preserves_flags));
                            }
                        } else if c == b'\n' {
                            core::arch::asm!("int 0x80", in("rax") 5, in("rdi") c as u64, options(nostack, preserves_flags));
                            
                            // Simple manual parsing to avoid alloc::string dependency
                            if buffer_len == 2 && buffer[0] == b'l' && buffer[1] == b's' {
                                core::arch::asm!("int 0x80", in("rax") 10, options(nostack, preserves_flags)); // SYS_LIST_DIR
                            } else if buffer_len == 5 && buffer[0] == b'c' && buffer[1] == b'l' && buffer[2] == b'e' && buffer[3] == b'a' && buffer[4] == b'r' {
                                core::arch::asm!("int 0x80", in("rax") 11, options(nostack, preserves_flags)); // SYS_CLEAR
                            } else if buffer_len > 5 && buffer[0] == b'e' && buffer[1] == b'd' && buffer[2] == b'i' && buffer[3] == b't' && buffer[4] == b' ' {
                                let mut filename_buf: [u8; 64] = [0; 64];
                                let mut f_len = 0;
                                for i in 5..buffer_len {
                                    if f_len < 63 {
                                        filename_buf[f_len] = buffer[i];
                                        f_len += 1;
                                    }
                                }
                                filename_buf[f_len] = 0;
                                
                                core::arch::asm!("int 0x80", inout("rax") 6u64 => current_fd, in("rdi") filename_buf.as_ptr() as u64, options(nostack, preserves_flags));
                                
                                if current_fd != core::u64::MAX {
                                    mode = ShellMode::Editor;
                                    buffer_len = 0;
                                    core::arch::asm!("int 0x80", in("rax") 11, options(nostack, preserves_flags)); // SYS_CLEAR
                                    
                                    // Load existing file into buffer
                                    loop {
                                        let mut byte: u64;
                                        core::arch::asm!("int 0x80", inout("rax") 7u64 => byte, in("rdi") current_fd, options(nostack, preserves_flags)); // SYS_READ_FILE
                                        if byte == core::u64::MAX { break; } // EOF
                                        if buffer_len < 1024 {
                                            buffer[buffer_len] = byte as u8;
                                            buffer_len += 1;
                                            core::arch::asm!("int 0x80", in("rax") 5, in("rdi") byte, options(nostack, preserves_flags)); // Print to screen
                                        }
                                    }
                                    continue; // Skip prompt print
                                } else {
                                    let err = b"Failed to open file\n\0";
                                    let mut i = 0;
                                    while err[i] != 0 {
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") err[i] as u64, options(nostack, preserves_flags));
                                        i += 1;
                                    }
                                }
                            } else if buffer_len > 4 && buffer[0] == b'c' && buffer[1] == b'a' && buffer[2] == b't' && buffer[3] == b' ' {
                                let mut filename_buf: [u8; 64] = [0; 64];
                                let mut f_len = 0;
                                for i in 4..buffer_len {
                                    if f_len < 63 {
                                        filename_buf[f_len] = buffer[i];
                                        f_len += 1;
                                    }
                                }
                                filename_buf[f_len] = 0;
                                
                                let mut fd: u64;
                                core::arch::asm!("int 0x80", inout("rax") 6u64 => fd, in("rdi") filename_buf.as_ptr() as u64, options(nostack, preserves_flags));
                                
                                if fd != core::u64::MAX {
                                    loop {
                                        let mut byte: u64;
                                        core::arch::asm!("int 0x80", inout("rax") 7u64 => byte, in("rdi") fd, options(nostack, preserves_flags));
                                        if byte == core::u64::MAX { break; } // EOF
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") byte, options(nostack, preserves_flags));
                                    }
                                    core::arch::asm!("int 0x80", in("rax") 5, in("rdi") b'\n' as u64, options(nostack, preserves_flags));
                                    core::arch::asm!("int 0x80", in("rax") 9, in("rdi") fd, options(nostack, preserves_flags)); // SYS_CLOSE
                                } else {
                                    let err = b"File not found\n\0";
                                    let mut i = 0;
                                    while err[i] != 0 {
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") err[i] as u64, options(nostack, preserves_flags));
                                        i += 1;
                                    }
                                }
                            } else if buffer_len > 5 && buffer[0] == b'e' && buffer[1] == b'c' && buffer[2] == b'h' && buffer[3] == b'o' && buffer[4] == b' ' {
                                // Find ' > '
                                let mut greater_idx = 0;
                                for i in 5..(buffer_len - 2) {
                                    if buffer[i] == b' ' && buffer[i+1] == b'>' && buffer[i+2] == b' ' {
                                        greater_idx = i;
                                        break;
                                    }
                                }

                                if greater_idx > 0 {
                                    let mut filename_buf: [u8; 64] = [0; 64];
                                    let mut f_len = 0;
                                    for i in (greater_idx + 3)..buffer_len {
                                        if f_len < 63 {
                                            filename_buf[f_len] = buffer[i];
                                            f_len += 1;
                                        }
                                    }
                                    filename_buf[f_len] = 0;
                                    
                                    let mut fd: u64;
                                    core::arch::asm!("int 0x80", inout("rax") 6u64 => fd, in("rdi") filename_buf.as_ptr() as u64, options(nostack, preserves_flags));
                                    
                                    if fd != core::u64::MAX {
                                        for i in 5..greater_idx {
                                            core::arch::asm!("int 0x80", in("rax") 8, in("rdi") fd, in("rsi") buffer[i] as u64, options(nostack, preserves_flags));
                                        }
                                        // Append newline
                                        core::arch::asm!("int 0x80", in("rax") 8, in("rdi") fd, in("rsi") b'\n' as u64, options(nostack, preserves_flags));
                                        core::arch::asm!("int 0x80", in("rax") 9, in("rdi") fd, options(nostack, preserves_flags)); // SYS_CLOSE
                                    }
                                }
                            } else if buffer_len > 4 && buffer[0] == b'm' && buffer[1] == b's' && buffer[2] == b'g' && buffer[3] == b' ' {
                                let mut msg_buf: [u8; 64] = [0; 64];
                                let mut f_len = 0;
                                for i in 4..buffer_len {
                                    if f_len < 63 {
                                        msg_buf[f_len] = buffer[i];
                                        f_len += 1;
                                    }
                                }
                                msg_buf[f_len] = 0;
                                
                                let mut res: u64;
                                core::arch::asm!("int 0x80", inout("rax") 15u64 => res, in("rdi") 2u64, in("rsi") msg_buf.as_ptr() as u64, options(nostack, preserves_flags)); // SYS_IPC_SEND
                                
                                if res != core::u64::MAX {
                                    let success = b"Message sent to Daemon\n\0";
                                    let mut i = 0;
                                    while success[i] != 0 {
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") success[i] as u64, options(nostack, preserves_flags));
                                        i += 1;
                                    }
                                } else {
                                    let err = b"Failed to send message\n\0";
                                    let mut i = 0;
                                    while err[i] != 0 {
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") err[i] as u64, options(nostack, preserves_flags));
                                        i += 1;
                                    }
                                }
                            } else if buffer_len == 5 && buffer[0] == b'b' && buffer[1] == b'e' && buffer[2] == b'n' && buffer[3] == b'c' && buffer[4] == b'h' {
                                let start = unsafe { core::arch::x86_64::_rdtsc() };
                                for _ in 0..10_000 {
                                    core::arch::asm!("int 0x80", in("rax") 1, options(nostack, preserves_flags)); // SYS_YIELD
                                }
                                let end = unsafe { core::arch::x86_64::_rdtsc() };
                                let diff = end - start;
                                
                                let success = b"10,000 SYS_YIELDs took (CPU cycles): \0";
                                let mut i = 0;
                                while success[i] != 0 {
                                    core::arch::asm!("int 0x80", in("rax") 5, in("rdi") success[i] as u64, options(nostack, preserves_flags));
                                    i += 1;
                                }
                                
                                // Print u64
                                let mut n = diff;
                                if n == 0 {
                                    core::arch::asm!("int 0x80", in("rax") 5, in("rdi") b'0' as u64, options(nostack, preserves_flags));
                                } else {
                                    let mut num_buf = [0u8; 20];
                                    let mut i = 0;
                                    while n > 0 {
                                        num_buf[i] = (n % 10) as u8 + b'0';
                                        n /= 10;
                                        i += 1;
                                    }
                                    while i > 0 {
                                        i -= 1;
                                        core::arch::asm!("int 0x80", in("rax") 5, in("rdi") num_buf[i] as u64, options(nostack, preserves_flags));
                                    }
                                }
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") b'\n' as u64, options(nostack, preserves_flags));
                            } else if buffer_len > 0 {
                                let err = b"Unknown command\n\0";
                                let mut i = 0;
                                while err[i] != 0 {
                                    core::arch::asm!("int 0x80", in("rax") 5, in("rdi") err[i] as u64, options(nostack, preserves_flags));
                                    i += 1;
                                }
                            }

                            buffer_len = 0;
                            
                            // Print prompt again
                            let prompt = b"> \0";
                            let mut i = 0;
                            while prompt[i] != 0 {
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") prompt[i] as u64, options(nostack, preserves_flags));
                                i += 1;
                            }
                        } else {
                            if buffer_len < 1024 {
                                buffer[buffer_len] = c;
                                buffer_len += 1;
                                core::arch::asm!("int 0x80", in("rax") 5, in("rdi") c as u64, options(nostack, preserves_flags));
                            }
                        }
                    }
                }
            } else {
                core::arch::asm!("int 0x80", in("rax") 1, options(nostack, preserves_flags)); // SYS_YIELD
            }
        }
    }
}
