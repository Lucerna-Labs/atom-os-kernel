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
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        let mut ticks: u64 = 0;
        loop {
            // Delay loop
            for _ in 0..500000 {
                core::arch::asm!("int 0x80", inout("rax") 1 => _, options(nostack, preserves_flags)); // SYS_YIELD
            }
            
            let mut ptr: u64;
            core::arch::asm!("int 0x80", inout("rax") 16u64 => ptr, options(nostack, preserves_flags)); // SYS_IPC_RECV
            
            if ptr != 0 && ptr != core::u64::MAX {
                let prefix = b"\n[Daemon] Received IPC: \0";
                let mut i = 0;
                while prefix[i] != 0 {
                    core::arch::asm!("int 0x80", inout("rax") 5 => _, in("rdi") prefix[i] as u64, options(nostack, preserves_flags));
                    i += 1;
                }
                
                let msg_ptr = ptr as *const u8;
                let mut j = 0;
                while *msg_ptr.add(j) != 0 && j < 255 {
                    core::arch::asm!("int 0x80", inout("rax") 5 => _, in("rdi") *msg_ptr.add(j) as u64, options(nostack, preserves_flags));
                    j += 1;
                }
                
                core::arch::asm!("int 0x80", inout("rax") 5 => _, in("rdi") b'\n' as u64, options(nostack, preserves_flags));
            } else {
                let msg = b"[Daemon] Heartbeat... \0";
                let mut i = 0;
                while msg[i] != 0 {
                    core::arch::asm!("int 0x80", inout("rax") 5 => _, in("rdi") msg[i] as u64, options(nostack, preserves_flags)); // SYS_WRITE
                    i += 1;
                }
            }
        }
    }
}
