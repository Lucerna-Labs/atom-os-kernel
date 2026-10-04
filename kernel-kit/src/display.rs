//! Linear-framebuffer display through the Bochs/QEMU "BGA" interface
//! (PCI 1234:1111, as presented by `-vga std`). Mode setting uses the
//! VBE DISPI index/data ports; the framebuffer is PCI BAR0.
//!
//! The mode adapts to the display: the monitor's preferred resolution from its EDID
//! (QEMU exposes it in the adapter's MMIO BAR2), limited by the adapter's video memory
//! and maximum mode. Without EDID, the largest standard mode that fits is used, up to
//! Full HD.
use crate::io::Port;

const VENDOR_DEVICE: u32 = 0x1111_1234;
const INDEX: u16 = 0x1ce;
const DATA: u16 = 0x1cf;
const XRES: u16 = 1;
const YRES: u16 = 2;
const BPP: u16 = 3;
const ENABLE: u16 = 4;
const VIRT_WIDTH: u16 = 6;
const VIDEO_MEMORY_64K: u16 = 0xa;
const ENABLED: u16 = 1;
const GETCAPS: u16 = 2;
const LFB: u16 = 0x40;

/// Standard modes, largest first. Used when the EDID mode does not fit, and (up to
/// `DEFAULT_LIMIT`) when there is no EDID.
pub const MODES: [(u32, u32); 9] = [
    (6144, 3456), (5120, 2880), (3840, 2160), (2560, 1440), (1920, 1080),
    (1600, 900), (1280, 720), (1024, 768), (800, 600),
];
/// Without EDID nothing says how big the monitor is, so stay at Full HD or below.
const DEFAULT_LIMIT: (u32, u32) = (1920, 1080);
pub const BYTES_PER_PIXEL: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct Framebuffer { pub phys: u64, pub width: u32, pub height: u32, pub pitch: u32 }

fn write(register: u16, value: u16) { Port::new(INDEX).write16(register); Port::new(DATA).write16(value); }
fn read(register: u16) -> u16 { Port::new(INDEX).write16(register); Port::new(DATA).read16() }

struct Adapter { lfb: u64, mmio: Option<u64> }

fn adapter() -> Option<Adapter> {
    for bus in 0..256 {
        for slot in 0..32 {
            if crate::virtio_blk::pci_read(bus, slot, 0, 0) & 0xffff == 0xffff { continue; }
            if crate::virtio_blk::pci_read(bus, slot, 0, 0) != VENDOR_DEVICE { continue; }
            let bar = crate::virtio_blk::pci_read(bus, slot, 0, 0x10);
            if bar & 1 != 0 { return None; }
            let command = crate::virtio_blk::pci_read(bus, slot, 0, 4) & 0xffff;
            crate::virtio_blk::pci_write(bus, slot, 0, 4, command | 2); // Memory space.
            let bar2 = crate::virtio_blk::pci_read(bus, slot, 0, 0x18);
            let mmio = (bar2 & 1 == 0 && bar2 & !0xf != 0).then_some((bar2 & !0xf) as u64);
            return Some(Adapter { lfb: (bar & !0xf) as u64, mmio });
        }
    }
    None
}

/// Physical address of the framebuffer, if a BGA-compatible device exists.
pub fn find() -> Option<u64> { adapter().map(|a| a.lfb) }

/// The preferred mode from the EDID QEMU's adapter exposes at BAR2 offset 0, if it is
/// present, mapped and well formed.
fn edid_preferred(mmio: u64) -> Option<(u32, u32)> {
    let virt = crate::paging::phys_to_virt(mmio);
    // Only read through the bootloader's physical mapping if it covers this page.
    let (entry, level) = unsafe { crate::paging::walk(crate::paging::Cr3::read(), virt) };
    if level > 2 || entry & 1 == 0 { return None; }
    let mut edid = [0u8; EDID_MAX];
    for (i, byte) in edid.iter_mut().enumerate() {
        *byte = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
    }
    parse_edid(&edid)
}

/// Base block plus up to three extension blocks.
pub const EDID_MAX: usize = 512;

/// The display's preferred mode. EDID's basic timing descriptors hold at most 4095
/// pixels, so larger displays (5K, 6K) describe themselves in a DisplayID extension;
/// the order checked is the base block's first detailed timing, then DisplayID type I
/// timings (the one flagged preferred first), then CTA-861 detailed timings.
pub fn parse_edid(edid: &[u8]) -> Option<(u32, u32)> {
    let block_ok = |b: &[u8]| b.len() == 128 && b.iter().fold(0u8, |s, &x| s.wrapping_add(x)) == 0;
    if edid.len() < 128 || edid[..8] != [0, 255, 255, 255, 255, 255, 255, 0] || !block_ok(&edid[..128]) {
        return None;
    }
    let valid = |(w, h): (u32, u32)| (w >= 640 && h >= 480).then_some((w, h));
    // An 18-byte detailed timing descriptor; a zero pixel clock marks other data.
    let dtd = |d: &[u8]| {
        if d[0] == 0 && d[1] == 0 { return None; }
        valid((d[2] as u32 | (d[4] as u32 >> 4) << 8, d[5] as u32 | (d[7] as u32 >> 4) << 8))
    };
    if let Some(mode) = dtd(&edid[54..72]) { return Some(mode); }
    let extensions = (edid[126] as usize).min(edid.len() / 128 - 1);
    let blocks = || (1..=extensions).map(|i| &edid[i * 128..(i + 1) * 128]).filter(|b| block_ok(b));
    for block in blocks().filter(|b| b[0] == 0x70) {
        // DisplayID: tag, version, section bytes, product type, extension count, then
        // data blocks (tag, revision, payload length, payload).
        let end = (5 + block[2] as usize).min(127);
        let mut at = 5;
        let mut first = None;
        while at + 3 <= end {
            let (tag, len) = (block[at], block[at + 2] as usize);
            let payload = &block[(at + 3).min(end)..(at + 3 + len).min(end)];
            if tag == 0x03 {
                // Type I detailed timings, 20 bytes each; option bit 7 = preferred.
                for t in payload.chunks_exact(20) {
                    let mode = valid((u16::from_le_bytes([t[4], t[5]]) as u32 + 1, u16::from_le_bytes([t[12], t[13]]) as u32 + 1));
                    if t[3] & 0x80 != 0 && mode.is_some() { return mode; }
                    first = first.or(mode);
                }
            }
            at += 3 + len;
        }
        if first.is_some() { return first; }
    }
    for block in blocks().filter(|b| b[0] == 0x02) {
        // CTA-861: byte 2 is where the detailed timing descriptors start.
        let mut at = block[2] as usize;
        while at >= 4 && at + 18 <= 127 {
            if let Some(mode) = dtd(&block[at..at + 18]) { return Some(mode); }
            at += 18;
        }
    }
    None
}

/// The adapter's largest mode and video memory, from the DISPI capability registers.
fn limits() -> (u32, u32, u64) {
    write(ENABLE, GETCAPS);
    let (max_x, max_y) = (read(XRES) as u32, read(YRES) as u32);
    write(ENABLE, 0);
    let vram = read(VIDEO_MEMORY_64K) as u64 * 65536;
    // Older adapters report nothing: assume the classic 16 MiB and no mode limit.
    let vram = if vram == 0 { 16 << 20 } else { vram };
    (if max_x == 0 { u32::MAX } else { max_x }, if max_y == 0 { u32::MAX } else { max_y }, vram)
}

/// The modes to try, best first: the EDID preferred mode, then every standard mode
/// no larger than it (or than `DEFAULT_LIMIT` without EDID), keeping only those that
/// fit the adapter.
pub fn candidates(preferred: Option<(u32, u32)>, limits: (u32, u32, u64)) -> alloc::vec::Vec<(u32, u32)> {
    let (max_x, max_y, vram) = limits;
    let fits = |(w, h): (u32, u32)| {
        w <= max_x && h <= max_y && w <= u16::MAX as u32 && h <= u16::MAX as u32
            && w as u64 * h as u64 * BYTES_PER_PIXEL as u64 <= vram
    };
    let ceiling = preferred.unwrap_or(DEFAULT_LIMIT);
    let mut list = alloc::vec::Vec::new();
    if let Some(p) = preferred { if fits(p) { list.push(p); } }
    for mode in MODES {
        if mode.0 <= ceiling.0 && mode.1 <= ceiling.1 && fits(mode) && !list.contains(&mode) { list.push(mode); }
    }
    list
}

/// Switches to the best mode the display and adapter support, 32-bit with a linear
/// framebuffer.
pub fn enable() -> Option<Framebuffer> {
    let adapter = adapter()?;
    // The graphical console stops drawing while the desktop owns the screen: with
    // the BGA enabled, the legacy 0xA0000 window is a bank of the desktop's video memory.
    crate::vga::suspend();
    let preferred = adapter.mmio.and_then(edid_preferred);
    for (width, height) in candidates(preferred, limits()) {
        write(ENABLE, 0);
        write(XRES, width as u16);
        write(YRES, height as u16);
        write(BPP, 32);
        write(VIRT_WIDTH, width as u16);
        write(ENABLE, ENABLED | LFB);
        if read(XRES) == width as u16 && read(YRES) == height as u16 && read(BPP) == 32 {
            return Some(Framebuffer { phys: adapter.lfb, width, height, pitch: width * BYTES_PER_PIXEL });
        }
    }
    write(ENABLE, 0);
    crate::vga::resume();
    None
}

/// Returns to the legacy VGA mode the console uses (320x200x256, mode 13h) and
/// redraws the console.
pub fn disable() {
    if find().is_some() {
        write(ENABLE, 0);
        restore_console_mode();
    }
    crate::vga::resume();
}

pub fn bytes(framebuffer: &Framebuffer) -> usize { (framebuffer.pitch * framebuffer.height) as usize }

// ---------------------------------------------------------------------------
// Returning to the console's VGA mode. Disabling the BGA leaves the legacy VGA
// registers in their linear-framebuffer configuration, so the standard mode-13h
// register set (320x200, 256 colours, chain-4 at 0xA0000) is reprogrammed, and the
// 16 classic colours the console draws with are reloaded into the DAC.

const MISC_MODE13: u8 = 0x63;
const SEQ_MODE13: [u8; 5] = [0x03, 0x01, 0x0f, 0x00, 0x0e];
const CRTC_MODE13: [u8; 25] = [0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0xbf, 0x1f, 0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x9c, 0x0e, 0x8f, 0x28, 0x40, 0x96, 0xb9, 0xa3, 0xff];
const GC_MODE13: [u8; 9] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x05, 0x0f, 0xff];
const AC_MODE13: [u8; 21] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x41, 0x00, 0x0f, 0x00, 0x00];
/// The default VGA palette's first 16 entries (6-bit DAC values).
const CLASSIC_COLOURS: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], [0x00, 0x00, 0x2a], [0x00, 0x2a, 0x00], [0x00, 0x2a, 0x2a],
    [0x2a, 0x00, 0x00], [0x2a, 0x00, 0x2a], [0x2a, 0x15, 0x00], [0x2a, 0x2a, 0x2a],
    [0x15, 0x15, 0x15], [0x15, 0x15, 0x3f], [0x15, 0x3f, 0x15], [0x15, 0x3f, 0x3f],
    [0x3f, 0x15, 0x15], [0x3f, 0x15, 0x3f], [0x3f, 0x3f, 0x15], [0x3f, 0x3f, 0x3f],
];

fn seq(index: u8, value: u8) { Port::new(0x3c4).write(index); Port::new(0x3c5).write(value); }
fn gc(index: u8, value: u8) { Port::new(0x3ce).write(index); Port::new(0x3cf).write(value); }

/// Reprograms mode 13h and the console's colours.
pub fn restore_console_mode() {
    Port::new(0x3c2).write(MISC_MODE13);
    for (i, &v) in SEQ_MODE13.iter().enumerate() { seq(i as u8, v); }
    // Unlock CRTC registers 0-7 before writing them.
    Port::new(0x3d4).write(0x11);
    let protect = Port::new(0x3d5).read();
    Port::new(0x3d5).write(protect & 0x7f);
    for (i, &v) in CRTC_MODE13.iter().enumerate() { Port::new(0x3d4).write(i as u8); Port::new(0x3d5).write(v); }
    for (i, &v) in GC_MODE13.iter().enumerate() { gc(i as u8, v); }
    for (i, &v) in AC_MODE13.iter().enumerate() {
        Port::new(0x3da).read(); // Reset the attribute flip-flop to the index state.
        Port::new(0x3c0).write(i as u8);
        Port::new(0x3c0).write(v);
    }
    Port::new(0x3da).read();
    Port::new(0x3c0).write(0x20); // Re-enable video output.
    Port::new(0x3c8).write(0);
    for colour in CLASSIC_COLOURS { for c in colour { Port::new(0x3c9).write(c); } }
}
