//! Legacy/transitional virtio PCI block transport, single in-flight request.
//! Split-ring layout and FLUSH follow OASIS Virtio 1.2, sections 2.7, 4.1, 5.2.
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::address_space::{frames_allocate, frames_free};
use crate::io::Port;
use crate::paging::phys_to_virt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskError { Missing, Unsupported, Memory, Bounds, Timeout, Io, Corrupt, Full }

pub trait BlockDevice {
    fn sectors(&self) -> u64;
    fn read_sector(&mut self, sector: u64, bytes: &mut [u8; 512]) -> Result<(), DiskError>;
    fn write_sector(&mut self, sector: u64, bytes: &[u8; 512]) -> Result<(), DiskError>;
    fn flush(&mut self) -> Result<(), DiskError>;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Descriptor { address: u64, length: u32, flags: u16, next: u16 }

pub struct VirtioBlock {
    io: u16, size: u16, queue: u64, used_offset: usize, request: u64,
    available: u16, consumed: u16, capacity: u64, online: bool,
}

pub fn pci_read(bus: u32, slot: u32, function: u32, offset: u32) -> u32 {
    Port::new(0xcf8).write32(0x80000000 | bus << 16 | slot << 11 | function << 8 | (offset & 0xfc));
    Port::new(0xcfc).read32()
}
pub fn pci_write(bus: u32, slot: u32, function: u32, offset: u32, value: u32) {
    Port::new(0xcf8).write32(0x80000000 | bus << 16 | slot << 11 | function << 8 | (offset & 0xfc));
    Port::new(0xcfc).write32(value);
}

impl VirtioBlock {
    pub fn discover() -> Result<Self, DiskError> {
        for bus in 0..256 {
            for slot in 0..32 {
                if pci_read(bus, slot, 0, 0) & 0xffff == 0xffff { continue; }
                for function in 0..8 {
                    if pci_read(bus, slot, function, 0) != 0x1001_1af4 { continue; }
                    let bar = pci_read(bus, slot, function, 0x10);
                    if bar & 1 == 0 || bar & !0xffff != 0 { return Err(DiskError::Unsupported); }
                    let io = (bar & !3) as u16;
                    let command = pci_read(bus, slot, function, 4) & 0xffff;
                    pci_write(bus, slot, function, 4, command | 5); // I/O + bus mastering.
                    Port::new(io + 18).write(0);
                    Port::new(io + 18).write(1);
                    Port::new(io + 18).write(3);
                    let offered = Port::new(io).read32();
                    if offered & (1 << 9) == 0 || offered & (1 << 5) != 0 {
                        Port::new(io + 18).write(128); return Err(DiskError::Unsupported);
                    }
                    Port::new(io + 4).write32(1 << 9); // Negotiate and actually use FLUSH.
                    Port::new(io + 14).write16(0);
                    let size = Port::new(io + 12).read16();
                    if size < 3 || size > 1024 || !size.is_power_of_two() || Port::new(io + 8).read32() != 0 {
                        Port::new(io + 18).write(128); return Err(DiskError::Unsupported);
                    }
                    let used_offset = (size as usize * 16 + 6 + size as usize * 2 + 4095) & !4095;
                    let pages = (used_offset + 6 + size as usize * 8 + 4095) / 4096;
                    let queue = frames_allocate(pages).map_err(|_| DiskError::Memory)?;
                    let request = match frames_allocate(1) { Ok(p) => p, Err(_) => {
                        frames_free(queue, pages); return Err(DiskError::Memory);
                    }};
                    // No queue interrupts: the one outstanding request is polled.
                    unsafe { write_volatile((phys_to_virt(queue) + size as u64 * 16) as *mut u16, 1); }
                    Port::new(io + 8).write32((queue / 4096) as u32);
                    let capacity = Port::new(io + 20).read32() as u64 | (Port::new(io + 24).read32() as u64) << 32;
                    Port::new(io + 18).write(7);
                    return Ok(Self { io, size, queue, used_offset, request, available: 0, consumed: 0, capacity, online: true });
                }
            }
        }
        Err(DiskError::Missing)
    }

    fn submit(&mut self, kind: u32, sector: u64, bytes: &mut [u8; 512]) -> Result<(), DiskError> {
        if !self.online { return Err(DiskError::Io); }
        if kind != 4 && sector >= self.capacity { return Err(DiskError::Bounds); }
        unsafe {
            let request = phys_to_virt(self.request) as *mut u8;
            write_volatile(request as *mut u32, kind);
            write_volatile(request.add(4) as *mut u32, 0);
            write_volatile(request.add(8) as *mut u64, sector);
            if kind == 1 { core::ptr::copy_nonoverlapping(bytes.as_ptr(), request.add(512), 512); }
            write_volatile(request.add(1024), 0xff);
            let descriptors = phys_to_virt(self.queue) as *mut Descriptor;
            write_volatile(descriptors, Descriptor { address: self.request, length: 16, flags: 1, next: if kind == 4 { 2 } else { 1 } });
            write_volatile(descriptors.add(1), Descriptor { address: self.request + 512, length: 512, flags: 1 | if kind == 0 { 2 } else { 0 }, next: 2 });
            write_volatile(descriptors.add(2), Descriptor { address: self.request + 1024, length: 1, flags: 2, next: 0 });
            let avail = (phys_to_virt(self.queue) + self.size as u64 * 16) as *mut u16;
            write_volatile(avail.add(2 + (self.available % self.size) as usize), 0);
            fence(Ordering::Release);
            self.available = self.available.wrapping_add(1);
            write_volatile(avail.add(1), self.available);
            fence(Ordering::SeqCst);
            Port::new(self.io + 16).write16(0);
            let used = (phys_to_virt(self.queue) + self.used_offset as u64) as *const u16;
            let mut completed = false;
            for _ in 0..100_000_000 {
                if read_volatile(used.add(1)) != self.consumed { completed = true; break; }
                core::hint::spin_loop();
            }
            if !completed { self.online = false; return Err(DiskError::Timeout); }
            fence(Ordering::Acquire);
            let head = read_volatile((used as *const u8).add(4 + (self.consumed % self.size) as usize * 8) as *const u32);
            self.consumed = self.consumed.wrapping_add(1);
            let _ = Port::new(self.io + 19).read();
            if head != 0 || read_volatile(request.add(1024)) != 0 { self.online = false; return Err(DiskError::Io); }
            if kind == 0 { core::ptr::copy_nonoverlapping(request.add(512), bytes.as_mut_ptr(), 512); }
        }
        Ok(())
    }
}

impl BlockDevice for VirtioBlock {
    fn sectors(&self) -> u64 { self.capacity }
    fn read_sector(&mut self, sector: u64, bytes: &mut [u8; 512]) -> Result<(), DiskError> { self.submit(0, sector, bytes) }
    fn write_sector(&mut self, sector: u64, bytes: &[u8; 512]) -> Result<(), DiskError> {
        let mut copy = *bytes;
        self.submit(1, sector, &mut copy)
    }
    fn flush(&mut self) -> Result<(), DiskError> { self.submit(4, 0, &mut [0; 512]) }
}
