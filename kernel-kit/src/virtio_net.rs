//! Legacy/transitional virtio PCI network transport, polled.
//! Split-ring layout follows OASIS Virtio 1.2 sections 2.7 and 5.1,
//! restricted to the legacy 10-byte virtio_net_hdr (no merged rxbuf,
//! no status, no control queue — the minimum honest wire).
//!
//! House pattern (see virtio_blk): the device is configured once at
//! discover(), queue rings and buffers live in DMA frames from the
//! frame allocator, and completion is POLLED — the kernel feels the
//! wire on its own heartbeat (timer tick), never on the device's.
//! An interrupt-less NIC is also an attack-surface decision: no new
//! IDT entry, no surprise control flow; packets exist when the
//! kernel looks for them.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::address_space::{frames_allocate, frames_free};
use crate::io::Port;
use crate::paging::phys_to_virt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError { Missing, Unsupported, Memory, Timeout, Io, Offline }

/// Legacy virtio_net_hdr (pre-1.0 layout; no merge flags in use).
pub const NET_HDR: usize = 10;
/// Receive data buffer: MTU 1500 + VLAN + headroom.
pub const RX_BUF: usize = 1600;
/// Posted receive chains (each = hdr page + data page).
const RX_DEPTH: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct Descriptor { address: u64, length: u32, flags: u16, next: u16 }

fn pci_read(bus: u32, slot: u32, function: u32, offset: u32) -> u32 {
    Port::new(0xcf8).write32(0x80000000 | bus << 16 | slot << 11 | function << 8 | (offset & 0xfc));
    Port::new(0xcfc).read32()
}
fn pci_write(bus: u32, slot: u32, function: u32, offset: u32, value: u32) {
    Port::new(0xcf8).write32(0x80000000 | bus << 16 | slot << 11 | function << 8 | (offset & 0xfc));
    Port::new(0xcfc).write32(value);
}

fn queue_used_offset(size: usize) -> usize {
    (size * 16 + 6 + size * 2 + 4095) & !4095
}
fn queue_frames(size: usize, used_offset: usize) -> usize {
    (used_offset + 6 + size * 8 + 4095) / 4096
}

struct Queue {
    frame: u64,
    size: usize,
    used_offset: usize,
    /// Total descriptor chains made available (avail cursor).
    posted: u16,
    /// Total used-ring entries consumed.
    consumed: u16,
}

/// Configure the selected virtqueue and return its state. The queue
/// PFN write is the legacy "activate" — after it, the device owns
/// the ring.
fn setup_queue(io: u16, size: usize) -> Result<Queue, NetError> {
    let used_offset = queue_used_offset(size);
    let frame = frames_allocate(queue_frames(size, used_offset)).map_err(|_| NetError::Memory)?;
    unsafe {
        // Disable queue interrupts: this driver polls.
        write_volatile((phys_to_virt(frame) + size as u64 * 16) as *mut u16, 1);
    }
    Port::new(io + 8).write32((frame / 4096) as u32);
    Ok(Queue { frame, size, used_offset, posted: 0, consumed: 0 })
}

pub struct VirtioNet {
    io: u16,
    mac: [u8; 6],
    rx: Queue,
    tx: Queue,
    /// Receive chain memory: [hdr page | data page] per chain.
    rx_chain: [u64; RX_DEPTH],
    tx_frame: u64,
    online: bool,
}

impl VirtioNet {
    pub fn is_online(&self) -> bool { self.online }
    pub fn mac(&self) -> [u8; 6] { self.mac }

    pub fn discover() -> Result<Self, NetError> {
        for bus in 0..256 {
            for slot in 0..32 {
                if pci_read(bus, slot, 0, 0) & 0xffff == 0xffff { continue; }
                for function in 0..8 {
                    if pci_read(bus, slot, function, 0) != 0x1000_1af4 { continue; }
                    let bar = pci_read(bus, slot, function, 0x10);
                    if bar & 1 == 0 || bar & !0xffff != 0 { return Err(NetError::Unsupported); }
                    let io = (bar & !3) as u16;
                    let command = pci_read(bus, slot, function, 4) & 0xffff;
                    pci_write(bus, slot, function, 4, command | 5); // I/O + bus mastering.
                    // Reset, acknowledge, driver — the proven block sequence.
                    Port::new(io + 18).write(0);
                    Port::new(io + 18).write(1);
                    Port::new(io + 18).write(3);
                    // Negotiate ONLY VIRTIO_NET_F_MAC (bit 5): no merged
                    // rx buffers, no status byte, no control queue.
                    Port::new(io + 4).write32(1 << 5);
                    let mut mac = [0u8; 6];
                    for (i, byte) in mac.iter_mut().enumerate() {
                        // Net config begins right after the legacy header.
                        *byte = Port::new(io + 20 + i as u16).read() as u8;
                    }
                    // Queue 0: receive.
                    Port::new(io + 14).write16(0);
                    let rsize = Port::new(io + 12).read16() as usize;
                    if rsize < RX_DEPTH || !rsize.is_power_of_two() { return Err(NetError::Unsupported); }
                    let rx = setup_queue(io, rsize)?;
                    // Queue 1: transmit.
                    Port::new(io + 14).write16(1);
                    let tsize = Port::new(io + 12).read16() as usize;
                    if tsize == 0 || !tsize.is_power_of_two() { return Err(NetError::Unsupported); }
                    let tx = setup_queue(io, tsize)?;
                    // Two pages per receive chain: hdr + data.
                    let mut rx_chain = [0u64; RX_DEPTH];
                    for slot in rx_chain.iter_mut() {
                        *slot = frames_allocate(2).map_err(|_| NetError::Memory)?;
                    }
                    let tx_frame = frames_allocate(1).map_err(|_| NetError::Memory)?;
                    Port::new(io + 18).write(7); // RUNNING.
                    let mut net = Self { io, mac, rx, tx, rx_chain, tx_frame, online: true };
                    net.post_all_rx();
                    return Ok(net);
                }
            }
        }
        Err(NetError::Missing)
    }

    /// Post one receive chain: hdr (writable) -> data (writable).
    fn post_chain(&mut self, chain: u16) {
        let descs = phys_to_virt(self.rx.frame) as *mut Descriptor;
        let avail = (phys_to_virt(self.rx.frame) + self.rx.size as u64 * 16) as *mut u16;
        let hdr = self.rx_chain[chain as usize];
        let data = hdr + 4096;
        unsafe {
            write_volatile(descs.add(chain as usize * 2), Descriptor {
                address: hdr, length: NET_HDR as u32, flags: 2 /* write */ | 1 /* next */, next: chain * 2 + 1 });
            write_volatile(descs.add(chain as usize * 2 + 1), Descriptor {
                address: data, length: RX_BUF as u32, flags: 2, next: 0 });
            write_volatile(avail.add(2 + (self.rx.posted as usize % self.rx.size)), chain * 2);
            self.rx.posted = self.rx.posted.wrapping_add(1);
        }
    }

    fn kick_rx(&self) {
        let avail = (phys_to_virt(self.rx.frame) + self.rx.size as u64 * 16) as *mut u16;
        unsafe {
            fence(Ordering::Release);
            write_volatile(avail.add(1), self.rx.posted);
            fence(Ordering::SeqCst);
        }
        Port::new(self.io + 16).write16(0); // notify receive queue
    }

    fn post_all_rx(&mut self) {
        for chain in 0..RX_DEPTH as u16 {
            self.post_chain(chain);
        }
        self.kick_rx();
    }

    /// Poll the receive used ring. Each completed chain's packet is
    /// handed to `sink` (hdr stripped) and its buffer reposted.
    /// Returns the number of packets delivered.
    pub fn poll_rx(&mut self, mut sink: impl FnMut(&[u8])) -> usize {
        if !self.online { return 0; }
        let used = phys_to_virt(self.rx.frame) + self.rx.used_offset as u64;
        let mut received = 0usize;
        loop {
            let used_idx = unsafe { read_volatile((used + 2) as *const u16) };
            if used_idx == self.rx.consumed { break; } // drained
            let slot = (self.rx.consumed % self.rx.size as u16) as usize;
            let entry = used as *const u8;
            let head = unsafe { read_volatile(entry.add(4 + slot * 8) as *const u32) } as u16;
            let written = unsafe { read_volatile(entry.add(4 + slot * 8 + 4) as *const u32) } as usize;
            self.rx.consumed = self.rx.consumed.wrapping_add(1);
            let chain = head / 2;
            if (chain as usize) < RX_DEPTH {
                let data = self.rx_chain[chain as usize] + 4096;
                let packet_len = written.saturating_sub(NET_HDR).min(RX_BUF);
                if packet_len > 0 {
                    unsafe {
                        let bytes = core::slice::from_raw_parts(phys_to_virt(data) as *const u8, packet_len);
                        sink(bytes);
                    }
                    received += 1;
                }
                self.post_chain(chain);
            }
        }
        if received > 0 {
            self.kick_rx();
            let _ = Port::new(self.io + 19).read(); // clear ISR
        }
        received
    }

    /// Transmit one packet (legacy hdr prefix + payload), polling for
    /// completion exactly like the block transport's single flight.
    pub fn transmit(&mut self, packet: &[u8]) -> Result<(), NetError> {
        if !self.online { return Err(NetError::Offline); }
        if packet.len() > RX_BUF - NET_HDR { return Err(NetError::Io); }
        unsafe {
            let frame = phys_to_virt(self.tx_frame) as *mut u8;
            core::ptr::write_bytes(frame, 0, NET_HDR); // zeroed legacy hdr
            core::ptr::copy_nonoverlapping(packet.as_ptr(), frame.add(NET_HDR), packet.len());
            let descs = phys_to_virt(self.tx.frame) as *mut Descriptor;
            write_volatile(descs, Descriptor {
                address: self.tx_frame, length: NET_HDR as u32, flags: 1 /* next */, next: 1 });
            write_volatile(descs.add(1), Descriptor {
                address: self.tx_frame + NET_HDR as u64, length: packet.len() as u32, flags: 0, next: 0 });
            let avail = (phys_to_virt(self.tx.frame) + self.tx.size as u64 * 16) as *mut u16;
            write_volatile(avail.add(2), 0);
            fence(Ordering::Release);
            self.tx.posted = self.tx.posted.wrapping_add(1);
            write_volatile(avail.add(1), self.tx.posted);
            fence(Ordering::SeqCst);
            Port::new(self.io + 16).write16(1); // notify transmit queue
            let used = phys_to_virt(self.tx.frame) + self.tx.used_offset as u64;
            for _ in 0..100_000_000 {
                if read_volatile((used + 2) as *const u16) == self.tx.posted { break; }
                core::hint::spin_loop();
            }
            let done = read_volatile((used + 2) as *const u16);
            if done != self.tx.posted {
                self.online = false;
                return Err(NetError::Timeout);
            }
            self.tx.consumed = self.tx.consumed.wrapping_add(1);
            fence(Ordering::Acquire);
            let _ = Port::new(self.io + 19).read();
        }
        Ok(())
    }
}

impl Drop for VirtioNet {
    fn drop(&mut self) {
        self.online = false;
        unsafe {
            frames_free(self.rx.frame, queue_frames(self.rx.size, self.rx.used_offset));
            frames_free(self.tx.frame, queue_frames(self.tx.size, self.tx.used_offset));
            for chain in self.rx_chain.iter() {
                frames_free(*chain, 2);
            }
            frames_free(self.tx_frame, 1);
        }
    }
}
