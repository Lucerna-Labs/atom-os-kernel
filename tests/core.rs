// Native diagnostics using the actual, unchanged source modules.
// No privileged IRQ, port-I/O, or paging instruction is executed here.
#![allow(dead_code, unused_imports, unused_variables)]
extern crate kernel_kit;
use kernel_kit::{context, elf, fs, keyboard, memory, slab};
#[path = "../kernel-orchestrator/src/scheduler.rs"] mod scheduler;

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

#[test]
fn ramfs_file_handle_survives_directory_growth() {
    let mut root = fs::AtomNode::Directory(Vec::with_capacity(1));
    let original = root.get_or_create_file("held.txt").unwrap() as usize;
    for index in 0..64 {
        root.get_or_create_file(&format!("new-{index}.txt")).unwrap();
        let current = root.get_or_create_file("held.txt").unwrap() as usize;
        assert_eq!(original, current,
            "creating file {index} relocated the Vec object referenced by the existing FD");
    }
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

use kernel_kit::virtio_blk::{BlockDevice, DiskError};
use kernel_kit::storage::{Journal, Files, REQUIRED_SECTORS, PAYLOAD_SECTORS};

// A fault-injection fixture for the production journal codec. Real virtio and
// reboot acceptance are separate VM tests; this fixture models volatile cache.
#[derive(Clone)]
struct FaultDisk { live: Vec<[u8; 512]>, durable: Vec<[u8; 512]>, operations: usize, fail_at: Option<usize> }
impl FaultDisk {
    fn new() -> Self {
        let data = vec![[0; 512]; REQUIRED_SECTORS as usize];
        Self { live: data.clone(), durable: data, operations: 0, fail_at: None }
    }
    fn operation(&mut self) -> Result<(), DiskError> {
        let at = self.operations; self.operations += 1;
        if self.fail_at == Some(at) { Err(DiskError::Io) } else { Ok(()) }
    }
    fn power_cut(&mut self) { self.live = self.durable.clone(); self.fail_at = None; }
}
impl BlockDevice for FaultDisk {
    fn sectors(&self) -> u64 { self.live.len() as u64 }
    fn read_sector(&mut self, sector: u64, bytes: &mut [u8; 512]) -> Result<(), DiskError> { *bytes = self.live[sector as usize]; Ok(()) }
    fn write_sector(&mut self, sector: u64, bytes: &[u8; 512]) -> Result<(), DiskError> { self.operation()?; self.live[sector as usize] = *bytes; Ok(()) }
    fn flush(&mut self) -> Result<(), DiskError> { self.operation()?; self.durable = self.live.clone(); Ok(()) }
}

#[test]
fn journal_preserves_exact_binary_data_across_power_cycle() {
    let (mut journal, initial) = Journal::open(FaultDisk::new()).unwrap(); assert!(initial.is_empty());
    let files: Files = vec![("binary.dat".into(), (0..4096).map(|n| (n % 251) as u8).collect())];
    journal.commit(&files).unwrap(); journal.device.power_cut();
    let (restored, read) = Journal::open(journal.device).unwrap();
    assert_eq!(restored.generation, 1); assert_eq!(read, files);
}

#[test]
fn interrupted_commit_retains_the_previous_generation() {
    let original: Files = vec![("saved.txt".into(), b"original".to_vec())];
    let replacement: Files = vec![("saved.txt".into(), vec![0x7e; 1100])];
    let (mut base, _) = Journal::open(FaultDisk::new()).unwrap(); base.commit(&original).unwrap();
    // Three payload sectors, flush, header sector, final flush: fail each step.
    for failure in 0..6 {
        let mut disk = base.device.clone(); disk.operations = 0; disk.fail_at = Some(failure);
        let (mut journal, _) = Journal::open(disk).unwrap();
        assert!(journal.commit(&replacement).is_err()); journal.device.power_cut();
        let (restored, read) = Journal::open(journal.device).unwrap();
        assert_eq!(restored.generation, 1, "failure={failure}"); assert_eq!(read, original);
    }
}

#[test]
fn journal_recovers_from_corrupt_newest_header() {
    let (mut journal, _) = Journal::open(FaultDisk::new()).unwrap();
    let old: Files = vec![("keep.txt".into(), b"old".to_vec())];
    journal.commit(&old).unwrap();
    journal.commit(&vec![("keep.txt".into(), b"new".to_vec())]).unwrap();
    journal.device.live[0][40] ^= 1;
    let (restored, files) = Journal::open(journal.device).unwrap();
    assert_eq!(restored.generation, 1); assert_eq!(files, old);
}

#[test]
fn journal_rejects_unknown_disk_and_invalid_file_metadata() {
    let mut disk = FaultDisk::new(); disk.live[0][0] = 42;
    assert!(matches!(Journal::open(disk), Err(DiskError::Corrupt)));
    for files in [vec![("shell.elf".into(), vec![1])], vec![("../bad".into(), vec![])],
                  vec![("a".into(), vec![]), ("a".into(), vec![])], vec![("huge".into(), vec![0; 65537])]] {
        assert!(kernel_kit::storage::encode(&files).is_err());
    }
    let mut payload = vec![0; PAYLOAD_SECTORS as usize * 512 + 1];
    assert!(kernel_kit::storage::decode(&payload).is_err());
    payload.truncate(4); payload[..4].copy_from_slice(&129u32.to_le_bytes());
    assert!(kernel_kit::storage::decode(&payload).is_err());
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
fn ramfs_remove_and_rename_keep_other_handles_stable() {
    let mut root = fs::AtomNode::Directory(Vec::new());
    let kept = root.get_or_create_file("kept.txt").unwrap();
    unsafe { (*kept).extend_from_slice(b"kept"); }
    let moved = root.get_or_create_file("old.txt").unwrap();
    unsafe { (*moved).extend_from_slice(b"moved"); }
    root.get_or_create_file("gone.txt").unwrap();

    assert!(root.remove("gone.txt"));
    assert!(!root.remove("gone.txt"));
    assert!(root.file("gone.txt").is_none());
    assert_eq!(root.file_address("kept.txt"), Some(kept as u64));

    assert!(!root.rename("old.txt", "kept.txt"), "rename must not replace an existing file");
    assert!(!root.rename("old.txt", ""));
    assert!(!root.rename("old.txt", "a/b"));
    assert!(!root.rename("missing.txt", "new.txt"));
    assert!(root.rename("old.txt", "new.txt"));
    assert!(root.file("old.txt").is_none());
    assert_eq!(root.file_address("new.txt"), Some(moved as u64), "rename relocated the data object");
    assert_eq!(root.file("new.txt").unwrap().as_slice(), b"moved");
    assert_eq!(unsafe { (*kept).as_slice() }, b"kept");
}

#[test]
fn builtin_programs_are_excluded_from_snapshots() {
    for name in fs::BUILTINS {
        assert!(kernel_kit::storage::builtin(name));
        assert!(kernel_kit::storage::encode(&vec![(name.to_string(), Vec::new())]).is_err());
    }
    assert!(!fs::builtin("notes.txt"));
}
