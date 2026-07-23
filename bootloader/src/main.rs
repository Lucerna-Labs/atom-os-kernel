#![no_std]
#![no_main]

//! ATOM Bootloader - 100% Dependency-Free
//! 
//! Built from the 8 root atoms:
//! - scan: examine memory and hardware
//! - hash: compute addresses and checksums  
//! - fold: compress and transform data
//! - project: map virtual to physical
//! - scale: adjust sizes and offsets
//! - compare: validate and branch
//! - combine: merge operations
//! - order: sequence boot steps

use core::panic::PanicInfo;

/// Boot information passed to kernel
#[repr(C)]
pub struct BootInfo {
    pub physical_memory_offset: u64,
    pub kernel_start: u64,
    pub kernel_end: u64,
    pub memory_map_addr: u64,
    pub memory_map_len: u64,
}

/// Entry point called from assembly
#[no_mangle]
pub extern "C" fn bootloader_main() -> ! {
    // TODO: Implement boot sequence using atoms
    // 1. scan: Detect hardware and memory
    // 2. project: Set up paging
    // 3. scan: Load kernel from disk
    // 4. compare: Validate kernel
    // 5. project: Jump to kernel
    
    // For now, just halt
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}
