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

/// Modes tried in order: Full HD first, then smaller ones for adapters with less video
/// memory. The desktop reads the size it got from `DisplayInfo`.
pub const MODES: [(u32, u32); 3] = [(1920, 1080), (1280, 720), (1024, 768)];
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

/// Switches to the first mode in `MODES` the adapter accepts, 32-bit with a linear
/// framebuffer.
pub fn enable() -> Option<Framebuffer> {
    let phys = find()?;
    if read(ENABLE) & ENABLED == 0 { save_text_font(); }
    for (width, height) in MODES {
        write(ENABLE, 0);
        write(XRES, width as u16);
        write(YRES, height as u16);
        write(BPP, 32);
        write(VIRT_WIDTH, width as u16);
        write(ENABLE, ENABLED | LFB);
        if read(XRES) == width as u16 && read(YRES) == height as u16 && read(BPP) == 32 {
            return Some(Framebuffer { phys, width, height, pitch: width * BYTES_PER_PIXEL });
        }
    }
    write(ENABLE, 0);
    None
}

/// Returns to the legacy VGA (text) mode the console uses.
pub fn disable() {
    if find().is_some() {
        write(ENABLE, 0);
        restore_text_mode();
    }
}

pub fn bytes(framebuffer: &Framebuffer) -> usize { (framebuffer.pitch * framebuffer.height) as usize }

// ---------------------------------------------------------------------------
// Returning to VGA text mode. Disabling the BGA leaves the legacy VGA
// registers in their graphics configuration, and the framebuffer shares video
// memory with the text font (plane 2), so both are restored explicitly: the
// standard 80x25 mode-3 register set, then the font saved before graphics.

const MISC_MODE3: u8 = 0x67;
const SEQ_MODE3: [u8; 5] = [0x03, 0x00, 0x03, 0x00, 0x02];
const CRTC_MODE3: [u8; 25] = [0x5f, 0x4f, 0x50, 0x82, 0x55, 0x81, 0xbf, 0x1f, 0x00, 0x4f, 0x0d, 0x0e, 0x00, 0x00, 0x00, 0x50,
    0x9c, 0x0e, 0x8f, 0x28, 0x1f, 0x96, 0xb9, 0xa3, 0xff];
const GC_MODE3: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x0e, 0x00, 0xff];
const AC_MODE3: [u8; 21] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x14, 0x07, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
    0x0c, 0x00, 0x0f, 0x08, 0x00];
const FONT_BYTES: usize = 256 * 32;
static FONT: crate::memory::Spinlock<Option<[u8; FONT_BYTES]>> = crate::memory::Spinlock::new(None);

fn seq(index: u8, value: u8) { Port::new(0x3c4).write(index); Port::new(0x3c5).write(value); }
fn seq_read(index: u8) -> u8 { Port::new(0x3c4).write(index); Port::new(0x3c5).read() }
fn gc(index: u8, value: u8) { Port::new(0x3ce).write(index); Port::new(0x3cf).write(value); }
fn gc_read(index: u8) -> u8 { Port::new(0x3ce).write(index); Port::new(0x3cf).read() }

/// Runs `f` with plane 2 (the font) mapped linearly at 0xA0000.
fn with_font_plane(f: impl FnOnce(*mut u8)) {
    let saved = (seq_read(2), seq_read(4), gc_read(4), gc_read(5), gc_read(6));
    seq(2, 0x04); // Write plane 2 only.
    seq(4, 0x06); // Sequential addressing, extended memory.
    gc(4, 0x02); // Read plane 2.
    gc(5, 0x00); // No odd/even.
    gc(6, 0x04); // Map 0xA0000-0xAFFFF, no chaining.
    f(crate::paging::phys_to_virt(0xa0000) as *mut u8);
    seq(2, saved.0); seq(4, saved.1); gc(4, saved.2); gc(5, saved.3); gc(6, saved.4);
}

/// Saves the text-mode font once, before the first switch to graphics.
pub fn save_text_font() {
    let font = FONT.lock();
    if font.is_none() {
        let mut bytes = [0u8; FONT_BYTES];
        with_font_plane(|plane| unsafe { core::ptr::copy_nonoverlapping(plane, bytes.as_mut_ptr(), FONT_BYTES) });
        *font = Some(bytes);
    }
    FONT.unlock();
}

/// Reprograms 80x25 text mode and restores the saved font.
pub fn restore_text_mode() {
    Port::new(0x3c2).write(MISC_MODE3);
    for (i, &v) in SEQ_MODE3.iter().enumerate() { seq(i as u8, v); }
    // Unlock CRTC registers 0-7 before writing them.
    Port::new(0x3d4).write(0x11);
    let protect = Port::new(0x3d5).read();
    Port::new(0x3d5).write(protect & 0x7f);
    for (i, &v) in CRTC_MODE3.iter().enumerate() { Port::new(0x3d4).write(i as u8); Port::new(0x3d5).write(v); }
    for (i, &v) in GC_MODE3.iter().enumerate() { gc(i as u8, v); }
    for (i, &v) in AC_MODE3.iter().enumerate() {
        Port::new(0x3da).read(); // Reset the attribute flip-flop to the index state.
        Port::new(0x3c0).write(i as u8);
        Port::new(0x3c0).write(v);
    }
    Port::new(0x3da).read();
    Port::new(0x3c0).write(0x20); // Re-enable video output.
    let font = FONT.lock();
    if let Some(bytes) = font.as_ref() {
        with_font_plane(|plane| unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), plane, FONT_BYTES) });
    }
    FONT.unlock();
    crate::vga::VgaWriter::new().clear_screen();
}
