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
    unsafe { asm!("pushfq; pop {}", out(reg) flags, options(nomem, nostack, preserves_flags)); }
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

/// Maximum number of frames the allocator will track. 16384 frames = 64 MiB,
/// which comfortably covers QEMU's default 128 MiB minus kernel/bootloader
/// overhead, and keeps the bitmap small (16 KiB).
pub const FRAMES_MAX: usize = 16384;

pub struct FrameAllocator {
    base: usize,            // physical address of the first frame in the pool
    count: usize,           // number of frames actually available (<= FRAMES_MAX)
    frames: [bool; FRAMES_MAX], // true = free, false = in use
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self {
            base: 0,
            count: 0,
            frames: [true; FRAMES_MAX],
        }
    }

    /// Reserve a frame pool at physical address `base`, covering `num_frames`
    /// 4 KiB frames. Both `base` and the frames MUST be in a region the
    /// bootloader marked Usable (so phys_to_virt can access them) and MUST NOT
    /// overlap the kernel image, heap, stack, or bootloader structures.
    /// Called once from _start before any paging operation.
    pub fn init(&mut self, base_phys: usize, num_frames: usize) {
        self.base = base_phys;
        self.count = if num_frames < FRAMES_MAX { num_frames } else { FRAMES_MAX };
        // Mark only the first `count` frames free; the rest are out of pool.
        for i in 0..FRAMES_MAX {
            self.frames[i] = i < self.count;
        }
    }

    /// Allocate one 4 KiB frame. Returns its PHYSICAL address (apply
    /// paging::phys_to_virt to access its contents).
    pub fn alloc_frame(&mut self) -> Option<u64> {
        for i in 0..self.count {
            if self.frames[i] {
                self.frames[i] = false;
                return Some((self.base + i * 4096) as u64);
            }
        }
        None
    }

    /// Allocate `n` CONTIGUOUS 4 KiB frames. Returns the physical
    /// address of the first frame, or None if no contiguous run of
    /// length `n` is free. Use this when the caller needs the frames
    /// to be physically adjacent (e.g. mapping a multi-page ELF
    /// segment where map_segment walks vaddr in 4K steps and expects
    /// each successive page to be phys_base + i*4K).
    pub fn alloc_contiguous(&mut self, n: usize) -> Option<u64> {
        if n == 0 {
            return None;
        }
        // Linear scan for a run of `n` free frames.
        let mut run_start: usize = 0;
        let mut run_len: usize = 0;
        for i in 0..self.count {
            if self.frames[i] {
                if run_len == 0 {
                    run_start = i;
                }
                run_len += 1;
                if run_len == n {
                    // Found — mark them all allocated.
                    for j in run_start..(run_start + n) {
                        self.frames[j] = false;
                    }
                    return Some((self.base + run_start * 4096) as u64);
                }
            } else {
                run_len = 0;
            }
        }
        None
    }

    /// Free a frame previously returned by `alloc_frame`.
    pub fn free_frame(&mut self, phys: u64) {
        if self.count == 0 {
            return;
        }
        let off = phys as usize;
        if off < self.base {
            return;
        }
        let i = (off - self.base) / 4096;
        if i < self.count && (self.base + i * 4096) == off {
            self.frames[i] = true;
        }
    }
}

/// Global frame allocator instance. Owned by the kernel crate; initialized once.
pub static FRAME_ALLOCATOR: IrqSpinlock<FrameAllocator> = IrqSpinlock::new(FrameAllocator::new());
