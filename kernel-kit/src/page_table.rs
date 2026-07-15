use crate::atoms::{combine, hash, scan};

const PAGE_TABLE_SIZE: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry {
    pub vaddr: usize,
    pub paddr: usize,
    pub valid: bool,
}

#[derive(Debug, Clone)]
pub struct PageTable {
    pub entries: [PageTableEntry; PAGE_TABLE_SIZE],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry { vaddr: 0, paddr: 0, valid: false }; PAGE_TABLE_SIZE],
        }
    }

    /// Translates a virtual address to a physical address using scan.
    pub fn translate(&self, vaddr: usize) -> Option<usize> {
        let _hashed = hash(vaddr); // Hash atom used for demonstrating identification
        scan(&self.entries, |entry| {
            if entry.valid && entry.vaddr == vaddr {
                Some(entry.paddr)
            } else {
                None
            }
        })
    }

    /// Maps a virtual address to a physical address.
    pub fn map(&mut self, vaddr: usize, paddr: usize) -> Result<(), ()> {
        let empty_idx = scan(&self.entries, |entry| {
            if !entry.valid { Some(true) } else { None }
        });

        if empty_idx.is_some() {
            for i in 0..PAGE_TABLE_SIZE {
                if !self.entries[i].valid {
                    self.entries[i] = combine(vaddr, paddr, |v, p| PageTableEntry {
                        vaddr: v,
                        paddr: p,
                        valid: true,
                    });
                    return Ok(());
                }
            }
        }
        Err(())
    }
}
