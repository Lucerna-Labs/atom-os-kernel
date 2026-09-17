// Native diagnostics using the actual, unchanged source modules.
// No privileged IRQ, port-I/O, or paging instruction is executed here.
#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
extern crate self as kernel_kit;
#[path = "source/kernel-kit/src/atoms.rs"] pub mod atoms;
#[path = "source/kernel-kit/src/context.rs"] pub mod context;
#[path = "source/kernel-kit/src/elf.rs"] pub mod elf;
#[path = "source/kernel-kit/src/fs.rs"] pub mod fs;
#[path = "source/kernel-kit/src/io.rs"] pub mod io;
#[path = "source/kernel-kit/src/keyboard.rs"] pub mod keyboard;
#[path = "source/kernel-kit/src/memory.rs"] pub mod memory;
#[path = "source/kernel-kit/src/slab.rs"] pub mod slab;
#[path = "source/kernel-orchestrator/src/scheduler.rs"] pub mod scheduler;

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
    assert_eq!(scheduler.switch_context(0xdead), 0x4000);
    assert_eq!(scheduler.current_task().unwrap().id, 2);
    assert_eq!(scheduler.switch_context(0x4110), 0x1000);
    assert_eq!(scheduler.current_task().unwrap().id, 1);
    assert_eq!(scheduler.switch_context(0x1110), 0x4110);
    assert_eq!(scheduler.switch_context(0x4220), 0x1110);
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
    for index in 0..4096 {
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
