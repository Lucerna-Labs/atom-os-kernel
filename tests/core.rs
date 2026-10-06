// Native diagnostics using the actual, unchanged source modules.
// No privileged IRQ, port-I/O, or paging instruction is executed here.
#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
extern crate kernel_kit;
use kernel_kit::{context, elf, fs, keyboard, memory, slab};
#[path = "../kernel-orchestrator/src/scheduler.rs"] mod scheduler;
#[path = "../kernel-orchestrator/src/conversation.rs"] mod conversation;

use std::alloc::{alloc_zeroed, dealloc, Layout};

struct Region { ptr: *mut u8, layout: Layout }
impl Region {
    fn new(size: usize) -> Self {
        let layout = Layout::from_size_align(size, 4096).unwrap();
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null());
        Self { ptr, layout }
    }
    fn bump(&self) -> memory::BumpAllocator {
        let mut bump = memory::BumpAllocator::new();
        bump.init(self.ptr as usize, self.layout.size());
        bump
    }
}
impl Drop for Region { fn drop(&mut self) { unsafe { dealloc(self.ptr, self.layout); } } }

#[test]
fn frame_exhaustion_and_reclamation() {
    let mut frames = memory::FrameAllocator::new();
    frames.init(0x100000, 8);
    let allocated: Vec<_> = (0..8).map(|_| frames.alloc_frame().unwrap()).collect();
    assert_eq!(allocated.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 8);
    assert_eq!(frames.alloc_frame(), None);
    frames.free_frame(allocated[3]);
    assert_eq!(frames.alloc_frame(), Some(allocated[3]));
    assert_eq!(frames.alloc_frame(), None);
}

#[test]
fn frame_contiguous_allocation_handles_fragmentation() {
    let mut frames = memory::FrameAllocator::new();
    frames.init(0x100000, 8);
    assert_eq!(frames.alloc_contiguous(4), Some(0x100000));
    frames.free_frame(0x101000);
    assert_eq!(frames.alloc_contiguous(3), Some(0x104000));
    assert_eq!(frames.alloc_contiguous(2), None);
    assert_eq!(frames.alloc_contiguous(0), None);
    assert_eq!(frames.alloc_frame(), Some(0x101000));
    assert_eq!(frames.alloc_frame(), Some(0x107000));
}

#[test]
fn frame_free_rejects_unaligned_and_out_of_pool_addresses() {
    let mut frames = memory::FrameAllocator::new();
    frames.init(0x100000, 1);
    assert_eq!(frames.alloc_frame(), Some(0x100000));
    for address in [0xfffff, 0x100001, 0x101000] { frames.free_frame(address); }
    assert_eq!(frames.alloc_frame(), None);
}

#[test]
fn slab_reuses_small_allocations_for_100000_rounds() {
    let region = Region::new(65536);
    let mut bump = region.bump();
    let mut heap = slab::SlabHeap::new();
    let layout = Layout::from_size_align(64, 8).unwrap();
    for round in 0..100000 {
        let ptr = unsafe { heap.alloc(layout, &mut bump) };
        assert!(!ptr.is_null(), "allocation exhausted at {round}");
        unsafe { ptr.write_bytes(0xa5, 64); heap.dealloc(ptr, layout); }
    }
}

#[test]
fn slab_honors_requested_alignment() {
    let region = Region::new(65536);
    let mut bump = region.bump();
    let mut heap = slab::SlabHeap::new();
    for alignment in [8, 16, 32, 64, 128, 256, 4096] {
        let layout = Layout::from_size_align(64, alignment).unwrap();
        let ptr = unsafe { heap.alloc(layout, &mut bump) };
        assert!(!ptr.is_null());
        assert_eq!(ptr as usize % alignment, 0, "requested align={alignment}, returned {ptr:p}");
        unsafe { heap.dealloc(ptr, layout); }
    }
}

#[test]
fn slab_live_allocations_remain_disjoint() {
    let region = Region::new(65536);
    let mut bump = region.bump();
    let mut heap = slab::SlabHeap::new();
    let mut blocks = Vec::new();
    for (index, size) in [1, 16, 17, 64, 128, 1024, 2048].into_iter().enumerate() {
        let layout = Layout::from_size_align(size, 8).unwrap();
        let ptr = unsafe { heap.alloc(layout, &mut bump) };
        assert!(!ptr.is_null());
        unsafe { ptr.write_bytes(index as u8 + 1, size); }
        blocks.push((ptr, layout, index as u8 + 1));
    }
    for (ptr, layout, value) in blocks {
        assert!(unsafe { std::slice::from_raw_parts(ptr, layout.size()) }.iter().all(|&b| b == value));
        unsafe { heap.dealloc(ptr, layout); }
    }
}

#[test]
fn scheduler_round_robin_preserves_saved_stack_pointers() {
    let mut scheduler = scheduler::Scheduler::new();
    scheduler.spawn(context::Context::new(1, 0x1000, 0x2000, 0x3000)).unwrap();
    scheduler.spawn(context::Context::new(2, 0x4000, 0x5000, 0x6000)).unwrap();
    assert_eq!(scheduler.switch_context(0xdead), 0x1000);
    assert_eq!(scheduler.current_task().unwrap().id, 1);
    assert_eq!(scheduler.switch_context(0x1110), 0x4000);
    assert_eq!(scheduler.current_task().unwrap().id, 2);
    assert_eq!(scheduler.switch_context(0x4110), 0x1110);
    assert_eq!(scheduler.switch_context(0x1220), 0x4110);
}

#[test]
fn scheduler_capacity_limit_is_enforced() {
    let mut scheduler = scheduler::Scheduler::new();
    for id in 1..=16 { scheduler.spawn(context::Context::new(id, id as u64, 0, 0)).unwrap(); }
    assert!(scheduler.spawn(context::Context::new(17, 17, 0, 0)).is_err());
}

#[test]
fn scheduler_reuses_terminated_slots() {
    let mut scheduler = scheduler::Scheduler::new();
    for id in 1..=16 { scheduler.spawn(context::Context::new(id, id as u64, 0, 0)).unwrap(); }
    scheduler.switch_context(0xdead);
    scheduler.current_task_mut().unwrap().state = context::TaskState::Terminated;
    scheduler.switch_context(0xbeef);
    assert!(scheduler.spawn(context::Context::new(17, 17, 0, 0)).is_ok(),
            "a terminated task permanently occupies its slot");
}

fn elf_header() -> elf::Elf64_Ehdr {
    let mut header: elf::Elf64_Ehdr = unsafe { std::mem::zeroed() };
    header.e_ident[..7].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1]);
    header.e_type = 2;
    header.e_machine = 62;
    header.e_version = 1;
    header.e_ehsize = 64;
    header.e_phentsize = 56;
    header
}

#[test]
fn elf_gate_accepts_x86_64_and_rejects_bad_magic() {
    let mut header = elf_header();
    assert!(header.is_valid());
    header.e_ident[0] = 0;
    assert!(!header.is_valid());
}

#[test]
fn elf_gate_rejects_wrong_architecture() {
    let mut header = elf_header();
    header.e_machine = 183; // AArch64 ELF must not be loaded as x86_64 instructions.
    assert!(!header.is_valid(), "the only ELF validity gate accepts AArch64");
}

#[test]
fn elf_gate_rejects_unsupported_byte_order() {
    let mut header = elf_header();
    header.e_ident[5] = 2; // Big-endian data cannot be interpreted as native x86_64 fields.
    assert!(!header.is_valid(), "the only ELF validity gate accepts big-endian headers");
}

#[test]
fn keyboard_basic_input_and_release_scancodes() {
    assert_eq!(keyboard::scancode_to_ascii(0x1e), Some(b'a'));
    assert_eq!(keyboard::scancode_to_ascii(0x1c), Some(b'\n'));
    assert_eq!(keyboard::scancode_to_ascii(0x9e), None);
    assert_eq!(keyboard::scancode_to_ascii(0x4e), Some(b'>'));
}

#[test]
fn tagged_fallback_reuses_large_aligned_allocations() {
    let region = Region::new(1024 * 1024);
    let mut bump = region.bump();
    let mut heap = slab::SlabHeap::new();
    for _ in 0..512 {
        for (size, align) in [(65536, 4096), (64, 8192), (32768, 64)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let ptr = unsafe { heap.alloc(layout, &mut bump) };
            assert!(!ptr.is_null()); assert_eq!(ptr as usize % align, 0);
            unsafe { ptr.write_bytes(0x5a, size); heap.dealloc(ptr, layout); }
        }
    }
}

fn executable() -> Vec<u8> {
    let mut header = elf_header();
    header.e_entry = elf::IMAGE_BASE;
    header.e_phoff = 64;
    header.e_phnum = 1;
    let segment = elf::Elf64_Phdr { p_type: 1, p_flags: 5, p_offset: 4096,
        p_vaddr: elf::IMAGE_BASE, p_paddr: 0, p_filesz: 4, p_memsz: 4096, p_align: 4096 };
    let mut bytes = vec![0; 4100];
    unsafe {
        std::ptr::write_unaligned(bytes.as_mut_ptr() as *mut elf::Elf64_Ehdr, header);
        std::ptr::write_unaligned(bytes.as_mut_ptr().add(64) as *mut elf::Elf64_Phdr, segment);
    }
    bytes[4096..].copy_from_slice(&[0x90, 0x90, 0x90, 0xc3]);
    bytes
}

#[test]
fn elf_loader_checks_offsets_lengths_and_permissions() {
    let valid = executable();
    assert!(elf::Image::parse(&valid).is_ok());
    for len in [0, 1, 63, 64, 119, 4099] { assert!(elf::Image::parse(&valid[..len]).is_err()); }
    for (offset, bad) in [(32, u64::MAX), (64 + 8, u64::MAX), (64 + 32, 8192), (24, 0x200000)] {
        let mut bytes = valid.clone(); bytes[offset..offset + 8].copy_from_slice(&bad.to_le_bytes());
        assert!(elf::Image::parse(&bytes).is_err(), "offset {offset}");
    }
    let mut wx = valid.clone(); wx[68..72].copy_from_slice(&7u32.to_le_bytes());
    assert!(elf::Image::parse(&wx).is_err());
}


#[test]
fn scheduler_uses_idle_when_all_tasks_are_blocked() {
    let mut s = scheduler::Scheduler::new();
    s.spawn(context::Context::new(1, 0x1000, 0x2000, 0x3000)).unwrap();
    assert_eq!(s.switch_context(0x9000), 0x1000);
    s.current_task_mut().unwrap().state = context::TaskState::Blocked;
    s.current_task_mut().unwrap().sleep_until = 2;
    assert_eq!(s.switch_context(0x1100), 0x9000);
    assert_eq!(s.timer_tick(0x9100), 0x9100);
    assert_eq!(s.timer_tick(0x9200), 0x1100);
}

#[test]
fn frame_allocator_spans_multiple_regions_up_to_8_gib() {
    let mut frames = Box::new(memory::FrameAllocator::new());
    // QEMU with 8 GiB: about 3 GiB below the PCI hole, 5 GiB above 4 GiB.
    let low = frames.add_region(0x100000, (3 << 30) / 4096 - 256);
    let high = frames.add_region(4 << 30, (5 << 30) / 4096);
    assert_eq!(frames.region_count(), 2);
    assert_eq!(frames.total_count(), low + high);
    assert!(frames.total_count() * 4096 >= (8u64 << 30) as usize - (1 << 20));
    assert_eq!(frames.add_region(0x200000, 4), 0, "overlapping region accepted");
    // Never hand out frames from the hole between the regions.
    frames.free_frame(3 << 30);
    assert_eq!(frames.free_count(), low + high);
    // Drain the low region and confirm allocation continues above 4 GiB.
    let run = frames.alloc_contiguous(low).unwrap();
    assert_eq!(run, 0x100000);
    let above = frames.alloc_frame().unwrap();
    assert_eq!(above, 4 << 30);
    let big = frames.alloc_contiguous(512).unwrap();
    assert!(big > 4 << 30);
    assert_eq!(frames.free_count(), high - 513);
    frames.free_frame(above);
    frames.free_frame(above);
    assert_eq!(frames.free_count(), high - 512, "double free counted twice");
    for page in 0..512 { frames.free_frame(big + page * 4096); }
    for page in 0..low as u64 { frames.free_frame(run + page * 4096); }
    assert_eq!(frames.free_count(), low + high);
    // A region past the 16 GiB tracking limit is clipped, not misindexed.
    assert_eq!(frames.add_region((16 << 30) - 4096, 8), 1);
}

#[test]
fn pipe_reports_end_of_input_and_broken_pipe_by_counted_ends() {
    use kernel_kit::pipe::{PipeEnd, Read, Write, PIPE_CAPACITY};
    let (reader, writer) = PipeEnd::pair();
    let mut out = [0u8; 8];
    assert_eq!(reader.read(&mut out), Read::WouldBlock, "empty pipe with a writer must block");
    assert_eq!(writer.write(b"hello"), Write::Wrote(5));
    assert_eq!(reader.read(&mut out[..3]), Read::Data(3));
    assert_eq!(&out[..3], b"hel");
    // A full pipe accepts a partial write, then blocks.
    let big = vec![b'x'; PIPE_CAPACITY];
    assert_eq!(writer.write(&big), Write::Wrote(PIPE_CAPACITY - 2));
    assert_eq!(writer.write(b"y"), Write::WouldBlock);
    // A cloned writer keeps the pipe open after the original is dropped.
    let second = writer.clone();
    drop(writer);
    let mut drain = vec![0u8; PIPE_CAPACITY];
    assert_eq!(reader.read(&mut drain), Read::Data(PIPE_CAPACITY));
    assert_eq!(&drain[..2], b"lo");
    assert_eq!(reader.read(&mut out), Read::WouldBlock);
    drop(second);
    assert_eq!(reader.read(&mut out), Read::End, "no writers and no data is end of input");
    // Writing with no reader left is a broken pipe.
    let (reader, writer) = PipeEnd::pair();
    drop(reader);
    assert_eq!(writer.write(b"z"), Write::Broken);
}

#[test]
fn keyboard_decoder_tracks_shift_caps_ctrl_and_extended_keys() {
    use kernel_kit::abi::*;
    use kernel_kit::input::KeyboardDecoder;
    let mut k = KeyboardDecoder::new();
    let mut typed = |k: &mut KeyboardDecoder, codes: &[u8]| -> String {
        codes.iter().filter_map(|&c| k.feed(c)).filter_map(|e| KeyboardDecoder::console_byte(&e)).map(|b| b as char).collect()
    };
    // a, Shift+a, Shift+1, Shift+., Shift+\ then release Shift, '.'
    assert_eq!(typed(&mut k, &[0x1e, 0x9e, 0x2a, 0x1e, 0x02, 0x34, 0x2b, 0xaa, 0x34]), "aA!>|.");
    // Caps Lock affects letters only; Shift inverts it.
    assert_eq!(typed(&mut k, &[0x3a, 0xba, 0x1e, 0x02, 0x36, 0x1e, 0xb6]), "A1a");
    assert_eq!(typed(&mut k, &[0x3a, 0xba]), "");
    // Ctrl+C produces a key event with MOD_CTRL but no console byte.
    let events: Vec<_> = [0x1d, 0x2e, 0x9d].iter().filter_map(|&c| k.feed(c)).collect();
    assert_eq!(events[1].key, b'c' as u16);
    assert_eq!(events[1].modifiers & MOD_CTRL, MOD_CTRL);
    assert_eq!(KeyboardDecoder::console_byte(&events[1]), None);
    // Extended arrows and release events.
    let up = k.feed(0xe0).or(k.feed(0x48)).unwrap();
    assert_eq!((up.key, up.pressed), (KEY_UP, 1));
    let release = { k.feed(0xe0); k.feed(0xc8).unwrap() };
    assert_eq!((release.key, release.pressed), (KEY_UP, 0));
    // The legacy numpad '+' still types '>'.
    assert_eq!(typed(&mut k, &[0x4e]), ">");
    assert_eq!(k.feed(0x3c).unwrap().key, KEY_F1 + 1);
}

#[test]
fn mouse_decoder_assembles_packets_with_signs_and_wheel() {
    use kernel_kit::abi::*;
    use kernel_kit::input::MouseDecoder;
    let mut m = MouseDecoder::new();
    // Out-of-sync byte (bit 3 clear) is skipped.
    assert!(m.feed(0x00).is_none());
    // Left button, dx = +5, dy = -3 in PS/2 terms (up), so screen dy = +3.
    assert!(m.feed(0x08 | 0x01 | 0x20).is_none());
    assert!(m.feed(5).is_none());
    let e = m.feed((-3i8) as u8).unwrap();
    assert_eq!((e.kind, e.buttons, e.dx, e.dy), (INPUT_MOUSE, MOUSE_LEFT, 5, 3));
    // dx = -2 with the sign bit; overflowed packets are dropped.
    m.feed(0x18); m.feed((-2i8) as u8);
    assert_eq!(m.feed(0).unwrap().dx, -2);
    m.feed(0x48); m.feed(1);
    assert!(m.feed(1).is_none());
    // IntelliMouse 4-byte packets: z = -1 means the wheel moved up.
    let mut w = MouseDecoder::new();
    w.wheel = true;
    w.feed(0x08); w.feed(0); w.feed(0);
    assert_eq!(w.feed(0x0f).unwrap().wheel, 1);
}

#[test]
fn rtc_fields_convert_to_unix_time() {
    use kernel_kit::rtc::{days_from_civil, to_unix};
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(2000, 3, 1), 11017);
    // 2026-10-03 19:45:30 in BCD, 24-hour mode (status B = 0x02).
    assert_eq!(to_unix([0x30, 0x45, 0x19, 0x03, 0x10, 0x26], 0x02), 1_791_056_730);
    // The same time in binary, 12-hour mode with the PM bit.
    assert_eq!(to_unix([30, 45, 0x80 | 7, 3, 10, 26], 0x04), 1_791_056_730);
}

#[test]
fn display_mode_follows_edid_and_video_memory() {
    use kernel_kit::display::{candidates, parse_edid};
    fn checksum(block: &mut [u8]) {
        block[127] = 0u8.wrapping_sub(block[..127].iter().fold(0u8, |s, &b| s.wrapping_add(b)));
    }
    // Base block whose first detailed timing is 2560x1440.
    let mut base = [0u8; 128];
    base[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
    base[54] = 1; // Non-zero pixel clock: a timing descriptor.
    base[56] = 0x00; base[58] = 0xa0; // 2560 = 0xa00
    base[59] = 0xa0; base[61] = 0x50; // 1440 = 0x5a0
    checksum(&mut base);
    assert_eq!(parse_edid(&base), Some((2560, 1440)));
    let mut bad = base;
    bad[100] ^= 1;
    assert_eq!(parse_edid(&bad), None, "checksum");

    // QEMU's EDID for a 6144x3456 display (-device VGA,xres=6144,yres=3456): no
    // detailed timing in the base block, the mode is in a DisplayID extension.
    let mut edid = [0u8; 384];
    edid[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
    edid[54..58].copy_from_slice(&[0, 0, 0, 0xf7]);
    edid[126] = 2;
    checksum(&mut edid[..128]);
    edid[128..132].copy_from_slice(&[0x02, 0x03, 0x0b, 0x00]); // CTA-861 with no timings.
    checksum(&mut edid[128..256]);
    edid[256..285].copy_from_slice(&[0x70, 0x13, 0x17, 0x03, 0x00, 0x03, 0x00, 0x14, 0xed, 0x64, 0x03, 0x88,
        0xff, 0x17, 0x65, 0x08, 0xff, 0x05, 0xb7, 0x00, 0x7f, 0x0d, 0x77, 0x00, 0x10, 0x00, 0x10, 0x00, 0x7f]);
    checksum(&mut edid[256..384]);
    assert_eq!(parse_edid(&edid), Some((6144, 3456)));

    let mib = |n: u64| n << 20;
    // Enough video memory: the monitor's own mode wins.
    assert_eq!(candidates(Some((6144, 3456)), (16000, 12000, mib(128)))[0], (6144, 3456));
    // 16 MiB holds at most 2560x1440 at 32 bpp, so a 6K monitor gets that.
    assert_eq!(candidates(Some((6144, 3456)), (16000, 12000, mib(16)))[0], (2560, 1440));
    // A mode the adapter cannot do is skipped.
    assert_eq!(candidates(Some((3840, 2160)), (2560, 1600, mib(64)))[0], (2560, 1440));
    // No EDID: Full HD at most, never a giant mode on an unknown screen.
    assert_eq!(candidates(None, (16000, 12000, mib(256)))[0], (1920, 1080));
    // An unusual preferred mode comes first, then standard modes below it.
    let list = candidates(Some((1280, 800)), (16000, 12000, mib(16)));
    assert_eq!(&list[..3], &[(1280, 800), (1280, 720), (1024, 768)]);
}

// ---- File system and storage -------------------------------------------------
//
// File contents live in "physical" frames. Natively, a fixed low mapping stands in
// for physical memory (the kernel's physical-memory offset is 0 here), handed once to
// the real frame allocator, so the production FileData and Store code run unchanged.

use kernel_kit::fs::{Content, Fs, FsError, ROOT};
use kernel_kit::storage::{legacy, Store, BLOCK};
use kernel_kit::virtio_blk::{BlockDevice, DiskError};

extern "C" { fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8; }

fn host_frames() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        const BASE: usize = 0x1_0000_0000; // Inside the allocator's 16 GiB range.
        const BYTES: usize = 512 << 20;
        // PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED_NOREPLACE.
        let at = unsafe { mmap(BASE as *mut u8, BYTES, 3, 0x02 | 0x20 | 0x100000, -1, 0) };
        assert_eq!(at as usize, BASE, "could not map the stand-in physical memory");
        let (frames, flags) = memory::FRAME_ALLOCATOR.lock();
        frames.add_region(BASE, BYTES / 4096);
        memory::FRAME_ALLOCATOR.unlock(flags);
    });
}
fn free_frames() -> usize {
    let (frames, flags) = memory::FRAME_ALLOCATOR.lock();
    let n = frames.free_count();
    memory::FRAME_ALLOCATOR.unlock(flags);
    n
}
/// Tests touching the shared frame pool or counting it run one at a time.
static FRAMES: std::sync::Mutex<()> = std::sync::Mutex::new(());

// A fault-injection disk modelling a volatile write cache: `power_cut` drops every
// write since the last flush, and `fail_at` makes the n-th write or flush fail.
#[derive(Clone)]
struct FaultDisk { live: Vec<[u8; 512]>, durable: Vec<[u8; 512]>, operations: usize, fail_at: Option<usize>, written: usize }
impl FaultDisk {
    fn new(bytes: usize) -> Self {
        let data = vec![[0; 512]; bytes / 512];
        Self { live: data.clone(), durable: data, operations: 0, fail_at: None, written: 0 }
    }
    fn operation(&mut self) -> Result<(), DiskError> {
        let at = self.operations; self.operations += 1;
        if self.fail_at == Some(at) { Err(DiskError::Io) } else { Ok(()) }
    }
    fn power_cut(&mut self) { self.live = self.durable.clone(); self.fail_at = None; self.operations = 0; }
}
impl BlockDevice for FaultDisk {
    fn sectors(&self) -> u64 { self.live.len() as u64 }
    fn read_sector(&mut self, sector: u64, bytes: &mut [u8; 512]) -> Result<(), DiskError> { *bytes = self.live[sector as usize]; Ok(()) }
    fn write_sector(&mut self, sector: u64, bytes: &[u8; 512]) -> Result<(), DiskError> {
        self.operation()?; self.written += 1; self.live[sector as usize] = *bytes; Ok(())
    }
    fn flush(&mut self) -> Result<(), DiskError> { self.operation()?; self.durable = self.live.clone(); Ok(()) }
}

fn pattern(len: usize, seed: u64) -> Vec<u8> { (0..len).map(|i| ((i as u64 * 31 + seed) % 251) as u8).collect() }
fn write_file(fs: &mut Fs, path: &str, bytes: &[u8]) {
    let ino = fs.open_or_create(path, 7).unwrap();
    fs.truncate(ino, 0, 7).unwrap();
    assert_eq!(fs.write(ino, 0, bytes, 7), Ok(bytes.len()));
}
fn read_file<D: BlockDevice>(fs: &mut Fs, store: &mut Store<D>, path: &str) -> Vec<u8> {
    let ino = fs.resolve(path).unwrap();
    store.load(fs.file_mut(ino).unwrap()).unwrap();
    let mut out = vec![0; fs.node(ino).unwrap().size() as usize];
    assert_eq!(fs.read(ino, 0, &mut out), Ok(out.len()));
    out
}

#[test]
fn fs_tree_creates_moves_and_removes_by_path() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    fs.mkdir("/docs", 1).unwrap();
    fs.mkdir("/docs/work", 1).unwrap();
    assert_eq!(fs.mkdir("/docs", 1), Err(FsError::Exists));
    assert_eq!(fs.mkdir("/missing/x", 1), Err(FsError::NotFound));
    write_file(&mut fs, "/docs/work/plan.txt", b"plan");
    write_file(&mut fs, "notes.txt", b"top");                       // No leading slash: the root.
    assert_eq!(fs.resolve("/notes.txt"), fs.resolve("notes.txt"));
    assert_eq!(fs.open_or_create("/docs/work/plan.txt/x", 1), Err(FsError::NotDir));
    assert_eq!(fs.open_or_create("/docs", 1), Err(FsError::IsDir));
    for bad in ["/a/../b", "/./x", "/bad\u{7}name", &"n".repeat(256)] { assert_eq!(fs.open_or_create(bad, 1), Err(FsError::Invalid)); }

    let plan = fs.resolve("/docs/work/plan.txt").unwrap();
    assert_eq!(fs.path_of(plan), "/docs/work/plan.txt");
    // Moving a folder carries its contents and keeps inode numbers (open files stay valid).
    fs.rename("/docs/work", "/archive", 2).unwrap();
    assert_eq!(fs.resolve("/archive/plan.txt"), Ok(plan));
    assert_eq!(fs.rename("/docs", "/docs/inner", 2), Err(FsError::Invalid), "a folder cannot move into itself");
    fs.rename("/archive", "/docs/archive", 2).unwrap();
    assert_eq!(fs.rename("/docs", "/docs/archive/deeper", 2), Err(FsError::Invalid));
    assert_eq!(fs.rename("/notes.txt", "/docs/archive/plan.txt", 2), Err(FsError::Exists));
    assert_eq!(fs.remove("/docs", |_| false), Err(FsError::NotEmpty));
    assert_eq!(fs.remove("/docs/archive/plan.txt", |ino| ino == plan), Err(FsError::Busy));
    fs.remove("/docs/archive/plan.txt", |_| false).unwrap();
    fs.remove("/docs/archive", |_| false).unwrap();
    fs.remove("/docs", |_| false).unwrap();
    assert_eq!(fs.resolve("/docs"), Err(FsError::NotFound));

    // Boot-image programs: in /bin, read-only, never removed, renamed or moved.
    static PROGRAM: [u8; 5] = *b"\x7fELF!";
    let shell = fs.install_builtin("shell.elf", &PROGRAM).unwrap();
    assert_eq!(fs.resolve("/bin/shell.elf"), Ok(shell));
    assert!(fs.is_builtin(shell));
    assert_eq!(fs.write(shell, 0, b"x", 3), Err(FsError::ReadOnly));
    assert_eq!(fs.remove("/bin/shell.elf", |_| false), Err(FsError::ReadOnly));
    assert_eq!(fs.rename("/bin", "/programs", 3), Err(FsError::ReadOnly));
    assert_eq!(fs.rename("/bin/shell.elf", "/x.elf", 3), Err(FsError::ReadOnly));
    let mut head = [0u8; 4];
    assert_eq!(fs.read(shell, 1, &mut head), Ok(4));
    assert_eq!(&head, b"ELF!");
}

#[test]
fn file_data_handles_large_sparse_and_truncated_files_without_leaking_frames() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let before = free_frames();
    {
        let mut fs = Fs::new();
        let ino = fs.open_or_create("/big.bin", 1).unwrap();
        // 5 MiB in uneven pieces crosses index frames (2 MiB each) and page boundaries.
        let data = pattern(5 << 20, 3);
        for chunk in data.chunks(100_003).enumerate() {
            assert_eq!(fs.write(ino, (chunk.0 * 100_003) as u64, chunk.1, 1), Ok(chunk.1.len()));
        }
        let mut back = vec![0; data.len()];
        assert_eq!(fs.read(ino, 0, &mut back), Ok(data.len()));
        assert!(back == data);
        // A write past the end leaves a gap that reads as zeros.
        fs.write(ino, (6 << 20) + 5, b"tail", 1).unwrap();
        let mut gap = vec![1u8; 4096];
        fs.read(ino, (5 << 20) + 100, &mut gap).unwrap();
        assert!(gap.iter().all(|&b| b == 0));
        // Shrink to an odd length, then grow: the old bytes must not reappear.
        fs.truncate(ino, 10_000, 1).unwrap();
        fs.truncate(ino, 20_000, 1).unwrap();
        let mut tail = vec![1u8; 10_000];
        assert_eq!(fs.read(ino, 10_000, &mut tail), Ok(10_000));
        assert!(tail.iter().all(|&b| b == 0), "bytes past a truncation came back");
        assert_eq!(fs.write(ino, kernel_kit::fs::FILE_MAX, b"x", 1), Err(FsError::NoSpace));
        // Capacity: with room for 4 blocks in total, a 5th block is refused.
        let mut small = Fs::new();
        small.capacity = Some(4);
        let f = small.open_or_create("/f", 1).unwrap();
        assert_eq!(small.write(f, 0, &vec![1; 4 * 4096], 1), Ok(4 * 4096));
        assert_eq!(small.write(f, 4 * 4096, b"x", 1), Err(FsError::NoSpace));
        small.truncate(f, 4096, 1).unwrap();
        assert_eq!(small.blocks, 1);
    }
    assert_eq!(free_frames(), before, "file frames were not all returned");
}

fn sample_tree(fs: &mut Fs) {
    fs.mkdir("/docs", 10).unwrap();
    fs.mkdir("/docs/photos", 11).unwrap();
    fs.mkdir("/empty", 12).unwrap();
    write_file(fs, "/docs/report.txt", b"quarterly numbers\n");
    write_file(fs, "/docs/photos/raw.bin", &pattern(3 << 20, 9)); // 3 MiB: far past the old 64 KiB limit.
    write_file(fs, "/empty.txt", b"");
    static PROGRAM: [u8; 4] = *b"prog";
    fs.install_builtin("shell.elf", &PROGRAM).unwrap();
}

#[test]
fn store_saves_folders_and_large_files_and_loads_them_lazily() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    let mut store = Store::open(FaultDisk::new(32 << 20), &mut fs).unwrap();
    assert_eq!(store.generation, 0);
    sample_tree(&mut fs);
    assert!(fs.unsaved());
    store.commit(&mut fs).unwrap();
    assert!(!fs.unsaved());
    store.device.power_cut();

    let mut fs = Fs::new();
    let mut store = Store::open(store.device, &mut fs).unwrap();
    assert_eq!(store.generation, 1);
    assert!(fs.node(fs.resolve("/empty").unwrap()).unwrap().is_dir());
    assert!(fs.resolve("/bin/shell.elf").is_err(), "a boot-image program was saved");
    let raw = fs.resolve("/docs/photos/raw.bin").unwrap();
    assert!(matches!(fs.node(raw).unwrap().file().unwrap().content, Content::Unloaded), "contents load on first open");
    assert_eq!(fs.node(raw).unwrap().modified, 7);
    assert!(read_file(&mut fs, &mut store, "/docs/photos/raw.bin") == pattern(3 << 20, 9));
    assert_eq!(read_file(&mut fs, &mut store, "/docs/report.txt"), b"quarterly numbers\n");
    assert_eq!(read_file(&mut fs, &mut store, "/empty.txt"), b"");

    // A second save writes only what changed: one small file, not the 3 MiB one.
    write_file(&mut fs, "/docs/report.txt", b"revised\n");
    fs.rename("/empty.txt", "/docs/empty.txt", 20).unwrap();
    store.device.written = 0;
    store.commit(&mut fs).unwrap();
    assert!(store.device.written < 64, "rewrote {} sectors for a small change", store.device.written);
    store.device.power_cut();
    let mut fs = Fs::new();
    let mut store = Store::open(store.device, &mut fs).unwrap();
    assert_eq!(store.generation, 2);
    assert_eq!(read_file(&mut fs, &mut store, "/docs/report.txt"), b"revised\n");
    assert!(fs.resolve("/docs/empty.txt").is_ok() && fs.resolve("/empty.txt").is_err());
    assert!(read_file(&mut fs, &mut store, "/docs/photos/raw.bin") == pattern(3 << 20, 9));
    // Saving with nothing changed writes nothing.
    store.device.written = 0;
    store.commit(&mut fs).unwrap();
    assert_eq!((store.generation, store.device.written), (2, 0));
}

#[test]
fn interrupted_save_keeps_the_previous_generation_at_every_step() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    let mut store = Store::open(FaultDisk::new(1 << 20), &mut fs).unwrap();
    fs.mkdir("/keep", 1).unwrap();
    write_file(&mut fs, "/keep/a.txt", b"original");
    store.commit(&mut fs).unwrap();
    let saved = store.device.clone();
    // Count the operations of the replacement save, then fail each one in turn.
    let change = |fs: &mut Fs| {
        write_file(fs, "/keep/a.txt", &pattern(9000, 4));
        fs.mkdir("/new", 2).unwrap();
    };
    let steps = {
        let mut fs = Fs::new();
        let mut trial = Store::open(saved.clone(), &mut fs).unwrap();
        trial.load(fs.file_mut(fs.resolve("/keep/a.txt").unwrap()).unwrap()).unwrap();
        trial.device.operations = 0;
        change(&mut fs);
        trial.commit(&mut fs).unwrap();
        trial.device.operations
    };
    assert!(steps > 10);
    for failure in 0..steps {
        let mut fs = Fs::new();
        let mut disk = saved.clone();
        disk.fail_at = Some(failure);
        let mut store = Store::open(disk, &mut fs).unwrap();
        store.load(fs.file_mut(fs.resolve("/keep/a.txt").unwrap()).unwrap()).unwrap();
        store.device.operations = 0;
        store.device.fail_at = Some(failure);
        change(&mut fs);
        assert!(store.commit(&mut fs).is_err(), "failure={failure}");
        assert!(fs.unsaved(), "a failed save marked changes as saved");
        store.device.power_cut();
        let mut fs = Fs::new();
        let mut store = Store::open(store.device, &mut fs).unwrap();
        assert_eq!(store.generation, 1, "failure={failure}");
        assert_eq!(read_file(&mut fs, &mut store, "/keep/a.txt"), b"original", "failure={failure}");
        assert!(fs.resolve("/new").is_err());
    }
}

#[test]
fn store_falls_back_from_a_damaged_superblock_and_detects_damaged_contents() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    let mut store = Store::open(FaultDisk::new(1 << 20), &mut fs).unwrap();
    write_file(&mut fs, "/a.txt", b"first");
    store.commit(&mut fs).unwrap();
    write_file(&mut fs, "/a.txt", b"second");
    store.commit(&mut fs).unwrap();
    // Generation 2 lives in superblock slot 0 (block 0): damage it.
    let mut disk = store.device.clone();
    disk.live[0][20] ^= 1;
    let mut fs = Fs::new();
    let mut store2 = Store::open(disk, &mut fs).unwrap();
    assert_eq!(store2.generation, 1);
    assert_eq!(read_file(&mut fs, &mut store2, "/a.txt"), b"first");

    // Damage the saved contents of a file: loading reports it instead of returning bad data.
    let mut fs = Fs::new();
    let mut disk = store.device.clone();
    let mut probe = Store::open(disk.clone(), &mut Fs::new()).unwrap();
    let mut fs_probe = Fs::new();
    probe = Store::open(probe.device, &mut fs_probe).unwrap();
    let ino = fs_probe.resolve("/a.txt").unwrap();
    let block = fs_probe.node(ino).unwrap().file().unwrap().extents[0].start;
    disk.live[(block * 8) as usize][0] ^= 0xff;
    let mut store3 = Store::open(disk, &mut fs).unwrap();
    let ino = fs.resolve("/a.txt").unwrap();
    assert!(matches!(store3.load(fs.file_mut(ino).unwrap()), Err(DiskError::Corrupt)));

    // A disk that is neither blank nor a known format is refused.
    let mut junk = FaultDisk::new(1 << 20);
    junk.live[3][7] = 42;
    assert!(matches!(Store::open(junk, &mut Fs::new()), Err(DiskError::Corrupt)));
    let mut tiny = Fs::new();
    assert!(matches!(Store::open(FaultDisk::new(64 << 10), &mut tiny), Err(DiskError::Bounds)));
}

#[test]
fn store_imports_a_disk_in_the_earlier_format() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let (mut old, _) = legacy::Journal::open(FaultDisk::new(8 << 20)).unwrap();
    let files: legacy::Files = vec![("notes.txt".into(), b"from the old format".to_vec()), ("data.bin".into(), pattern(60_000, 1))];
    old.commit(&files).unwrap();
    old.device.power_cut();
    let mut fs = Fs::new();
    let mut store = Store::open(old.device, &mut fs).unwrap();
    assert_eq!(store.generation, 0);
    assert!(fs.unsaved(), "imported files must be saved in the new format");
    assert_eq!(read_file(&mut fs, &mut store, "/notes.txt"), b"from the old format");
    // Until the first new-format save, the old area stays intact and readable.
    let (still, again) = legacy::Journal::open(store.device.clone()).unwrap();
    assert_eq!((still.generation, again), (1, files.clone()));
    store.commit(&mut fs).unwrap();
    store.device.power_cut();
    let mut fs = Fs::new();
    let mut store = Store::open(store.device, &mut fs).unwrap();
    assert_eq!(store.generation, 1);
    assert!(read_file(&mut fs, &mut store, "/data.bin") == pattern(60_000, 1));
}


// Behaviours the earlier tree filesystem tested, on the page-backed one.

#[test]
fn replace_refuses_a_size_the_disk_cannot_hold_without_changes() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    fs.capacity = Some(4); // Four 4 KiB blocks for every file together.
    write_file(&mut fs, "/kept.txt", &pattern(5000, 3));
    let ino = fs.resolve("/kept.txt").unwrap();
    assert_eq!(fs.replace(ino, &pattern(5 * 4096, 4), 9), Err(fs::FsError::NoSpace));
    let mut out = vec![0; 5000];
    assert_eq!(fs.read(ino, 0, &mut out), Ok(5000));
    assert_eq!(out, pattern(5000, 3), "a refused replace leaves the contents alone");
    assert_eq!(fs.blocks, 2);
    fs.replace(ino, &pattern(4 * 4096, 5), 9).unwrap();
    assert_eq!(fs.node(ino).unwrap().size(), 4 * 4096);
    fs.replace(ino, b"small", 9).unwrap();
    assert_eq!(fs.blocks, 1);
}

#[test]
fn saved_revision_follows_changes_and_commits() {
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    let mut fs = Fs::new();
    let mut store = Store::open(FaultDisk::new(8 << 20), &mut fs).unwrap();
    assert_eq!(fs.revision, fs.saved_revision);
    write_file(&mut fs, "/a.txt", b"first");
    assert!(fs.unsaved() && fs.revision != fs.saved_revision);
    store.commit(&mut fs).unwrap();
    assert!(!fs.unsaved());
    assert_eq!(fs.revision, fs.saved_revision);
    fs.mkdir("/later", 9).unwrap();
    assert!(fs.unsaved() && fs.revision != fs.saved_revision);
    assert_eq!(fs.user_files(), 1);
}

#[test]
fn join_resolves_paths_against_a_working_folder() {
    assert_eq!(fs::join("/docs", "a.txt").unwrap(), "/docs/a.txt");
    assert_eq!(fs::join("", "a.txt").unwrap(), "/a.txt");
    assert_eq!(fs::join("/a/b", "..").unwrap(), "/a");
    assert_eq!(fs::join("/a/b", "../../..").unwrap(), "/");
    assert_eq!(fs::join("/a", "/z/./y").unwrap(), "/z/y");
    assert_eq!(fs::join("/a", ".").unwrap(), "/a");
    assert_eq!(fs::join("/a", "bad\u{1}name"), Err(fs::FsError::Invalid));
    assert_eq!(fs::join("/", &"x".repeat(fs::PATH_MAX + 1)), Err(fs::FsError::Invalid));
}

#[test]
fn store_imports_the_earlier_tree_format_with_folders() {
    use kernel_kit::storage::checksum;
    let _guard = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    host_frames();
    // An ATOMFS01 disk whose newest slot holds a version-2 ("ATOMFST2") payload.
    let entries: [(u8, &str, &[u8]); 3] = [(2, "docs/note.txt", b"in a folder"), (1, "empty/", b""), (2, "top.txt", b"at the root")];
    let mut payload = b"ATOMFST2".to_vec();
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (kind, name, data) in entries {
        payload.push(kind);
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        if kind == 2 { payload.extend_from_slice(&(data.len() as u32).to_le_bytes()); }
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(data);
    }
    let mut header = [0u8; 512];
    header[..8].copy_from_slice(b"ATOMFS01");
    header[8..12].copy_from_slice(&2u32.to_le_bytes());
    header[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    header[16..24].copy_from_slice(&1u64.to_le_bytes());
    header[24..32].copy_from_slice(&checksum(&payload).to_le_bytes());
    header[32..36].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    let hash = checksum(&header[..40]);
    header[40..48].copy_from_slice(&hash.to_le_bytes());
    let mut disk = FaultDisk::new(8 << 20);
    let base = legacy::PAYLOAD_SECTORS + 1; // Generation 1 lives in the second slot.
    disk.write_sector(base, &header).unwrap();
    for (index, chunk) in payload.chunks(512).enumerate() {
        let mut sector = [0u8; 512];
        sector[..chunk.len()].copy_from_slice(chunk);
        disk.write_sector(base + 1 + index as u64, &sector).unwrap();
    }
    disk.flush().unwrap();
    let mut fs = Fs::new();
    let mut store = Store::open(disk, &mut fs).unwrap();
    assert!(fs.unsaved(), "imported files must be saved in the new format");
    assert_eq!(read_file(&mut fs, &mut store, "/docs/note.txt"), b"in a folder");
    assert_eq!(read_file(&mut fs, &mut store, "/top.txt"), b"at the root");
    assert!(fs.node(fs.resolve("/empty").unwrap()).unwrap().is_dir());
    store.commit(&mut fs).unwrap();
    store.device.power_cut();
    let mut fs = Fs::new();
    let mut store = Store::open(store.device, &mut fs).unwrap();
    assert_eq!(store.generation, 1);
    assert_eq!(read_file(&mut fs, &mut store, "/docs/note.txt"), b"in a folder");
}

#[test]
fn process_argument_encoding_limits_and_empty_values() {
    use kernel_kit::{arguments::pack, abi::*};
    assert_eq!(pack("a", b"one\0\0two words\0").unwrap(), b"a\0one\0\0two words\0");
    assert!(pack("a", b"unterminated").is_err());
    assert!(pack("a", &[255, 0]).is_err());
    assert!(pack("", b"").is_err());
    assert!(pack("a\0b", b"").is_err());
    assert!(pack("a", &[0; MAX_ARGS]).is_err());
    assert!(pack("a", &[0; MAX_ARGS - 1]).is_ok());
    let mut extra = vec![b'x'; MAX_ARG_BYTES - 2]; *extra.last_mut().unwrap() = 0;
    assert_eq!(pack("a", &extra).unwrap().len(), MAX_ARG_BYTES);
    extra.insert(0, b'x'); assert!(pack("a", &extra).is_err());
}
#[test]
fn launch_quoting_preserves_empty_spaces_and_escapes() {
    use kernel_kit::arguments::words;
    assert_eq!(words(r#"worker.elf plain "two words" '' 'a"b' c\ d "q\"x""#).unwrap(),
               ["worker.elf", "plain", "two words", "", "a\"b", "c d", "q\"x"]);
    for bad in ["", "   ", "worker 'unfinished", "worker \\", "worker \0"] { assert!(words(bad).is_err()); }
}
#[test]
fn process_snapshot_states_names_and_zeroed_unused_records() {
    use kernel_kit::abi::*;
    let mut scheduler = scheduler::Scheduler::new();
    let mut context = context::Context::new(99, 0, 0, 0);
    context.parent = 8; context.arguments = b"worker.elf\0private argument\0".to_vec();
    context.state = context::TaskState::Blocked; context.wait_for = Some(7);
    scheduler.spawn(context).unwrap();
    let (rows, count) = scheduler.snapshot(); assert_eq!(count, 1);
    assert_eq!((rows[0].pid, rows[0].parent, rows[0].state), (99, 8, PROCESS_WAITING));
    assert_eq!(&rows[0].name[..11], b"worker.elf\0"); assert!(rows[0].name[11..].iter().all(|&b| b == 0));
    assert_eq!(rows[1].pid, 0); assert!(rows[1].name.iter().all(|&b| b == 0));
    scheduler.task_mut(99).unwrap().wait_for = None;
    assert_eq!(scheduler.snapshot().0[0].state, PROCESS_SLEEPING);
    scheduler.task_mut(99).unwrap().state = context::TaskState::Terminated;
    scheduler.task_mut(99).unwrap().exit_code = KILLED_STATUS;
    assert_eq!(scheduler.snapshot().0[0].state, PROCESS_EXITED);
    assert_eq!(scheduler.snapshot().0[0].exit_code, KILLED_STATUS);
}

#[test]
fn conversation_partner_is_the_pid_or_sub_function_and_nothing_else() {
    use kernel_kit::abi::*;
    assert_eq!(conversation::partner(SYS_IPC_SEND, 99), 99);
    assert_eq!(conversation::partner(SYS_WAIT, 4), 4);
    assert_eq!(conversation::partner(SYS_SENSE, 2), 2);
    assert_eq!(conversation::partner(SYS_WRITE_BUFFER, 0x1000), 0);
    assert_eq!(conversation::partner(SYS_FILE_WRITE, 3), 0);
    assert_ne!(conversation::SHARED_IDENTITY, 0, "0 would mean 'use the pid' to the sensor");
}
