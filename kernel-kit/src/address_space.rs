//! Private user mappings with explicit frame ownership. Kernel mappings are
//! inherited from the immutable boot root; modified table paths are copied.
use alloc::vec::Vec;
use crate::paging::{phys_to_virt, PageTable, Cr3};
use crate::memory::FRAME_ALLOCATOR;

pub const STACK_TOP: u64 = crate::elf::IMAGE_BASE;
pub const STACK_BYTES: usize = 32 * 1024;
pub const HEAP_BASE: u64 = 0x0000_7f10_0000_0000;
pub const HEAP_BYTES: usize = 1024 * 1024 * 1024;
/// Largest single heap allocation (physically contiguous frames).
pub const ALLOC_MAX: usize = 64 * 1024 * 1024;
/// Where SYS_DISPLAY_OPEN maps the framebuffer in the display owner.
pub const FRAMEBUFFER_BASE: u64 = 0x0000_7f20_0000_0000;
pub const RECV_BASE: u64 = 0x0000_7f00_0000_0000;
const ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
const NX: u64 = 1 << 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError { Memory, Address, Permission }

pub fn frames_allocate(count: usize) -> Result<u64, MapError> {
    let (pool, flags) = FRAME_ALLOCATOR.lock();
    let result = pool.alloc_contiguous(count);
    FRAME_ALLOCATOR.unlock(flags);
    let phys = result.ok_or(MapError::Memory)?;
    unsafe { core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, count * 4096); }
    Ok(phys)
}

pub fn frames_free(phys: u64, count: usize) {
    let (pool, flags) = FRAME_ALLOCATOR.lock();
    for page in 0..count { pool.free_frame(phys + page as u64 * 4096); }
    FRAME_ALLOCATOR.unlock(flags);
}

#[derive(Debug)]
struct Region { virt: u64, phys: u64, pages: usize, heap: bool, device: bool }

#[derive(Debug)]
pub struct AddressSpace {
    pub root: u64,
    tables: Vec<u64>,
    regions: Vec<Region>,
}

impl AddressSpace {
    pub fn new(kernel_root: u64) -> Result<Self, MapError> {
        let root = frames_allocate(1)?;
        unsafe { core::ptr::copy_nonoverlapping(phys_to_virt(kernel_root) as *const u8,
                                              phys_to_virt(root) as *mut u8, 4096); }
        Ok(Self { root, tables: alloc::vec![root], regions: Vec::new() })
    }

    fn private_child(&mut self, parent: u64, index: usize) -> Result<u64, MapError> {
        unsafe {
            let table = &mut *(phys_to_virt(parent) as *mut PageTable);
            let entry = table.entries[index].0;
            if entry & 128 != 0 { return Err(MapError::Address); }
            let existing = entry & ADDR_MASK;
            if entry & 1 != 0 && self.tables.contains(&existing) { return Ok(existing); }
            let child = frames_allocate(1)?;
            if entry & 1 != 0 {
                core::ptr::copy_nonoverlapping(phys_to_virt(existing) as *const u8,
                                              phys_to_virt(child) as *mut u8, 4096);
            }
            self.tables.push(child);
            table.entries[index].0 = child | 7;
            Ok(child)
        }
    }

    pub fn map_page(&mut self, virt: u64, phys: u64, writable: bool, executable: bool) -> Result<(), MapError> {
        if !canonical(virt) || virt & 4095 != 0 || phys & 4095 != 0 || (writable && executable) {
            return Err(MapError::Permission);
        }
        let mut table = self.root;
        for shift in [39, 30, 21] { table = self.private_child(table, ((virt >> shift) & 511) as usize)?; }
        unsafe {
            let leaf = &mut *(phys_to_virt(table) as *mut PageTable);
            leaf.entries[((virt >> 12) & 511) as usize].0 = phys | 5 | if writable { 2 } else { 0 }
                | if executable { 0 } else { NX };
        }
        Ok(())
    }

    pub fn allocate_region(&mut self, virt: u64, pages: usize, heap: bool) -> Result<u64, MapError> {
        if pages == 0 || pages > ALLOC_MAX / 4096 { return Err(MapError::Memory); }
        let end = virt.checked_add(pages as u64 * 4096).ok_or(MapError::Address)?;
        if self.regions.iter().any(|r| virt < r.virt + r.pages as u64 * 4096 && r.virt < end) {
            return Err(MapError::Address);
        }
        let phys = frames_allocate(pages)?;
        // Record ownership before mapping so any partial failure is reclaimed.
        self.regions.push(Region { virt, phys, pages, heap, device: false });
        for p in 0..pages {
            if let Err(error) = self.map_page(virt + p as u64 * 4096, phys + p as u64 * 4096, true, false) {
                self.remove_region(virt);
                return Err(error);
            }
        }
        Ok(phys)
    }

    pub fn user_alloc(&mut self, bytes: usize) -> Result<u64, MapError> {
        self.user_alloc_aligned(bytes, 4096)
    }
    pub fn user_alloc_aligned(&mut self, bytes: usize, alignment: usize) -> Result<u64, MapError> {
        if !alignment.is_power_of_two() || alignment > 1024 * 1024 { return Err(MapError::Address); }
        let alignment = alignment.max(4096) as u64;
        if bytes == 0 || bytes > ALLOC_MAX { return Err(MapError::Memory); }
        let pages = (bytes + 4095) / 4096;
        let mut virt = HEAP_BASE;
        loop {
            virt = (virt + alignment - 1) & !(alignment - 1);
            let end = virt + pages as u64 * 4096;
            if end > HEAP_BASE + HEAP_BYTES as u64 { return Err(MapError::Memory); }
            if let Some(r) = self.regions.iter().find(|r| virt < r.virt + r.pages as u64 * 4096 && r.virt < end) {
                virt = r.virt + r.pages as u64 * 4096;
            } else { break; }
        }
        self.allocate_region(virt, pages, true)?;
        Ok(virt)
    }

    /// Maps device memory (not owned, never freed) at `virt`, writable and
    /// non-executable. Unmapped again by `unmap_device` or when the space drops.
    pub fn map_device(&mut self, virt: u64, phys: u64, bytes: usize) -> Result<(), MapError> {
        let pages = bytes.div_ceil(4096);
        let end = virt.checked_add(pages as u64 * 4096).ok_or(MapError::Address)?;
        if pages == 0 || phys & 4095 != 0 || self.regions.iter().any(|r| virt < r.virt + r.pages as u64 * 4096 && r.virt < end) {
            return Err(MapError::Address);
        }
        self.regions.push(Region { virt, phys, pages, heap: false, device: true });
        for page in 0..pages {
            if let Err(error) = self.map_page(virt + page as u64 * 4096, phys + page as u64 * 4096, true, false) {
                self.remove_region(virt);
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn unmap_device(&mut self, virt: u64) -> bool {
        if !self.regions.iter().any(|r| r.virt == virt && r.device) { return false; }
        self.remove_region(virt);
        true
    }

    pub fn user_free(&mut self, virt: u64) -> bool {
        if !self.regions.iter().any(|r| r.virt == virt && r.heap) { return false; }
        self.remove_region(virt);
        true
    }

    fn remove_region(&mut self, virt: u64) {
        if let Some(index) = self.regions.iter().position(|r| r.virt == virt) {
            let r = self.regions.remove(index);
            for page in 0..r.pages {
                let address = r.virt + page as u64 * 4096;
                let mut table = self.root;
                let mut present = true;
                for shift in [39, 30, 21] {
                    let entry = unsafe { (*(phys_to_virt(table) as *const PageTable)).entries[((address >> shift) & 511) as usize].0 };
                    if entry & 1 == 0 || entry & 128 != 0 { present = false; break; }
                    table = entry & ADDR_MASK;
                }
                if present {
                    unsafe { (*(phys_to_virt(table) as *mut PageTable)).entries[((address >> 12) & 511) as usize].0 = 0; }
                }
            }
            if Cr3::read() == self.root { unsafe { Cr3::load(self.root); } }
            if !r.device { frames_free(r.phys, r.pages); }
        }
    }

    pub fn translate_user(&self, virt: u64, writable: bool) -> Option<u64> {
        if !canonical(virt) { return None; }
        let mut table = self.root;
        for shift in [39, 30, 21, 12] {
            let entry = unsafe { (*(phys_to_virt(table) as *const PageTable)).entries[((virt >> shift) & 511) as usize].0 };
            if entry & 5 != 5 || (writable && entry & 2 == 0) { return None; }
            // User mappings are deliberately 4 KiB pages. Reject inherited huge pages.
            if shift != 12 && entry & 128 != 0 { return None; }
            table = entry & ADDR_MASK;
        }
        Some(table | (virt & 4095))
    }

    pub fn valid_user_range(&self, virt: u64, len: usize, writable: bool) -> bool {
        if len == 0 { return canonical(virt); }
        let Some(end) = virt.checked_add(len as u64 - 1) else { return false; };
        if !canonical(virt) || !canonical(end) { return false; }
        let mut page = virt & !4095;
        loop {
            if self.translate_user(page, writable).is_none() { return false; }
            if page >= (end & !4095) { return true; }
            page += 4096;
        }
    }

    pub fn frame_count(&self) -> usize { self.tables.len() + self.regions.iter().filter(|r| !r.device).map(|r| r.pages).sum::<usize>() }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        assert_ne!(Cr3::read(), self.root, "cannot reclaim the active address space");
        for r in self.regions.iter().filter(|r| !r.device) { frames_free(r.phys, r.pages); }
        for &table in &self.tables { frames_free(table, 1); }
    }
}

pub fn canonical(address: u64) -> bool {
    let high = address >> 48;
    if address & (1 << 47) != 0 { high == 0xffff } else { high == 0 }
}
