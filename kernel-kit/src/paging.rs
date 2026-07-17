//! Bare-metal x86_64 4-level Paging mechanism.
use crate::atoms::{project, combine};
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

/// The virtual address offset at which the bootloader maps all physical memory.
/// Set once from `_start` via `set_phys_offset()` using BootInfo.
///
/// Physical addresses convert to virtual by adding this offset. Direct identity
/// mapping (treating phys as virt) is WRONG — the bootloader maps all RAM at
/// this offset, not at physical addresses. Without the translation, page-table
/// copies go to wrong physical memory and CR3 switches triple-fault.
pub static PHYS_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Set the physical→virtual offset. Called once from `_start` (main.rs).
pub fn set_phys_offset(offset: u64) {
    PHYS_OFFSET.store(offset, Ordering::SeqCst);
}

/// Convert a physical address to the virtual address at which the bootloader
/// mapped it. Returns `phys` unchanged if no offset has been set yet (early
/// boot, before _start reads BootInfo).
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + PHYS_OFFSET.load(Ordering::SeqCst)
}

/// Inverse of phys_to_virt: convert a virtual address (in the bootloader's
/// physical-memory mapping) back to its physical address. Use this when a
/// kernel-heap pointer must be encoded into a page-table entry as a physical
/// frame number.
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - PHYS_OFFSET.load(Ordering::SeqCst)
}
/// Walk the 4-level page table rooted at `cr3_phys` for `vaddr`. Returns
/// (leaf_pte_raw, level): 0=4K leaf, 1=2M huge, 2=1G huge, 3=PML4 absent,
/// 4=intermediate absent. Diagnostic only.


pub unsafe fn walk(cr3_phys: u64, vaddr: u64) -> (u64, u8) {
    let va = VirtualAddress(vaddr);
    let pml4 = &*(phys_to_virt(cr3_phys) as *const PageTable);
    let p4 = pml4.entries[va.p4_index()].0;
    if p4 & 1 == 0 { return (p4, 3); }
    let pdpt = &*(phys_to_virt(p4 & 0x000FFFFFFFFFF000) as *const PageTable);
    let p3 = pdpt.entries[va.p3_index()].0;
    if p3 & 1 == 0 { return (p3, 4); }
    if p3 & (1u64 << 7) != 0 { return (p3, 2); }
    let pd = &*(phys_to_virt(p3 & 0x000FFFFFFFFFF000) as *const PageTable);
    let p2 = pd.entries[va.p2_index()].0;
    if p2 & 1 == 0 { return (p2, 4); }
    if p2 & (1u64 << 7) != 0 { return (p2, 1); }
    let pt = &*(phys_to_virt(p2 & 0x000FFFFFFFFFF000) as *const PageTable);
    (pt.entries[va.p1_index()].0, 0)
}


pub const PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn set_address(&mut self, addr: u64, flags: u64) {
        // Compose from the outside: The MMU expects the physical address and flags combined.
        // We use the combine atom to merge the physical address with the control flags.
        self.0 = combine(addr, flags, |a, f| (a & !0xFFF) | (f & 0xFFF));
    }

    pub fn is_present(&self) -> bool {
        (self.0 & 0x1) != 0
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }
    
    pub fn zero(&mut self) {
        for i in 0..512 {
            self.entries[i] = PageTableEntry::empty();
        }
    }
}

/// The CR3 primitive acts as a mathematical projection between CPU and memory.
pub struct Cr3;

impl Cr3 {
    /// Projects a physical address into the CR3 register to activate a page table hierarchy.
    pub unsafe fn load(phys_addr: u64) {
        project(phys_addr, |addr| {
            unsafe {
                asm!("mov cr3, {}", in(reg) addr, options(nostack, preserves_flags));
            }
        });
    }

    /// Reads the current physical address of the active page table from CR3.
    pub fn read() -> u64 {
        let mut addr: u64;
        unsafe {
            asm!("mov {}, cr3", out(reg) addr, options(nomem, nostack, preserves_flags));
        }
        addr & 0x000FFFFFFFFFF000
    }
}

pub struct VirtualAddress(pub u64);

impl VirtualAddress {
    pub fn p4_index(&self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    pub fn p3_index(&self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    pub fn p2_index(&self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    pub fn p1_index(&self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }
}

/// Helper to duplicate a page table.
/// Requires the active root physical address.
/// Returns the physical address of the new PML4.
pub fn duplicate_pml4(active_cr3: u64) -> Option<u64> {
    // C2 fix: use the dedicated frame allocator instead of the kernel heap.
    let (fa, sif) = crate::memory::FRAME_ALLOCATOR.lock();
    let new_pml4_phys = fa.alloc_frame()?;
    crate::memory::FRAME_ALLOCATOR.unlock(sif);

    unsafe {
        // Copy ALL 512 PML4 entries from the bootloader's PML4. Earlier per-entry
        // and per-half copies failed for a subtle reason (see NOTES); copying the
        // whole table byte-for-byte is the faithful starting point.
        // Translate physical addresses to virtual via the bootloader's offset.
        // Without this, the dereference reads/writes wrong memory and the PML4
        // copy is corrupted — the root cause of the triple-fault on CR3 switch.
        let src_virt = phys_to_virt(active_cr3);
        let dst_virt = phys_to_virt(new_pml4_phys);
        let src_pml4 = src_virt as *const PageTable;
        let new_pml4 = &mut *(dst_virt as *mut PageTable);
        core::ptr::copy_nonoverlapping(src_pml4, new_pml4, 1);
    }

    Some(new_pml4_phys)
}

/// Maps a contiguous virtual slice to a contiguous physical slice in the given PML4.
pub fn map_segment(pml4_phys: u64, vaddr: u64, paddr: u64, size: usize) {
    let mut current_vaddr = vaddr & !(PAGE_SIZE as u64 - 1);
    let end_vaddr = (vaddr + size as u64 + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
    let mut current_paddr = paddr & !(PAGE_SIZE as u64 - 1);

    unsafe {
        while current_vaddr < end_vaddr {
            let va = VirtualAddress(current_vaddr);

            let pml4 = &mut *(phys_to_virt(pml4_phys) as *mut PageTable);
            let p4_entry = pml4.entries[va.p4_index()];

            // C2 fix: intermediate page-table levels (PDPT/PD/PT) come from the
            // tracked frame allocator, not the heap, so they can never collide
            // with kernel data structures.
            let pdpt_phys = if (p4_entry.0 & 1) == 0 {
                let (fa, sif) = crate::memory::FRAME_ALLOCATOR.lock();
                let phys = fa.alloc_frame();
                crate::memory::FRAME_ALLOCATOR.unlock(sif);
                let phys = match phys { Some(p) => p, None => panic!("OOM in map_segment (pdpt)") };
                core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE as usize);
                pml4.entries[va.p4_index()].0 = phys | 0x7; // Present | Writable | User
                phys
            } else {
                p4_entry.0 & 0x000FFFFFFFFFF000
            };

            let pdpt = &mut *(phys_to_virt(pdpt_phys) as *mut PageTable);
            let p3_entry = pdpt.entries[va.p3_index()];

            let pd_phys = if (p3_entry.0 & 1) == 0 {
                let (fa, sif) = crate::memory::FRAME_ALLOCATOR.lock();
                let phys = fa.alloc_frame();
                crate::memory::FRAME_ALLOCATOR.unlock(sif);
                let phys = match phys { Some(p) => p, None => panic!("OOM in map_segment (pd)") };
                core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE as usize);
                pdpt.entries[va.p3_index()].0 = phys | 0x7;
                phys
            } else {
                p3_entry.0 & 0x000FFFFFFFFFF000
            };

            let pd = &mut *(phys_to_virt(pd_phys) as *mut PageTable);
            let p2_entry = pd.entries[va.p2_index()];

            let pt_phys = if (p2_entry.0 & 1) == 0 {
                let (fa, sif) = crate::memory::FRAME_ALLOCATOR.lock();
                let phys = fa.alloc_frame();
                crate::memory::FRAME_ALLOCATOR.unlock(sif);
                let phys = match phys { Some(p) => p, None => panic!("OOM in map_segment (pt)") };
                core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE as usize);
                pd.entries[va.p2_index()].0 = phys | 0x7;
                phys
            } else {
                p2_entry.0 & 0x000FFFFFFFFFF000
            };

            let pt = &mut *(phys_to_virt(pt_phys) as *mut PageTable);
            // Mask to physical address bits [12..51] only, then OR in flags.
            // Explicitly clear NX (bit 63) and any reserved bits so user code
            // pages are executable and don't trigger a reserved-bit #PF.
            pt.entries[va.p1_index()].0 = (current_paddr & 0x000FFFFFFFFFF000) | 0x7; // P|W|U

            current_vaddr += PAGE_SIZE as u64;
            current_paddr += PAGE_SIZE as u64;
        }
    }
}
