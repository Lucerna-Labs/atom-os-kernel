use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
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
        let heap = self.0.lock();
        
        heap.allocations -= 1;
        
        // If all allocations are freed, we can mathematically project `next` back to `start`
        if compare(&heap.allocations, &1) {
            heap.next = heap.heap_start;
        }
        
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
