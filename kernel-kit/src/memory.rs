use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
use core::arch::asm;
use crate::atoms::{combine, compare};

/// A pure mathematical spinlock using the `compare` and swap concept (via AtomicBool)
pub struct Spinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T> Sync for Spinlock<T> {}
unsafe impl<T> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> &mut T {
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        unsafe { &mut *self.data.get() }
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// GAP 2: IRQ-aware lock stack
//
// Stage contract (ATOM-STACK-KERNEL-DESIGN.md Appendix B):
//
//   STAGE if_save
//     in_shape:    (lock_addr, calling_context)
//     in_invariant:{interrupts may fire}
//     op:          read RFLAGS.IF into a u8 (0 or 1)
//     out_shape:   (lock_addr, calling_context, saved_if)
//     preserves:   {interrupts may fire}  (observed, not yet changed)
//     destroys:    ∅
//     introduces:  {saved_if is the pre-lock IF state}
//
//   STAGE irq_disable
//     in_shape:    (lock_addr, calling_context, saved_if)
//     in_invariant:{saved_if recorded}
//     op:          cli
//     out_shape:   (lock_addr, calling_context, saved_if, irqs_off=true)
//     preserves:   {saved_if recorded}
//     destroys:    {interrupts may fire}
//     introduces:  {irqs_off=true, atomic w.r.t. IRQ context}
//
//   STAGE cas_acquire
//     in_shape:    (lock_addr, ..., irqs_off=true)
//     in_invariant:{irqs_off=true}  ← REQUIRED, hazarded if violated
//     op:          atomic CAS on lock_addr (spin while contended)
//     out_shape:   (lock held, irqs_off=true)
//     preserves:   {irqs_off=true}
//     destroys:    ∅
//     introduces:  {lock held}
//
//   STAGE release_and_restore
//     in_shape:    (lock held, saved_if, irqs_off=true)
//     in_invariant:{lock held}
//     op:          release = false; if saved_if != 0 then sti
//     out_shape:   (caller resumed)
//     preserves:   ∅
//     destroys:    {lock held, irqs_off}
//     introduces:  {interrupts may fire} (iff saved_if)
//
// Hazard explicitly designed around (operator warning 2026-07-17):
// cas_acquire MUST come AFTER irq_disable. Reversing them opens a
// one-instruction window where an IRQ can fire and re-enter on the
// same lock → deadlock. The contract encodes this as
// in_invariant(cas_acquire) = {irqs_off=true}.
//
// Two lock flavors, deliberately not unified:
//   * Spinlock<T>      — for non-IRQ-crossing callers (ROOT_FS, CURSOR).
//                        Lowest latency, no IRQ cost.
//   * IrqSpinlock<T>   — for IRQ-crossing callers (SERIAL1, KEYBOARD_BUFFER,
//                        FRAME_ALLOCATOR). Correct under re-entrant IRQ.
// ---------------------------------------------------------------------------

/// Read the current RFLAGS and return bit 9 (the Interrupt Flag) as 0 or 1.
/// Pure read — does not change IF.
#[inline]
pub fn read_if() -> u8 {
    let flags: u64;
    unsafe { asm!("pushfq; pop {}", out(reg) flags, options(preserves_flags)); }
    ((flags >> 9) & 1) as u8
}

/// Atomically disable maskable interrupts. Pair with restore_if(saved_if).
#[inline]
pub fn disable_irq() {
    unsafe { asm!("cli", options(nomem, nostack, preserves_flags)); }
}

/// Conditionally re-enable maskable interrupts iff `saved_if != 0`.
/// Restores the IF state recorded by a prior read_if() so a caller that
/// was already IRQ-disabled is not re-enabled by us.
#[inline]
pub fn restore_if(saved_if: u8) {
    if saved_if != 0 {
        unsafe { asm!("sti", options(nomem, nostack, preserves_flags)); }
    }
}

/// IRQ-aware spinlock. Use instead of Spinlock when the lock can be
/// acquired from both thread context AND interrupt context (or when you
/// are unsure). Costs one RFLAGS read + one cli on acquire, one
/// conditional sti on release.
pub struct IrqSpinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T> Sync for IrqSpinlock<T> {}
unsafe impl<T> Send for IrqSpinlock<T> {}

impl<T> IrqSpinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquire the lock with IRQs disabled. Returns a reference AND the
    /// saved IF state, which MUST be passed back to unlock().
    ///
    /// The caller MUST treat the returned `&mut T` as borrowed only until
    /// the matching unlock() — there is no RAII guard in no_std here.
    pub fn lock(&self) -> (&mut T, u8) {
        // STAGE if_save
        let saved_if = read_if();
        // STAGE irq_disable
        disable_irq();
        // STAGE cas_acquire — REQUIRES irqs_off=true (encoded above).
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            // While spinning, IRQs are already off; just pause.
            core::hint::spin_loop();
        }
        // STAGE do_crit_sec happens in the caller between lock() and unlock().
        (unsafe { &mut *self.data.get() }, saved_if)
    }

    /// Release the lock and restore the caller's prior IF state.
    pub fn unlock(&self, saved_if: u8) {
        // STAGE release_and_restore
        self.locked.store(false, Ordering::Release);
        restore_if(saved_if);
    }
}

/// A bump allocator that projects a flat memory block into sub-allocations.
pub struct BumpAllocator {
    pub heap_start: usize,
    pub heap_end: usize,
    pub next: usize,
    pub allocations: usize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    pub fn init(&mut self, start: usize, size: usize) {
        self.heap_start = start;
        self.heap_end = start + size;
        self.next = start;
    }
}

/// The Global Heap wrapper enforcing Atom Doctrine.
pub struct AtomHeap(pub Spinlock<BumpAllocator>);

unsafe impl GlobalAlloc for AtomHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap = self.0.lock();
        
        // Project the current 'next' pointer to respect alignment
        let align = layout.align();
        let alloc_start = combine(heap.next, align, |n, a| {
            let remainder = n % a;
            if remainder == 0 { n } else { n + a - remainder }
        });
        
        let alloc_end = alloc_start.saturating_add(layout.size());
        
        // Compare to ensure we don't overflow the heap boundary
        if alloc_end <= heap.heap_end {
            heap.next = alloc_end;
            heap.allocations += 1;
            let ptr = alloc_start as *mut u8;
            self.0.unlock();
            ptr
        } else {
            self.0.unlock();
            core::ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // C3 fix: this is a pure bump allocator. The old code reset `next` to
        // `heap_start` once the live-allocation count dropped to zero, which
        // collapsed the ENTIRE heap and silently aliased every still-reachable
        // allocation (static Vecs, injected payloads, FS nodes, kernel stacks)
        // with future allocations — a use-after-free. A bump allocator cannot
        // support individual frees correctly;dealloc is therefore a no-op here.
        // Kernel heap memory is reclaimed only by never reusing it (we have a
        // large enough pool). A proper free-list allocator is future work.
        let _ = self.0.lock();
        self.0.unlock();
    }
}

// We will also keep a legacy MemoryPool for the Orchestrator's virtual mapping simulation (until phased out).
pub struct MemoryPool {
    pub blocks: [bool; 1024],
}

impl MemoryPool {
    pub const fn new() -> Self {
        Self {
            blocks: [true; 1024],
        }
    }
    
    pub fn allocate(&mut self) -> Option<usize> {
        for i in 0..1024 {
            if self.blocks[i] {
                self.blocks[i] = false;
                return Some(i);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Physical frame allocator.
//
// Draws 4 KiB frames from a region of physical RAM that the bootloader marked
// Usable. Such regions are accessible via paging::phys_to_virt() (the
// bootloader's physical_memory_offset mapping), so page-table pages allocated
// here can be both written (via phys_to_virt) and loaded into CR3 (via their
// raw physical address).
//
// The previous implementation carved a frame pool out of a static [u8; 16MiB]
// array that overlapped the kernel image's load range. That pool sat in
// physical memory the bootloader had NOT marked Usable, and which was NOT
// reliably covered by phys_to_virt — so duplicate_pml4 wrote through an
// address that resolved to wrong/no RAM, producing corrupted PML4 copies and
// a triple-fault on CR3 switch.
// ---------------------------------------------------------------------------

/// Highest physical address the allocator can track: 16 GiB. Frames are
/// indexed by physical frame number, so the bitmap also spans the holes
/// between regions (e.g. QEMU's PCI hole below 4 GiB); those bits stay clear.
pub const MAX_PHYS_BYTES: u64 = 16 << 30;
/// Number of 4 KiB frames covered by the bitmap (4 Mi frames = 512 KiB of bits).
pub const FRAMES_MAX: usize = (MAX_PHYS_BYTES / 4096) as usize;
const WORDS: usize = FRAMES_MAX / 64;
/// Usable regions remembered so `free_frame` can reject foreign addresses.
pub const REGIONS_MAX: usize = 32;

pub struct FrameAllocator {
    /// One bit per physical frame: 1 = free. Zero-initialised, so the
    /// bitmap lives in .bss and every frame starts out unavailable.
    bits: [u64; WORDS],
    /// (first frame, end frame) of each region handed to `add_region`.
    regions: [(usize, usize); REGIONS_MAX],
    region_count: usize,
    /// Number of bitmap words that can contain a free bit.
    words: usize,
    /// No free bit exists in a word below this index.
    hint: usize,
    free: usize,
    total: usize,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self { bits: [0; WORDS], regions: [(0, 0); REGIONS_MAX], region_count: 0,
            words: 0, hint: 0, free: 0, total: 0 }
    }

    /// Reset the allocator to a single pool of `num_frames` frames at `base_phys`.
    pub fn init(&mut self, base_phys: usize, num_frames: usize) {
        self.bits[..self.words].fill(0);
        self.region_count = 0; self.words = 0; self.hint = 0; self.free = 0; self.total = 0;
        self.add_region(base_phys, num_frames);
    }

    /// Add `num_frames` frames starting at `base_phys` to the pool. The range
    /// MUST be RAM the bootloader marked Usable (so phys_to_virt reaches it)
    /// and MUST NOT overlap the kernel image, heap, stack or bootloader data.
    /// Returns how many frames were added; ranges are clipped to
    /// MAX_PHYS_BYTES, and overlapping or excess regions are ignored.
    pub fn add_region(&mut self, base_phys: usize, num_frames: usize) -> usize {
        let first = base_phys.div_ceil(4096);
        let end = (base_phys / 4096).saturating_add(num_frames).min(FRAMES_MAX);
        if first >= end || self.region_count == REGIONS_MAX
            || self.regions[..self.region_count].iter().any(|&(s, e)| first < e && s < end) {
            return 0;
        }
        self.regions[self.region_count] = (first, end);
        self.region_count += 1;
        for frame in first..end { self.bits[frame / 64] |= 1 << (frame % 64); }
        self.words = self.words.max(end.div_ceil(64));
        self.hint = self.hint.min(first / 64);
        self.free += end - first;
        self.total += end - first;
        end - first
    }

    pub fn free_count(&self) -> usize { self.free }
    pub fn total_count(&self) -> usize { self.total }
    pub fn region_count(&self) -> usize { self.region_count }
    /// Physical (start, end) byte ranges of the regions in the pool.
    pub fn regions(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.regions[..self.region_count].iter().map(|&(s, e)| (s as u64 * 4096, e as u64 * 4096))
    }

    fn owns(&self, frame: usize) -> bool {
        self.regions[..self.region_count].iter().any(|&(s, e)| s <= frame && frame < e)
    }

    /// Allocate one 4 KiB frame. Returns its PHYSICAL address (apply
    /// paging::phys_to_virt to access its contents).
    pub fn alloc_frame(&mut self) -> Option<u64> {
        for word in self.hint..self.words {
            let bits = self.bits[word];
            if bits != 0 {
                let frame = word * 64 + bits.trailing_zeros() as usize;
                self.bits[word] &= !(1 << (frame % 64));
                self.free -= 1;
                self.hint = word;
                return Some(frame as u64 * 4096);
            }
        }
        self.hint = self.words;
        None
    }

    /// Allocate `n` CONTIGUOUS 4 KiB frames. Returns the physical
    /// address of the first frame, or None if no contiguous run of
    /// length `n` is free. Use this when the caller needs the frames
    /// to be physically adjacent (e.g. mapping a multi-page ELF
    /// segment where map_segment walks vaddr in 4K steps and expects
    /// each successive page to be phys_base + i*4K).
    pub fn alloc_contiguous(&mut self, n: usize) -> Option<u64> {
        if n == 0 || n > self.free { return None; }
        let mut run_start = 0;
        let mut run_len = 0;
        let mut word = self.hint;
        while word < self.words {
            let bits = self.bits[word];
            if bits == 0 { run_len = 0; word += 1; continue; }
            if bits == u64::MAX && run_len + 64 < n {
                if run_len == 0 { run_start = word * 64; }
                run_len += 64; word += 1; continue;
            }
            for bit in 0..64 {
                if bits & (1 << bit) == 0 { run_len = 0; continue; }
                if run_len == 0 { run_start = word * 64 + bit; }
                run_len += 1;
                if run_len == n {
                    for frame in run_start..run_start + n { self.bits[frame / 64] &= !(1 << (frame % 64)); }
                    self.free -= n;
                    return Some(run_start as u64 * 4096);
                }
            }
            word += 1;
        }
        None
    }

    /// Free a frame previously returned by `alloc_frame`/`alloc_contiguous`.
    /// Unaligned, foreign and already-free addresses are ignored.
    pub fn free_frame(&mut self, phys: u64) {
        if phys % 4096 != 0 { return; }
        let frame = (phys / 4096) as usize;
        if frame >= FRAMES_MAX || !self.owns(frame) { return; }
        let mask = 1 << (frame % 64);
        if self.bits[frame / 64] & mask != 0 { return; }
        self.bits[frame / 64] |= mask;
        self.free += 1;
        self.hint = self.hint.min(frame / 64);
    }
}

/// Global frame allocator instance. Owned by the kernel crate; initialized once.
pub static FRAME_ALLOCATOR: IrqSpinlock<FrameAllocator> = IrqSpinlock::new(FrameAllocator::new());
