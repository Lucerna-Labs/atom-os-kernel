//! Linear-framebuffer display through the Bochs/QEMU "BGA" interface
//! (PCI 1234:1111, as presented by `-vga std`). Mode setting uses the
//! VBE DISPI index/data ports; the framebuffer is PCI BAR0.
use crate::io::Port;

const VENDOR_DEVICE: u32 = 0x1111_1234;
const INDEX: u16 = 0x1ce;
const DATA: u16 = 0x1cf;
const XRES: u16 = 1;
const YRES: u16 = 2;
const BPP: u16 = 3;
const ENABLE: u16 = 4;
const VIRT_WIDTH: u16 = 6;
const ENABLED: u16 = 1;
const LFB: u16 = 0x40;

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 768;
pub const BYTES_PER_PIXEL: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct Framebuffer { pub phys: u64, pub width: u32, pub height: u32, pub pitch: u32 }

fn write(register: u16, value: u16) { Port::new(INDEX).write16(register); Port::new(DATA).write16(value); }
fn read(register: u16) -> u16 { Port::new(INDEX).write16(register); Port::new(DATA).read16() }

/// Physical address of the framebuffer, if a BGA-compatible device exists.
pub fn find() -> Option<u64> {
    for bus in 0..256 {
        for slot in 0..32 {
            if crate::virtio_blk::pci_read(bus, slot, 0, 0) & 0xffff == 0xffff { continue; }
            if crate::virtio_blk::pci_read(bus, slot, 0, 0) != VENDOR_DEVICE { continue; }
            let bar = crate::virtio_blk::pci_read(bus, slot, 0, 0x10);
            if bar & 1 != 0 { return None; }
            let command = crate::virtio_blk::pci_read(bus, slot, 0, 4) & 0xffff;
            crate::virtio_blk::pci_write(bus, slot, 0, 4, command | 2); // Memory space.
            return Some((bar & !0xf) as u64);
        }
    }
    None
}

/// Switches to WIDTH x HEIGHT x 32 with a linear framebuffer.
pub fn enable() -> Option<Framebuffer> {
    let phys = find()?;
    write(ENABLE, 0);
    write(XRES, WIDTH as u16);
    write(YRES, HEIGHT as u16);
    write(BPP, 32);
    write(VIRT_WIDTH, WIDTH as u16);
    write(ENABLE, ENABLED | LFB);
    if read(XRES) != WIDTH as u16 || read(YRES) != HEIGHT as u16 || read(BPP) != 32 {
        write(ENABLE, 0);
        return None;
    }
    Some(Framebuffer { phys, width: WIDTH, height: HEIGHT, pitch: WIDTH * BYTES_PER_PIXEL })
}

/// Returns to the legacy VGA (text) mode the console uses.
pub fn disable() {
    if find().is_some() { write(ENABLE, 0); }
}

pub fn bytes(framebuffer: &Framebuffer) -> usize { (framebuffer.pitch * framebuffer.height) as usize }
