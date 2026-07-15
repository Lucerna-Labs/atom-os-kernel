//! Bare-metal x86_64 4-level Paging mechanism.
use crate::atoms::{project, combine};
use core::arch::asm;

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
        // Project the physical address into the CR3 register using inline assembly
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
    use core::alloc::Layout;
    use alloc::alloc::alloc;
    
    unsafe {
        let layout = Layout::from_size_align(PAGE_SIZE as usize, PAGE_SIZE as usize).unwrap();
        let new_pml4_ptr = alloc(layout);
        if new_pml4_ptr.is_null() {
            return None;
        }
        
        let new_pml4_phys = new_pml4_ptr as u64;

        // Copy the active CR3 into the new CR3
        core::ptr::copy_nonoverlapping(
            active_cr3 as *const PageTable,
            new_pml4_phys as *mut PageTable,
            1
        );
        
        // Clear the upper half (user space, 256 to 511)
        let new_table = &mut *(new_pml4_phys as *mut PageTable);
        for i in 256..512 {
            new_table.entries[i] = PageTableEntry::empty();
        }
        
        Some(new_pml4_phys)
    }
}

/// Maps a contiguous virtual slice to a contiguous physical slice in the given PML4.
pub fn map_segment(pml4_phys: u64, vaddr: u64, paddr: u64, size: usize) {
    use core::alloc::Layout;
    use alloc::alloc::alloc;
    
    let mut current_vaddr = vaddr & !(PAGE_SIZE as u64 - 1);
    let end_vaddr = (vaddr + size as u64 + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
    let mut current_paddr = paddr & !(PAGE_SIZE as u64 - 1);

    unsafe {
        let layout = Layout::from_size_align(PAGE_SIZE as usize, PAGE_SIZE as usize).unwrap();
        
        while current_vaddr < end_vaddr {
            let va = VirtualAddress(current_vaddr);
            
            let pml4 = &mut *(pml4_phys as *mut PageTable);
            let p4_entry = pml4.entries[va.p4_index()];
            
            let pdpt_phys = if (p4_entry.0 & 1) == 0 {
                let phys = alloc(layout);
                if phys.is_null() { panic!("OOM"); }
                core::ptr::write_bytes(phys, 0, PAGE_SIZE as usize);
                pml4.entries[va.p4_index()].0 = (phys as u64) | 0x7; // Present | Writable | User
                phys as u64
            } else {
                p4_entry.0 & 0x000FFFFFFFFFF000
            };

            let pdpt = &mut *(pdpt_phys as *mut PageTable);
            let p3_entry = pdpt.entries[va.p3_index()];

            let pd_phys = if (p3_entry.0 & 1) == 0 {
                let phys = alloc(layout);
                if phys.is_null() { panic!("OOM"); }
                core::ptr::write_bytes(phys, 0, PAGE_SIZE as usize);
                pdpt.entries[va.p3_index()].0 = (phys as u64) | 0x7;
                phys as u64
            } else {
                p3_entry.0 & 0x000FFFFFFFFFF000
            };

            let pd = &mut *(pd_phys as *mut PageTable);
            let p2_entry = pd.entries[va.p2_index()];

            let pt_phys = if (p2_entry.0 & 1) == 0 {
                let phys = alloc(layout);
                if phys.is_null() { panic!("OOM"); }
                core::ptr::write_bytes(phys, 0, PAGE_SIZE as usize);
                pd.entries[va.p2_index()].0 = (phys as u64) | 0x7;
                phys as u64
            } else {
                p2_entry.0 & 0x000FFFFFFFFFF000
            };

            let pt = &mut *(pt_phys as *mut PageTable);
            pt.entries[va.p1_index()].0 = current_paddr | 0x7; // Present | Writable | User

            current_vaddr += PAGE_SIZE as u64;
            current_paddr += PAGE_SIZE as u64;
        }
    }
}
