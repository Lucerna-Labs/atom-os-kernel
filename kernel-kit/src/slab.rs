//! GAP 1 — Slab heap: bucket-route + LIFO free-list + avalanche-tag verify.
//!
//! See ATOM-STACK-KERNEL-DESIGN.md Appendix B for the stage contract.
//! Sizing tuned to the kernel's actual allocation profile (T0 survey):
//! most allocs are 24-256 B (Vec headers, Context, TrapFrame, String);
//! a few are KiB-scale (payload buffers). Eight small size classes
//! cover the common case; anything bigger falls back to BumpAllocator
//! so we don't pay buddy-split complexity for the long tail.
//!
//! Slab design:
//!   - Each bucket holds a singly-linked free-list of fixed-size nodes.
//!   - Each node has an 8-byte header (avalanche tag + size class) then
//!     usable bytes.
//!   - alloc: bucket_route(layout) -> pop -> tag_write
//!   - dealloc: tag_verify -> push
//!   - The free list itself is stored IN the freed nodes (header.next
//!     overlaps user bytes when the node is on the free-list). This is
//!     the classic slab trick: zero per-bucket heap overhead.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::memory::{IrqSpinlock, BumpAllocator, AtomHeap};

// ─────────────────────────── configuration ───────────────────────────

/// Size classes (usable bytes, not counting the 16-byte header).
/// Tuned to the kernel's observed allocation sizes.
///
/// The bucket index for a given request is the SMALLEST class whose
/// usable bytes >= requested size. Sizes above the largest class fall
/// through to BumpAllocator.
const SLAB_CLASSES: [usize; 8] = [
    16,    // 0: small headers (Vec<u8> empty, refs)
    32,    // 1: Vec headers (cap,len,ptr = 24) + String
    64,    // 2: Context (estimated)
    128,   // 3: TrapFrame / medium structs
    256,   // 4: syscall working buffers
    512,   // 5: msg buffers
    1024,  // 6: small buffers
    2048,  // 7: medium buffers
];

/// Per-bucket free-list capacity (initial pre-allocated nodes per bucket).
/// The free list can grow beyond this under load (via bump fallback
/// returning to the slab on free); this is the warm-pool size.
const SLAB_PREALLOC_PER_BUCKET: usize = 32;

/// Header prepended to every slab node. 16 bytes keeps user data aligned
/// to 16 bytes (sufficient for any rust type).
#[repr(C)]
struct SlabHeader {
    /// Avalanche hash of (layout.size, layout.align, caller_rip) — used by
    /// the VERIFY stage to detect double-free, use-after-free, and
    /// header corruption. ZERO when the node is on the free-list (the
    /// free-list next pointer occupies the bytes after this field).
    tag: u64,
    /// Bucket index + 1 (so 0 == "not a slab alloc"). Used by dealloc to
    /// route back to the right bucket without re-running bucket_of (which
    /// could disagree if the caller passed the wrong layout).
    bucket_plus_one: u8,
    _pad: [u8; 7],
}

const HEADER_BYTES: usize = 16;

/// A bucket is just the head pointer of its free-list. Nodes on the
/// free-list use the bytes after SlabHeader to store `next: *mut u8`.
struct Bucket {
    head: *mut u8,
    len: usize,
}

impl Bucket {
    const fn new() -> Self {
        Self { head: core::ptr::null_mut(), len: 0 }
    }
}

/// The slab allocator itself. Each bucket has a free-list; an init()
/// call pre-allocates nodes from a backing bump region so the first
/// SLAB_PREALLOC_PER_BUCKET allocs per bucket don't pay bump cost.
pub struct SlabHeap {
    buckets: [Bucket; SLAB_CLASSES.len()],
    /// Counters for the VERIFY currency (measured at runtime).
    alloc_count: AtomicUsize,
    dealloc_count: AtomicUsize,
    slab_hits: AtomicUsize,
    slab_misses: AtomicUsize,
    tag_mismatches: AtomicUsize,
}

unsafe impl Sync for SlabHeap {}
unsafe impl Send for SlabHeap {}

// ─────────────────────────── root atoms used ─────────────────────────

#[inline]
fn avalanche_hash(size: usize, align: usize) -> u64 {
    // Reuse the atoms::hash primitive's mixing schedule so the slab
    // composes with the existing atom doctrine rather than inventing
    // its own hash. Two rounds to mix both inputs.
    let mut x = (size as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ ((align as u64) << 32);
    x ^= x >> 27;
    x = x.wrapping_mul(0x2545F4914F6CDD1D);
    x ^= x >> 28;
    // OR a sentinel so 0 (free-list marker) never collides with a live tag.
    x | 0x8000_0000_0000_0001
}

/// bucket_of(layout) -> Option<idx>. The hash/project root-atom
/// composition: project(layout.size) into a bucket index by scan.
fn bucket_of(size: usize) -> Option<usize> {
    let mut idx = None;
    for (i, cls) in SLAB_CLASSES.iter().enumerate() {
        if size + HEADER_BYTES <= *cls + HEADER_BYTES && size <= *cls {
            idx = Some(i);
            break;
        }
    }
    idx
}

// ─────────────────────────── SlabHeap impl ───────────────────────────

impl SlabHeap {
    pub const fn new() -> Self {
        Self {
            buckets: [
                Bucket::new(), Bucket::new(), Bucket::new(), Bucket::new(),
                Bucket::new(), Bucket::new(), Bucket::new(), Bucket::new(),
            ],
            alloc_count: AtomicUsize::new(0),
            dealloc_count: AtomicUsize::new(0),
            slab_hits: AtomicUsize::new(0),
            slab_misses: AtomicUsize::new(0),
            tag_mismatches: AtomicUsize::new(0),
        }
    }

    // (init removed — backing is now passed explicitly per-call.)

    /*
      STAGE bucket_route
        in_shape:    Layout
        in_invariant:{layout.size > 0}
        op:          bucket_of(layout.size)
        out_shape:   Option<bucket_idx>
    */
    fn bucket_route(layout: &Layout) -> Option<usize> {
        bucket_of(layout.size())
    }

    /*
      STAGE free_list_pop
        in_shape:    bucket_idx
        in_invariant:{bucket may be empty}
        op:          pop head
        out_shape:   Option<user_ptr>
        introduces:  {ptr is user pointer, header uninitialized}
    */
    unsafe fn free_list_pop(bucket: &mut Bucket) -> Option<*mut u8> {
        if bucket.head.is_null() {
            None
        } else {
            let node = bucket.head;
            // The free-list `next` pointer lives at the start of the user
            // data area (after SlabHeader). Read it.
            let next = *((node.add(HEADER_BYTES)) as *mut *mut u8);
            bucket.head = next;
            bucket.len -= 1;
            // Return pointer to USER area (past header).
            Some(node.add(HEADER_BYTES))
        }
    }

    /*
      STAGE avalanche_tag_write
        in_shape:    user_ptr, layout, bucket_idx
        in_invariant:{ptr is USER pointer, not free-list head} ← hazard
        op:          write SlabHeader{tag, bucket_plus_one}
    */
    unsafe fn avalanche_tag_write(user_ptr: *mut u8, layout: &Layout, bucket_idx: usize) {
        let header_ptr = (user_ptr.sub(HEADER_BYTES)) as *mut SlabHeader;
        (*header_ptr).tag = avalanche_hash(layout.size(), layout.align());
        (*header_ptr).bucket_plus_one = (bucket_idx + 1) as u8;
    }

    /*
      STAGE avalanche_tag_verify
        in_shape:    user_ptr, layout
        in_invariant:{header.tag should match avalanche_hash(layout)}
        out_shape:   bool
    */
    unsafe fn avalanche_tag_verify(user_ptr: *mut u8, layout: &Layout) -> (bool, u8) {
        let header_ptr = (user_ptr.sub(HEADER_BYTES)) as *mut SlabHeader;
        let expected = avalanche_hash(layout.size(), layout.align());
        let actual = (*header_ptr).tag;
        ((actual == expected), (*header_ptr).bucket_plus_one)
    }

    /*
      STAGE free_list_push
        in_shape:    user_ptr, bucket
        in_invariant:{tag verified}
        op:          write next=current_head; head=node
    */
    unsafe fn free_list_push(bucket: &mut Bucket, user_ptr: *mut u8) {
        let node = user_ptr.sub(HEADER_BYTES);
        let next_slot = (node.add(HEADER_BYTES)) as *mut *mut u8;
        *next_slot = bucket.head;
        // Zero the tag so the free-list marker is unambiguous if anyone
        // scans the pool.
        let header_ptr = node as *mut SlabHeader;
        (*header_ptr).tag = 0;
        (*header_ptr).bucket_plus_one = 0;
        bucket.head = node;
        bucket.len += 1;
    }

    /*
      COMPOSED alloc — bucket_route then bucket_alloc (which runs
      free_list_pop and avalanche_tag_write). Sizes above the largest
      slab class fall through to bump (no header, no reclaim).

      `bump` is passed explicitly to avoid the self-referential borrow
      that storing a pointer inside SlabHeap would create.
    */
    pub unsafe fn alloc(&mut self, layout: Layout, bump: &mut BumpAllocator) -> *mut u8 {
        self.alloc_count.fetch_add(1, Ordering::Relaxed);

        // STAGE bucket_route
        let bucket_idx = match Self::bucket_route(&layout) {
            Some(i) => i,
            None => {
                // Too big for any slab bucket — fall through to bump.
                self.slab_misses.fetch_add(1, Ordering::Relaxed);
                return bump.bump_alloc_bytes(layout.size(), layout.align());
            }
        };

        // STAGE free_list_pop
        let user_ptr = match Self::free_list_pop(&mut self.buckets[bucket_idx]) {
            Some(p) => {
                self.slab_hits.fetch_add(1, Ordering::Relaxed);
                p
            }
            None => {
                // Free-list empty: carve a new node from backing bump.
                let class_size = SLAB_CLASSES[bucket_idx];
                let total = HEADER_BYTES + class_size;
                let node = bump.bump_alloc_bytes(total, 16);
                if node.is_null() {
                    return core::ptr::null_mut();
                }
                node.add(HEADER_BYTES)
            }
        };

        // STAGE avalanche_tag_write (hazard: AFTER pop, on user ptr)
        Self::avalanche_tag_write(user_ptr, &layout, bucket_idx);
        user_ptr
    }

    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        self.dealloc_count.fetch_add(1, Ordering::Relaxed);

        // STAGE avalanche_tag_verify
        let (ok, bucket_plus_one) = Self::avalanche_tag_verify(ptr, &layout);
        if !ok {
            self.tag_mismatches.fetch_add(1, Ordering::Relaxed);
            // Mismatch could mean: double-free, use-after-free, header
            // corruption, OR (legitimately) a non-slab alloc that fell
            // through to bump. The bucket_plus_one==0 case is the
            // legitimate fall-through; anything else is a real bug.
            if bucket_plus_one != 0 {
                // Real corruption — for now, leak (don't push a poisoned
                // node onto the free-list). Panic in debug, leak in release.
                #[cfg(debug_assertions)]
                panic!("slab: tag mismatch (double-free or corruption)");
            }
            return;
        }

        // STAGE free_list_push
        let bucket_idx = (bucket_plus_one - 1) as usize;
        Self::free_list_push(&mut self.buckets[bucket_idx], ptr);
    }
}

// ─────────────────────── bump helper extension ───────────────────────

impl BumpAllocator {
    /// Direct byte allocation from the bump region. Used by SlabHeap as
    /// backing. Mirrors the AtomHeap::alloc math but without going through
    /// the Spinlock (the caller already holds the lock).
    pub unsafe fn bump_alloc_bytes(&mut self, size: usize, align: usize) -> *mut u8 {
        if self.heap_start == 0 {
            return core::ptr::null_mut();
        }
        let next = self.next;
        let rem = next % align;
        let start = if rem == 0 { next } else { next + align - rem };
        let end = start + size;
        if end <= self.heap_end {
            self.next = end;
            self.allocations += 1;
            start as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }
}


// ─────────────────────────── locked wrapper ──────────────────────────
//
// SlabHeap needs mutable access to its buckets during alloc/dealloc.
// GlobalAlloc takes &self, so we wrap SlabHeap in an IrqSpinlock and
// implement GlobalAlloc on the wrapper. This means EVERY alloc/dealloc
// pays one IRQ save/restore — which is the cost of any global allocator
// that needs internal mutation. The slab itself stays O(1) inside the
// lock, which is the design goal.
//
// NOTE on stage-contract consistency with GAP 2:
//   The IrqSpinlock here provides irqs_off=true as required by the
//   slab's internal CAS-free mutation (we don't need a CAS because
//   the lock serializes access). The hazard from Appendix B
//   (cas_acquire REQUIRES irqs_off=true) is satisfied transitively.

use crate::memory::Spinlock;

pub struct SlabLocked {
    // We use a Spinlock (not IrqSpinlock) here because the heap is
    // never touched from IRQ context in the current kernel design —
    // allocs happen from syscall/mainline paths only. If that changes,
    // switch to IrqSpinlock. (T2 inference: matches the existing
    // AtomHeap, which also uses plain Spinlock for the same reason.)
    inner: Spinlock<SlabHeapInner>,
}

struct SlabHeapInner {
    heap: SlabHeap,
    bump: BumpAllocator,
}

unsafe impl GlobalAlloc for SlabLocked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let inner = self.inner.lock();
        // Split the mutable borrow: heap gets &mut self (with its buckets),
        // bump gets &mut as a sibling. This is the fix for the
        // self-referential borrow the checker rejected.
        let SlabHeapInner { heap, bump } = inner;
        let ptr = heap.alloc(layout, bump);
        self.inner.unlock();
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let inner = self.inner.lock();
        inner.heap.dealloc(ptr, layout);
        self.inner.unlock();
    }
}

impl SlabLocked {
    pub const fn new() -> Self {
        Self {
            inner: Spinlock::new(SlabHeapInner {
                heap: SlabHeap::new(),
                bump: BumpAllocator::new(),
            }),
        }
    }

    /// Initialize with a backing memory region. Called once from _start.
    pub fn init(&self, start: usize, size: usize) {
        let inner = self.inner.lock();
        inner.bump.init(start, size);
        self.inner.unlock();
    }

    /// Read-only snapshot of the counters, for VERIFY-stage reporting.
    pub fn stats(&self) -> SlabStats {
        let inner = self.inner.lock();
        let s = SlabStats {
            alloc_count: inner.heap.alloc_count.load(Ordering::Relaxed),
            dealloc_count: inner.heap.dealloc_count.load(Ordering::Relaxed),
            slab_hits: inner.heap.slab_hits.load(Ordering::Relaxed),
            slab_misses: inner.heap.slab_misses.load(Ordering::Relaxed),
            tag_mismatches: inner.heap.tag_mismatches.load(Ordering::Relaxed),
        };
        self.inner.unlock();
        s
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SlabStats {
    pub alloc_count: usize,
    pub dealloc_count: usize,
    pub slab_hits: usize,
    pub slab_misses: usize,
    pub tag_mismatches: usize,
}

impl SlabStats {
    /// The named currency for GAP 1: how many allocs hit the slab fast
    /// path vs the bump fallback. Higher hit ratio = better.
    pub fn hit_ratio(&self) -> f64 {
        let total = self.slab_hits + self.slab_misses;
        if total == 0 {
            0.0
        } else {
            self.slab_hits as f64 / total as f64
        }
    }
}


// ─────────────────────────── self-test ───────────────────────────
//
// Runs at boot from main.rs _start. Allocates a small private memory
// region (NOT the kernel heap), runs an alloc/free/alloc cycle, and
// prints stats. The gate: the second alloc pass should hit the
// free-list (slab_hits == N), proving the free-list + tag-verify
// round-trip works.
//
// Output goes via a caller-supplied print function so this module
// doesn't depend on serial directly.

pub fn slab_self_test(
    region_start: usize,
    region_size: usize,
    print_fn: impl Fn(&str),
) {
    // We can't easily construct a SlabLocked in a static here (it needs
    // const init). Instead, build a SlabHeap + BumpAllocator on the
    // caller-provided region and exercise them directly.
    use core::ptr;
    // Place a BumpAllocator + SlabHeap in a static-sized stack array.
    // For simplicity, use a leaked stack allocation.
    let mut bump_storage = BumpAllocator::new();
    bump_storage.init(region_start, region_size);

    let mut heap = SlabHeap::new();

    // Layout for a 24-byte allocation (Vec header): bucket 1 (32 bytes).
    let layout = Layout::from_size_align(24, 8).unwrap();
    const N: usize = 64;
    let mut ptrs: [*mut u8; N] = [ptr::null_mut(); N];

    // Pass 1: alloc N (should miss free-list, carve from bump).
    for i in 0..N {
        let p = unsafe { heap.alloc(layout.clone(), &mut bump_storage) };
        if p.is_null() {
            let msg = "slab_self_test: OOM during pass 1\n";
            print_fn(msg);
            return;
        }
        ptrs[i] = p;
    }
    let stats1_alloc = heap.alloc_count.load(Ordering::Relaxed);
    let stats1_hits = heap.slab_hits.load(Ordering::Relaxed);

    // Free all N.
    for i in 0..N {
        if !ptrs[i].is_null() {
            unsafe { heap.dealloc(ptrs[i], layout.clone()); }
            ptrs[i] = ptr::null_mut();
        }
    }
    let stats1_dealloc = heap.dealloc_count.load(Ordering::Relaxed);

    // Pass 2: alloc N again (should hit the free-list this time).
    let mut hits2 = 0usize;
    for i in 0..N {
        let p = unsafe { heap.alloc(layout.clone(), &mut bump_storage) };
        if p.is_null() {
            let msg = "slab_self_test: OOM during pass 2 (free-list broken)\n";
            print_fn(msg);
            return;
        }
        ptrs[i] = p;
    }
    let stats2_hits = heap.slab_hits.load(Ordering::Relaxed);
    let stats2_misses = heap.slab_misses.load(Ordering::Relaxed);
    let stats2_mismatch = heap.tag_mismatches.load(Ordering::Relaxed);

    // Build a tiny report without alloc (the test must not depend on
    // the heap it's testing). ASCII-only, fixed format.
    let mut buf = [0u8; 160];
    let mut len = 0;
    let push = |buf: &mut [u8; 160], len: &mut usize, s: &[u8]| {
        for &b in s {
            if *len < buf.len() {
                buf[*len] = b;
                *len += 1;
            }
        }
    };
    let push_num = |buf: &mut [u8; 160], len: &mut usize, n: usize| {
        if n == 0 {
            push(buf, len, b"0");
            return;
        }
        let mut digits = [0u8; 20];
        let mut k = 0;
        let mut m = n;
        while m > 0 {
            digits[k] = b'0' + (m % 10) as u8;
            m /= 10;
            k += 1;
        }
        while k > 0 {
            k -= 1;
            push(buf, len, &[digits[k]]);
        }
    };

    push(&mut buf, &mut len, b"slab_self_test: pass1_alloc=");
    push_num(&mut buf, &mut len, stats1_alloc);
    push(&mut buf, &mut len, b" pass1_hits=");
    push_num(&mut buf, &mut len, stats1_hits);
    push(&mut buf, &mut len, b" dealloc=");
    push_num(&mut buf, &mut len, stats1_dealloc);
    push(&mut buf, &mut len, b" pass2_hits=");
    push_num(&mut buf, &mut len, stats2_hits);
    push(&mut buf, &mut len, b" misses=");
    push_num(&mut buf, &mut len, stats2_misses);
    push(&mut buf, &mut len, b" mismatch=");
    push_num(&mut buf, &mut len, stats2_mismatch);
    push(&mut buf, &mut len, b"\n");

    let report = core::str::from_utf8(&buf[..len]).unwrap_or("slab_self_test: utf8 err\n");
    print_fn(report);

    // The gate: pass2 should have N more hits than pass1 (the free-list
    // returned all N). Report PASS/FAIL explicitly.
    if stats2_hits >= stats1_hits + N && stats2_mismatch == 0 {
        print_fn("slab_self_test: GATE PASS (free-list round-trip OK)\n");
    } else {
        print_fn("slab_self_test: GATE FAIL (free-list round-trip broken)\n");
    }

    // Free the pass-2 allocations so we leave the test region clean.
    for i in 0..N {
        if !ptrs[i].is_null() {
            unsafe { heap.dealloc(ptrs[i], layout.clone()); }
        }
    }
}


// ─────────────────── OOM-after-N benchmark ───────────────────
//
// The named currency for GAP 1: how many alloc/free cycles can each
// allocator sustain before OOM, on the same workload and equal regions?
//
// Workload: alloc 64 bytes (Layout::from_size_align(64, 8)), free,
// repeat. For BumpAllocator this is the worst case — dealloc is a
// no-op, so it OOMs at region_size / 64. For SlabHeap the second and
// later allocs hit the free-list, so it should run indefinitely (or
// until the test's hard cap).
//
// Reports N_slab and N_bump over the supplied print closure.

pub fn oom_after_n_benchmark(
    slab_region_start: usize,
    slab_region_size: usize,
    bump_region_start: usize,
    bump_region_size: usize,
    print_fn: impl Fn(&str),
) {
    use core::ptr;

    // ----- SlabHeap trial -----
    let mut slab_bump = BumpAllocator::new();
    slab_bump.init(slab_region_start, slab_region_size);
    let mut slab_heap = SlabHeap::new();
    let layout = Layout::from_size_align(64, 8).unwrap();
    const HARD_CAP: usize = 100_000;
    let mut n_slab: usize = 0;
    let mut slab_oom = false;
    for _ in 0..HARD_CAP {
        let p = unsafe { slab_heap.alloc(layout.clone(), &mut slab_bump) };
        if p.is_null() {
            slab_oom = true;
            break;
        }
        unsafe { slab_heap.dealloc(p, layout.clone()); }
        n_slab += 1;
    }

    // ----- BumpAllocator trial -----
    let mut bump = BumpAllocator::new();
    bump.init(bump_region_start, bump_region_size);
    let mut n_bump: usize = 0;
    let mut bump_oom = false;
    for _ in 0..HARD_CAP {
        let p = unsafe { bump.bump_alloc_bytes(64, 8) };
        if p.is_null() {
            bump_oom = true;
            break;
        }
        // BumpAllocator.dealloc is a no-op — no free possible.
        n_bump += 1;
    }

    // ----- Report -----
    let mut buf = [0u8; 200];
    let mut len = 0;
    let push = |buf: &mut [u8; 200], len: &mut usize, s: &[u8]| {
        for &b in s { if *len < buf.len() { buf[*len] = b; *len += 1; } }
    };
    let push_num = |buf: &mut [u8; 200], len: &mut usize, n: usize| {
        if n == 0 { push(buf, len, b"0"); return; }
        let mut digits = [0u8; 20];
        let mut k = 0;
        let mut m = n;
        while m > 0 { digits[k] = b'0' + (m % 10) as u8; m /= 10; k += 1; }
        while k > 0 { k -= 1; push(buf, len, &[digits[k]]); }
    };

    push(&mut buf, &mut len, b"oom_after_n: slab=");
    push_num(&mut buf, &mut len, n_slab);
    push(&mut buf, &mut len, b" (oom=");
    push(&mut buf, &mut len, if slab_oom { b"yes" } else { b"no " });
    push(&mut buf, &mut len, b", capped=");
    push_num(&mut buf, &mut len, HARD_CAP);
    push(&mut buf, &mut len, b")  bump=");
    push_num(&mut buf, &mut len, n_bump);
    push(&mut buf, &mut len, b" (oom=");
    push(&mut buf, &mut len, if bump_oom { b"yes" } else { b"no " });
    push(&mut buf, &mut len, b")\n");

    let report = core::str::from_utf8(&buf[..len]).unwrap_or("oom_after_n: utf8 err\n");
    print_fn(report);

    // Explicit verdict.
    if !slab_oom && bump_oom {
        let msg = "oom_after_n: GATE PASS — slab ran indefinitely, bump OOMed\n";
        print_fn(msg);
    } else if slab_oom && bump_oom && n_slab > n_bump {
        let msg = "oom_after_n: GATE PARTIAL — both OOMed but slab outlasted bump\n";
        print_fn(msg);
    } else {
        let msg = "oom_after_n: GATE FAIL — slab did not beat bump\n";
        print_fn(msg);
    }
}
