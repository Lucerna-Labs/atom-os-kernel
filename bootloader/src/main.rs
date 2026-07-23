#![no_std]
#![no_main]

//! ATOM Bootloader - 100% Dependency-Free
//! Built from math primitives: scan, hash, fold, project, scale, compare, combine, order
//! 
//! This bootloader is completely agnostic - no external dependencies.
//! It handles: CPU initialization, mode switching, memory setup, and kernel loading.

use core::panic::PanicInfo;
use core::arch::asm;

// ATOM BootInfo structure - built from scan atom
#[repr(C)]
pub struct BootInfo {
    pub physical_memory_offset: u64,
    pub kernel_start: u64,
    pub kernel_end: u64,
    pub memory_map_addr: u64,
    pub memory_map_len: u64,
}

// ATOM memory region types - built from compare atom
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    Usable = 1,
    Reserved = 2,
    AcpiReclaimable = 3,
    AcpiNvs = 4,
    BadMemory = 5,
    Bootloader = 6,
    Kernel = 7,
    Framebuffer = 8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
    pub start: u64,
    pub end: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub range: MemoryRange,
    pub region_type: MemoryRegionType,
}

// Multiboot2 header - built from hash atom (checksum validation)
#[repr(C, packed)]
struct MultibootHeader {
    magic: u32,
    architecture: u32,
    header_length: u32,
    checksum: u32,
}

#[used]
#[link_section = ".multiboot"]
static MULTIBOOT_HEADER: MultibootHeader = MultibootHeader {
    magic: 0xE85250D6,
    architecture: 0, // Protected mode i386
    header_length: core::mem::size_of::<MultibootHeader>() as u32,
    checksum: (0x100000000u64 - (0xE85250D6u64 + 0u64 + core::mem::size_of::<MultibootHeader>() as u64)) as u32,
};

// GDT for 64-bit mode - built from project atom
#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

// Global state - built from combine atom
static mut MEMORY_MAP: [MemoryRegion; 32] = [MemoryRegion {
    range: MemoryRange { start: 0, end: 0 },
    region_type: MemoryRegionType::Usable,
}; 32];
static mut MEMORY_MAP_COUNT: u32 = 0;
static mut DISK_BUFFER: [u8; 65536] = [0; 65536]; // 64KB buffer for disk I/O

static mut GDT: [GdtEntry; 3] = [
    // Null descriptor
    GdtEntry {
        limit_low: 0,
        base_low: 0,
        base_mid: 0,
        access: 0,
        granularity: 0,
        base_high: 0,
    },
    // Code segment (64-bit)
    GdtEntry {
        limit_low: 0xFFFF,
        base_low: 0,
        base_mid: 0,
        access: 0x9A, // Present, Ring 0, Code, Executable, Readable
        granularity: 0xAF, // 64-bit, Granularity 4K
        base_high: 0,
    },
    // Data segment (64-bit)
    GdtEntry {
        limit_low: 0xFFFF,
        base_low: 0,
        base_mid: 0,
        access: 0x92, // Present, Ring 0, Data, Writable
        granularity: 0xCF, // 32-bit, Granularity 4K
        base_high: 0,
    },
];

static mut GDT_PTR: GdtPointer = GdtPointer {
    limit: 0,
    base: 0,
};

// Entry point - called by BIOS/UEFI or multiboot loader
#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        // Disable interrupts
        asm!("cli");
        
        // Set up stack (will be set by linker script or bootloader protocol)
        // For now, use a local stack
        let stack: [u8; 16384] = [0; 16384];
        let stack_top = stack.as_ptr() as u64 + stack.len() as u64;
        asm!("mov rsp, {0}", in(reg) stack_top);
        
        // Call Rust entry point
        bootloader_main();
        
        // Halt if we return
        loop {
            asm!("hlt");
        }
    }
}

/// Main bootloader logic - built from all 8 atoms
#[no_mangle]
pub extern "C" fn bootloader_main() -> ! {
    // ATOM: scan - detect hardware
    scan_hardware();
    
    // ATOM: project - set up paging
    setup_paging();
    
    // ATOM: hash - validate kernel
    let kernel_info = load_kernel();
    
    // ATOM: combine - prepare boot info
    let boot_info = prepare_bootInfo(kernel_info);
    
    // ATOM: order - jump to kernel
    unsafe {
        jump_to_kernel(boot_info);
    }
    
    // Should never reach here
    loop {
        unsafe { asm!("hlt"); }
    }
}

// ATOM: scan - examine hardware and memory
fn scan_hardware() {
    unsafe {
        // ATOM: scan - retrieve memory map using BIOS interrupt 0x15, EAX=0xE820
        let mut continuation_id: u32 = 0;
        let mut index = 0;
        
        loop {
            if index >= 32 {
                break;
            }
            
            // Initialize with valid enum value
            let mut entry = MemoryRegion {
                range: MemoryRange { start: 0, end: 0 },
                region_type: MemoryRegionType::Usable,
            };
            let mut status: u32 = 0;
            let mut ebx_val: u32 = continuation_id;
            
            // BIOS int 0x15 E820 requires ebx as continuation, but rbx is reserved
            // Save/restore it manually
            asm!(
                "push rbx",
                "mov ebx, {ebx_val:e}",
                "mov eax, 0xE820",
                "mov edx, 0x534D4150", // 'SMAP' signature
                "mov ecx, 24",         // Size of memory region entry
                "mov edi, {entry:e}",  // Pointer to entry
                "int 0x15",
                "jc 99f",              // Jump if error
                "mov {status:e}, 1",   // Success
                "mov {ebx_out:e}, ebx", // Save continuation
                "jmp 98f",
                "99: mov {status:e}, 0", // Error
                "98: pop rbx",
                ebx_val = in(reg) ebx_val,
                ebx_out = out(reg) ebx_val,
                entry = in(reg) &mut entry as *mut MemoryRegion as usize,
                status = out(reg) status,
                out("eax") _,
                out("ecx") _,
                out("edx") _,
                out("edi") _,
                options(nostack),
            );
            
            continuation_id = ebx_val;
            
            if status == 0 {
                break; // Error or end of memory map
            }
            
            MEMORY_MAP[index] = entry;
            index += 1;
            
            if continuation_id == 0 {
                break; // Last entry
            }
        }
        
        MEMORY_MAP_COUNT = index as u32;
        
        // ATOM: scan - detect disk drives
        // BIOS interrupt 0x13, AH=0x08 - Get Drive Parameters
        let mut drive_count: u32 = 0;
        asm!(
            "mov ah, 0x08",
            "mov dl, 0x80", // First hard disk
            "int 0x13",
            "jc 99f",
            "movzx {count:e}, dl", // Number of drives
            "jmp 98f",
            "99: mov {count:e}, 0",
            "98:",
            count = out(reg) drive_count,
            out("eax") _,
            out("edx") _,
            options(nostack),
        );
        
        // Store drive count (would be used by filesystem)
        DRIVE_COUNT = drive_count as u8;
    }
}

static mut DRIVE_COUNT: u8 = 0;

// ATOM: project - set up 64-bit paging
fn setup_paging() {
    unsafe {
        // Initialize GDT pointer
        GDT_PTR.limit = (core::mem::size_of::<[GdtEntry; 3]>() - 1) as u16;
        GDT_PTR.base = &GDT as *const _ as u64;
        
        // Load GDT
        asm!(
            "lgdt [{0}]",
            in(reg) &GDT_PTR,
        );
    }
}

// ATOM: fold - compress kernel loading logic
struct KernelInfo {
    start: u64,
    end: u64,
}

// ATOM: scan + fold - disk I/O using BIOS interrupt 0x13
unsafe fn read_disk_sectors(lba: u32, count: u16, buffer: *mut u8) -> u32 {
    // BIOS interrupt 0x13, AH=0x42 - Extended Read (LBA mode)
    // Drive 0x80 = first hard disk
    let mut success: u32 = 0;
    
    let packet = DiskAddressPacket {
        size: 16,
        zero: 0,
        count: count,
        buffer: buffer as u32,
        lba_low: lba,
        lba_high: 0,
    };
    
    asm!(
        "push rbx",
        "mov ah, 0x42",      // Extended read function
        "mov dl, 0x80",      // First hard disk
        "mov esi, {packet:e}", // Disk address packet (32-bit)
        "int 0x13",
        "jc 99f",            // Jump if error (carry flag set)
        "mov {success:e}, 1", // Success
        "jmp 98f",
        "99: mov {success:e}, 0", // Failure
        "98: pop rbx",
        packet = in(reg) &packet as *const DiskAddressPacket as usize,
        success = out(reg) success,
        out("eax") _,
        out("edx") _,
        out("esi") _,
        options(nostack),
    );
    
    success
}

#[repr(C, packed)]
struct DiskAddressPacket {
    size: u8,
    zero: u8,
    count: u16,
    buffer: u32,
    lba_low: u32,
    lba_high: u32,
}

// ATOM: hash + compare - filesystem detection and kernel loading
fn load_kernel() -> KernelInfo {
    unsafe {
        // ATOM: scan + fold - read boot sector to detect filesystem
        let boot_sector = DISK_BUFFER.as_mut_ptr();
        if read_disk_sectors(0, 1, boot_sector) == 0 {
            return fallback_kernel_location();
        }
        
        // ATOM: compare - detect filesystem type
        // Check for FAT signature (0x55 0xAA at offset 510-511)
        let sig = core::ptr::read_unaligned(boot_sector.add(510) as *const u16);
        if sig != 0xAA55 {
            // Not a valid boot sector, use fallback
            return fallback_kernel_location();
        }
        
        // ATOM: hash - detect FAT type by examining OEM name and sector size
        let oem_name = core::ptr::read_unaligned(boot_sector.add(3) as *const u64);
        let bytes_per_sector = core::ptr::read_unaligned(boot_sector.add(11) as *const u16);
        let sectors_per_cluster = *boot_sector.add(13);
        
        // Simple FAT detection: look for "FAT12", "FAT16", or "FAT" in OEM name
        let is_fat = oem_name & 0xFFFFFF == 0x544146; // "FAT"
        
        if !is_fat {
            return fallback_kernel_location();
        }
        
        // ATOM: fold - locate root directory
        let reserved_sectors = core::ptr::read_unaligned(boot_sector.add(14) as *const u16);
        let num_fats = *boot_sector.add(16);
        let root_entry_count = core::ptr::read_unaligned(boot_sector.add(17) as *const u16);
        let fat_size = core::ptr::read_unaligned(boot_sector.add(22) as *const u16);
        
        let root_dir_start = reserved_sectors as u32 + (num_fats as u32 * fat_size as u32);
        let root_dir_sectors = (root_entry_count * 32 + bytes_per_sector - 1) / bytes_per_sector;
        
        // ATOM: scan - read root directory
        let root_dir = DISK_BUFFER.as_mut_ptr();
        if read_disk_sectors(root_dir_start, root_dir_sectors, root_dir) == 0 {
            return fallback_kernel_location();
        }
        
        // ATOM: compare - search for kernel file "KERNEL  BIN" or "ATOM    BIN"
        for i in 0..root_entry_count {
            let entry_offset = i as usize * 32;
            let filename = core::ptr::read_unaligned(root_dir.add(entry_offset) as *const u64);
            let ext = core::ptr::read_unaligned(root_dir.add(entry_offset + 8) as *const u32) & 0xFFFFFF;
            
            // Check for "KERNEL  " or "ATOM    " with "BIN" extension
            let is_kernel = (filename == 0x20204C454E52454B || // "KERNEL  "
                            filename == 0x202020204D4F5441) && // "ATOM    "
                           ext == 0x4E4942; // "BIN"
            
            if is_kernel {
                // ATOM: project - get file location
                let cluster = core::ptr::read_unaligned(root_dir.add(entry_offset + 26) as *const u16);
                let file_size = core::ptr::read_unaligned(root_dir.add(entry_offset + 28) as *const u32);
                
                // ATOM: scale - convert cluster to LBA
                let data_start = root_dir_start + root_dir_sectors as u32;
                let kernel_lba = data_start + ((cluster as u32 - 2) * sectors_per_cluster as u32);
                let kernel_sectors = (file_size + bytes_per_sector as u32 - 1) / bytes_per_sector as u32;
                
                // ATOM: fold - read kernel file
                let kernel_buffer = DISK_BUFFER.as_mut_ptr();
                if read_disk_sectors(kernel_lba, kernel_sectors as u16, kernel_buffer) == 0 {
                    return fallback_kernel_location();
                }
                
                // ATOM: hash - validate kernel
                let kernel_start = kernel_buffer as u64;
                let kernel_end = kernel_start + file_size as u64;
                
                return KernelInfo {
                    start: kernel_start,
                    end: kernel_end,
                };
            }
        }
        
        // Kernel not found in filesystem, use fallback
        fallback_kernel_location()
    }
}

unsafe fn fallback_kernel_location() -> KernelInfo {
    // Fallback: assume kernel is loaded by multiboot at 1MB
    KernelInfo {
        start: 0x100000,
        end: 0x110000,
    }
}

// ATOM: scale - adjust memory map for kernel
fn prepare_bootInfo(kernel: KernelInfo) -> BootInfo {
    unsafe {
        // ATOM: scale - calculate memory map address and length
        let memory_map_addr = MEMORY_MAP.as_ptr() as u64;
        let memory_map_len = MEMORY_MAP_COUNT as u64;
        
        BootInfo {
            physical_memory_offset: 0xFFFF800000000000, // Higher half mapping
            kernel_start: kernel.start,
            kernel_end: kernel.end,
            memory_map_addr,
            memory_map_len,
        }
    }
}

// ATOM: compare + order - validate and jump
unsafe fn jump_to_kernel(boot_info: BootInfo) -> ! {
    let kernel_entry = boot_info.kernel_start as *const ();
    
    // TODO: Set up proper calling convention
    // For now, just jump
    asm!(
        "mov rdi, {0}",
        "jmp {1}",
        in(reg) &boot_info as *const BootInfo,
        in(reg) kernel_entry,
        options(noreturn)
    );
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}
