# atom-os-kernel — Working Notes (NAT review → fix session)

## Verification ceiling (established 2026-07-15)
- ✅ `nightly-x86_64-pc-windows-msvc` installed; `bootimage 0.10.4` installed → can build bootimage.
- ❌ **QEMU NOT installed** → cannot boot the kernel. Verification stops at "compiles + bootimage produced".
- Default toolchain is `1.96.0` (stable) → bare `cargo check` fails on `#![feature(...)]`.
- CI (`.github/workflows/test.yml`) gates on QEMU grep `"10,000 SYS_YIELDs took"`.

## Runtime crash confirmed from run.log (CI output, 2026-07-15T16:38)
- `check_exception old: 0x8 new: 0xe` → **Triple fault** → CPU Reset.
  - 0x8 = #DF (Double Fault), 0xe = #PF (Page Fault). Page fault → no handler → DF → no DF handler → triple.
  - **This is C7 (no exception handlers) firing at runtime.**
- Crash regs: RIP=`0x208abe` (heap addr), RSP=`0x10001ff1f0` (in 1MB HEAP_MEM @ 0x10000000),
  RBP=`0x1000202000`. RIP inside the heap = jumped into page-table memory treated as code
  → consistent with C2 (virtual ptr stored as physical PML4 entry).
- CI result: `Benchmark failed or not found!` exit 1. **CI is red.**
- Secondary CI failure: `fatal: No url found for submodule path 'atom-3d-engine' in .gitmodules`.

## Verified defects (file:line, runtime-confirmed where noted)
- **BUILD-BREAK-1**: `runner/src/main.rs` stale — `Context::new(100,0)` (2-arg), `.r0/.r1`, `sys.run()`,
  `sys.scheduler.next_task()`, `ctx.page_table.translate()` — none exist. `cargo check --workspace` fails.
- **BUILD-BREAK-2**: `x86_64-kernel` needs nightly (`#![feature(alloc_error_handler)]`, main.rs:3).
  Bare `cargo check` on stable fails E0554.
- **C1**: keyboard vec 33 registered to bare Rust fn, no asm wrapper/iretq (main.rs:330 vs 329/333).
- **C2**: `duplicate_pml4` stores heap (virtual) ptr as physical addr (paging.rs:104,107-111,145,158,171,178).
  Runtime-confirmed (RIP in heap at crash).
- **C3**: bump `dealloc` collapses whole heap when last alloc freed (memory.rs:92-97).
- **C4**: `Spinlock` no IRQ masking, used by allocator + from IRQ ctx (memory.rs:23-32).
- **C5**: syscall ptr args deref'd unvalidated (syscall.rs:76-79,175-182,295-298).
- **C6**: `int 0x80` gets spurious EOI to PIC2 (main.rs:196, pic.rs:55-61).
- **C7**: no exception handlers vec 0-31 (interrupts.rs:17-27). **Runtime-confirmed triple fault.**
- **C8**: `VgaWriter::new()` per SYS_WRITE, cursor resets (syscall.rs:66,139,159).
- **H4**: `duplicate_pml4` clears PML4[256..511] but comment/code disagree on user/kernel split
  (paging.rs:113-117); user stack @ 0xFFFFFFFF800FE000 is upper-half = just cleared.

## Key API facts (verified by reading)
- `Context::new(id: usize, rsp: u64, kernel_stack: u64, page_table_root: u64)` — 4 args (context.rs:22).
- `Context` fields: `rsp, kernel_stack, state, id, page_table_root, open_files` — NO r0/r1/pc.
- `Scheduler`: `spawn`, `current_task`, `current_task_mut`, `switch_context` — NO `next_task`, NO `run`.
- `System`: `new`, `schedule_tick` — NO `run`.
- TrapFrame (trap.rs): 15 GPRs (rax..r15) + rip/cs/rflags/rsp/ss. `new_user(rip, rsp)`.
- IDT entry selector hardcoded 0x08 (interrupts.rs:38); type_attr = 0x8E | (dpl<<5).
- GDT order: null(0), kcode(1=0x08), kdata(2=0x10), ucode(3=0x18→0x1B RPL3), udata(4=0x20→0x23), tss(5=0x28).
- Boot order in _start: CR0/CR4 → serial → VGA → heap → inject_payloads → IDT.load → PIC.init →
  System::new → GDT.load → spawn_process×2 → sti → hlt loop.
  NOTE: spawn_process calls dispatch→duplicate_pml4 BEFORE sti, so C2 triggers during boot setup.

## Phase results (2026-07-15)

### Phase 1 (build) — ✅ FIXED + VERIFIED
- `Cargo.toml`: removed `runner` from members, added `exclude = ["runner"]` (files kept on disk).
- `rust-toolchain.toml` created: nightly + rust-src + llvm-tools-preview.
- `.cargo/config.toml` created: target=x86_64-os.json, build-std, json-target-spec.
- Gate: `cargo +nightly check --workspace` → Finished (only pre-existing static_mut warnings).

### Phase 2 (interrupts) — ✅ FIXED + COMPILES + QEMU behavior changed
- C1: keyboard vec 33 now registered to `keyboard_interrupt_wrapper` (asm stub with iretq),
  not the bare Rust fn. `keyboard_interrupt_handler` signature → `(rsp)->rsp`.
- C6: removed spurious `PICS.notify_end_of_interrupt(0x80)` (int 0x80 isn't PIC-sourced).
- C7: added `exception_common` asm + per-vector `exception_entry_N` trampolines (NEC pushes
  synth errcode, EC skips it) + Rust `exception_handler` that prints vector and halts.
  Registered all 32 CPU exceptions in the IDT.
- H1: `ChainedPics` stores offset1/offset2; `initialize` masks all IRQs; new `unmask(irq)`
  method; `_start` unmasks IRQ0 (timer) + IRQ1 (kbd) explicitly.
- QEMU before fix: silent triple-fault, "check_exception old 0x8 new 0xe" (the run.log crash).
- QEMU after fix: boot completes to "System running autonomously" (idle loop). Faults now
  happen LATER (during the first task switch), confirming the interrupt path itself works.

### Phase 3 (paging) — ⚠️ PARTIALLY FIXED — root cause found, deeper fix needed
- C2: `duplicate_pml4` and `map_segment` now use `FRAME_ALLOCATOR` (tracked frame pool)
  instead of the kernel heap. `paging.rs:99,142,155,168`.
- New `FrameAllocator` in `memory.rs` (1024-frame bitmap pool, 4 MiB).
- `HEAP_MEM` enlarged 1 MiB → 16 MiB; front ~12 MiB = kernel heap, back 4 MiB = frame pool.
- **Sub-bug found via QEMU diagnostic**: `HEAP_MEM` is a `[u8; 16MiB]` with align 1, so its
  link address (0x20fdb0 observed) is NOT page-aligned. The frame pool base inherited that
  misalignment → CR3 was loaded with unaligned PML4 addresses → undefined MMU walk → triple
  fault. FIXED by rounding frame_base UP to 4 KiB and shrinking heap to match.
  After this fix, serial confirms `page_table_root = 0xe14000` (page-aligned). The earlier
  unaligned value (0xe15b70) is gone.
- **REMAINING BLOCKER (the actual crash now)**: after the timer ISR loads a user task's CR3
  (0xe14000), the next instruction fetch page-faults and triple-faults before the exception
  handler can print. Cause: `duplicate_pml4` does a SHALLOW copy of the kernel PML4's 512
  entries, but the kernel code/stack/IDT/handler addresses do NOT resolve in the new address
  space. The bootloader (0.9.23) sets up the kernel mapping in the lower half; the shallow
  copy SHOULD preserve it (entries point at the shared kernel PDPT/PD/PT pages), but
  empirically the first kernel-side instruction fetch in the user CR3 faults.
  → This needs the **higher-half kernel / shared-kernel-PML4 technique**: map the kernel
  identically into EVERY address space (typically via a recursive PML4 entry or by copying
  the kernel PDPT entries into every new PML4), so the kernel remains addressable after any
  CR3 switch. This is the standard x86-64 kernel design and is the correct next step.

### Phase 4 (smaller fixes) — ✅ FIXED + COMPILES
- C3: `BumpAllocator::dealloc` is now a no-op (pure bump) — stops the heap-collapse UAF.
- C8: `VgaWriter` now uses a global `static CURSOR: Spinlock<(usize,usize)>`; `new()` loads
  it, every mutation saves it back. SYS_WRITE no longer resets to (0,0).

## Out of scope this session (flagged for next pass)
- REMAINING BLOCKER above (higher-half kernel mapping) — the paging isolation fix.
- C4: spinlock IRQ masking (needs IRQ-aware lock type across 4 statics).
- C5: syscall pointer validation (user/kernel range check).
- Hygiene: untrack run*.log, fix atom-3d-engine gitlink (no .gitmodules), unify editions.

## Update 2 — payload linker + remaining fault signature (2026-07-15)

### Payload linker bug — FIXED
- ROOT CAUSE of "user task jumps to 0x201190": the payload was being linked at the DEFAULT
  0x200000 (same range as the kernel!) instead of 0xFFFFFFFF80100000, because
  `payload/.cargo/config.toml` rustflags only apply when cargo runs INSIDE `payload/`.
  `cargo build -p payload` from the workspace root ignored `-Tpayload/linker.ld`.
- CI avoided this by `working-directory: ./payload`, but local/workspace builds were broken.
- FIX: added `payload/build.rs` and `daemon/build.rs` that emit
  `cargo:rustc-link-arg=-T$CARGO_MANIFEST_DIR/linker.ld` — applies in all invocation styles.
- VERIFIED: `e_entry` now reads `0xFFFFFFFF80100010` for both payload and daemon whether
  built from root or from inside the crate. (Was `0x201370` before.)

### Empirical finding: CR3 switch is the fault trigger
- TEST: disabled the `Cr3::load` in timer_interrupt_handler. Result: kernel STABLE (1 boot,
  no reboot loop), exception handler WORKS ("!! CPU EXCEPTION 0E (#PF) -- HALTING" printed,
  then clean halt). Proves all interrupt/PIC/frame/exception fixes are sound.
- With that one `Cr3::load` re-enabled: reboot loop returns.

### CRITICAL FINDING (2026-07-15) — Cr3::load works; user-CR3 content is the issue
- DECISIVE TEST: calling `Cr3::load(0x1000)` (reload the kernel's OWN CR3, a no-op) WORKS —
  `Cr3::read()` survives, prints `0x1000`, boot continues. So the `mov cr3` asm is correct.
- NUCLEAR TEST: made `duplicate_pml4` return the kernel's own CR3 (0x1000) for all tasks
  (no new frame allocated). Result: kernel STABLE, no reboot loop, timer fires, handler
  runs, exception handler catches a #PF and halts cleanly. Proves the ENTIRE
  interrupt/timer/switch/exception path is correct. The ONLY thing that breaks it is
  loading a freshly-allocated PML4 frame.
- Calling `Cr3::load(0xe16000)` (the user PML4 built by duplicate_pml4) FAULTS on the very
  next instruction fetch, regardless of page-table content tried:
  * Shallow copy of bootloader PML4 (cleared upper half): fault.
  * Fresh PML4 + 1-GiB huge page for low half: fault. PML4[0]=0xe13003, PDPT[0]=0x83 verified.
  * Fresh PML4 + 2-MiB huge pages (PD-level, 16 entries): fault.
  * Deep copy of bootloader's PML4[0]→PDPT[0]→PD[0..512] chain into fresh frames: fault.
- Frame addresses verified distinct & non-aliased (0xe12000/0xe13000/0xe14000 for shell,
  0xe18000/0xe19000/0xe1a000 for daemon). Frame pool base ~0xe0f000, no heap overlap.
- This means: NO page-table content placed in a user CR3 allows kernel .text fetch after
  mov cr3, including a verbatim reproduction of the working kernel mapping. The bootloader's
  own PML4 (0x1000) works; an equivalent reconstructed one does not.

### FINAL PARADOX (verified 2026-07-15) — the smoking gun
Three facts, all empirically verified, that are mutually contradictory under standard
x86-64 paging semantics:
  1. `Cr3::load(0x1000)` (bootloader's PML4) WORKS — kernel runs, timer fires, stable.
  2. `Cr3::load(0xe16000)` (a fresh frame whose content is a BYTE-IDENTICAL copy of 0x1000,
     verified by reading PML4[0]=0x2023 at the instant before mov cr3) FAULTS on the next
     instruction fetch.
  3. The frame at 0xe16000 IS real RAM: wrote 0xDEADBEEF12345678, read it back perfectly.
     Frame addresses verified distinct & non-aliased. Pool base ~0xe0f000, in QEMU's 128 MiB.
- The ONLY difference between the working and failing case is the physical address CR3 holds
  (0x1000 vs 0xe16000). Under x86-64, CR3 is just a physical pointer to the PML4 — the MMU
  walks whatever it points to, regardless of address. This should be impossible.
- UNRESOLVED HYPOTHESES (not yet distinguished):
  (a) The bootloader (0.9.23) set up PCID / CR4.PCIDE / a context identifier that ties the
      working walk to CR3=0x1000, and a different CR3 value defeats it. (EFER dump didn't
      show PCIDE, but CR4 bits worth re-checking explicitly.)
  (b) The bootloader's PML4 at 0x1000 has more than just the low-half entries — e.g. a
      recursive-map entry (PML4[some_index] pointing back at 0x1000) that the walk needs
      to resolve kernel addresses via the recursive path. Copying bytes to 0xe16000 would
      copy the recursive entry's *value* (pointing at 0x1000) but the recursive lookup
      from 0xe16000 wouldn't self-reference correctly. THIS IS THE LEADING HYPOTHESIS.
  (c) A subtle CPU feature (SMAP/SMEP/PKE) keyed off CR3.

### RECOMMENDED NEXT STEP
Verify hypothesis (b): dump ALL 512 entries of the bootloader's PML4 (0x1000) and look for
a self-referential entry (an entry whose address bits == 0x1000). If found, the bootloader
uses recursive paging and `duplicate_pml4` must (1) preserve that entry pointing at the
*original* 0x1000 (not the copy), or (2) the kernel must adopt its own recursive index.
Alternatively, upgrade to `bootloader 0.10`/`bootloader-api` which exposes the memory map
and a cleaner page-table setup, removing the dependency on the bootloader's PT structure.

### UPDATE — hypothesis (b) DISPROVEN (2026-07-15)
Scanned all 512 bootloader PML4 entries. Exactly 3 are Present:
  [000]=0x2023 (PDPT@0x2000, kernel low image)
  [002]=0x411063 (PDPT@0x411000, ~4GiB region: HEAP_MEM + kernel stack at RSP=0x100001ff1f0)
  [01f]=0x4115063 (PDPT@0x4115000, index 31)
NONE is self-referential (no entry points at 0x1000). So the bootloader does NOT use
recursive paging. copy_nonoverlapping of all 512 entries reproduces these exactly, yet
loading the copy faults.

### REMAINING UNRESOLVED HYPOTHESES (in priority order)
  1. PCID: CR4.PCIDE (bit 17) may be enabled, binding the walk to CR3=0x1000's PCID.
     MOV CR3 to a new physical addr with the same PCID bits may invalidate incorrectly.
     CHECK: read CR4, inspect bit 17; if set, clear it or set PCID=0 in the loaded CR3.
  2. The bootloader-0.9 default config uses a recursive index that we haven't found
     because the recursive entry may be at an index whose address bits encode 0x1000
     with a high flag we filtered. Re-scan WITHOUT the Present-bit filter.
  3. QEMU 11.0.50 quirk / the bootimage tool's loader sets up a context we're defeating.
  4. Upgrade to `bootloader 0.10`/`bootloader-api` — exposes memory map + clean PT setup,
     removing the dependency on reverse-engineering the bootloader's PT structure. This is
     the most reliable path forward.

### CURRENT CODE STATE (end of session)
- `duplicate_pml4` does `copy_nonoverlapping` of all 512 entries (clean, no diagnostics).
- `Cr3::load` restored to plain `mov cr3` asm (no diagnostics).
- All temp diagnostics removed. `cargo +nightly check --workspace` green; bootimage builds.
- Kernel boots to idle loop; with the CR3 switch active it reboots (the unresolved paradox).
  With duplicate_pml4 returning the kernel CR3 (the nuclear test) it runs stably.

### UPDATE — hypothesis 1 (PCID) DISPROVEN too (2026-07-15)
- CR4 = 0x620 (PAE | OSFXSR | OSXMMEXCPT). PCIDE (bit 17) = 0. SMEP/SMAP/PKE all 0.
- ALL testable hypotheses now exhausted: content (byte-identical copy), RAM (genuine,
  pattern read back), frame address (front & back pools both fail), recursive paging
  (no self-referential entry), PCID (off), Cr3::load asm (works with 0x1000).
- The paradox stands: loading CR3=0x1000 works; loading CR3=<any fresh frame with
  byte-identical PML4 content> faults on the next instruction fetch. Under standard
  x86-64 semantics this is impossible; it implies an interaction with the bootloader-0.9
  context that isn't visible from inside the running kernel.

### BLOCKER (genuine, requires operator decision or external info)
Cannot make a freshly-allocated PML4 loadable as CR3, despite identical content to the
working bootloader PML4. Options to unblock:
  A. Upgrade `bootloader` 0.9.23 → 0.10 / `bootloader-api`. This exposes the boot
     memory map and uses a documented page-table setup, removing the reverse-engineering.
     (Most reliable; some API migration.)
  B. Read bootloader-0.9.23 source to understand exactly what its PML4 setup does
     that a plain copy doesn't capture (e.g. a recursive map entry we filtered, or a
     frame that must remain at its original physical address).
  C. Build the ENTIRE kernel page-table hierarchy from scratch (own PML4+PDPT+PD+PT,
     not copying bootloader structures) and switch to it before spawning tasks — so the
     kernel never depends on the bootloader's PT pages at all. This is the "higher-half
     kernel from boot" approach and is substantial.
  `v=0e e=0010 cpl=0 RIP=CR2=0x209b0e CR3=0x1000 RCX=0xe16000(new CR3)`.
- Error code 0x10 = instruction-fetch violation. NXE is enabled (EFER bit 11 set, EFER=0xd00).
- Kernel PML4 is NOT corrupted: dumped PML4[0..4] = [0x2023, 0, 0x411063, 0] — entry 0 is
  Present/Writable/User pointing at PDPT 0x2000. The kernel low-half mapping is intact.
- The fault is on instruction fetch of a kernel .text page that is present in the walk.
  Candidate mechanisms (not yet distinguished):
  (a) The leaf PTE for the 0x209xxx page has the NX bit (bit 63) set by the bootloader, and
      something about the second timer-tick path crosses into that page. (Unlikely: the
      handler ran the first time.)
  (b) TLB coherency: `Cr3::load` on the PREVIOUS tick's switch_context left a stale TLB,
      and the next fetch sees a cached not-present entry. (mov cr3 invalidates the TLB by
      default unless PCID is used; EFER doesn't show PCID enabled.)
  (c) The handler's kernel stack (`RSP=0x100001ff1f0`, in HEAP_MEM) page-faults first and
      the reported RIP is where the push went — but error code says instruction-fetch, not
      data, so this is less likely.
- NEXT diagnostic step: dump the leaf PTE (PML4[0]→PDPT→PD→PT) for address 0x209b0e to read
  its flags directly, and check whether NX (bit 63) is set. That will distinguish (a) from
  a genuine not-present walk failure. This requires a PTE-walk helper in paging.rs and is
  the clean next step but wasn't completed in this session.

### Files changed this session (final state)
- `Cargo.toml` (dropped runner from members, added exclude)
- `rust-toolchain.toml` (NEW: nightly + rust-src + llvm-tools-preview)
- `.cargo/config.toml` (NEW: target, build-std, json-target-spec)
- `payload/build.rs`, `daemon/build.rs` (NEW: emit linker-script link-arg)
- `x86_64-kernel/src/main.rs` (C1 keyboard wrapper, C6 EOI removal, C7 exception handlers
  + all 32 IDT entries, H1 unmask IRQ0/1, C2 frame-allocator init + page-aligned frame base,
  HEAP_MEM 1MiB→16MiB)
- `kernel-kit/src/pic.rs` (H1: store offsets, mask-all-at-init, unmask())
- `kernel-kit/src/memory.rs` (C2 FrameAllocator, C3 dealloc no-op)
- `kernel-kit/src/paging.rs` (C2 duplicate_pml4/map_segment use FRAME_ALLOCATOR)
- `kernel-kit/src/vga.rs` (C8 persistent cursor)
- `NOTES.md` (this file)


---

# RESOLUTION (2026-07-17 session)

## The "final paradox" is solved

The prior session documented a paradox: `Cr3::load(0x1000)` (bootloader's PML4)
worked, but `Cr3::load(<any fresh frame>)` faulted on the next instruction
fetch, even when the fresh frame held byte-identical content. Every testable
hypothesis was disproven.

## Root cause (verified via QEMU diagnostics)

**Two compounding bugs**, both stemming from the kernel throwing away the
bootloader's `BootInfo`:

### Bug 1: `_start()` ignored BootInfo; no physical-memory mapping

- `x86_64-kernel/Cargo.toml` depended on `bootloader = "0.9.23"` with **no features**.
- `_start()` signature was `pub extern "C" fn _start() -> !` — **no BootInfo argument**,
  so the `rdi` the bootloader passed was discarded.
- Without the `map_physical_memory` feature, the bootloader does NOT map all
  physical memory at an offset, and `BootInfo` has no `physical_memory_offset` field.
- Direct identity mapping (treating physical addresses as virtual) is **wrong**:
  reading VA `0x1000` (where CR3 points) immediately triple-faults.

### Bug 2: `duplicate_pml4` treated physical addresses as virtual pointers

```rust
let src_pml4 = active_cr3 as *const PageTable;          // WRONG
let new_pml4 = &mut *(new_pml4_phys as *mut PageTable); // WRONG
core::ptr::copy_nonoverlapping(src_pml4, new_pml4, 1);
```

Without a phys→virt translation, the copy read from / wrote to wrong memory.

## Why the "byte-identical copy faults" paradox held

The prior session's `duplicate_pml4` *appeared* to copy correctly because the
FRAME_ALLOCATOR's pool (0xE14000) sat in a region that was coincidentally
directly accessible (the bootloader identity-maps the kernel image's load
range). So writes "worked" in the sense of not faulting. But they wrote to
**virtual** address 0xE14000, which the bootloader's page tables mapped to
some physical address X. When the copy was loaded as CR3, the MMU walked
**physical** 0xE14000 — a different location — and found garbage. The
"byte-identical" verification read back via the same wrong virtual address,
so it looked correct while being physically wrong.

## The fix (4 changes, all verified)

### 1. Enable `map_physical_memory` on the bootloader dep
```toml
# x86_64-kernel/Cargo.toml
bootloader = { version = "0.9.23", features = ["map_physical_memory"] }
```
This makes the bootloader map ALL physical memory at `physical_memory_offset`
(passed in BootInfo) and adds the field to the BootInfo struct.

### 2. Accept BootInfo in `_start`, store the offset
```rust
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    kernel_kit::paging::set_phys_offset(boot_info.physical_memory_offset);
    // ...
```
Verified: `physical_memory_offset = 0x0000018000000000` (1.5 TiB virtual).

### 3. Add `phys_to_virt` / `virt_to_phys` translators in paging.rs
```rust
pub static PHYS_OFFSET: AtomicU64 = AtomicU64::new(0);
pub fn phys_to_virt(phys: u64) -> u64 { phys + PHYS_OFFSET.load(Ordering::SeqCst) }
pub fn virt_to_phys(virt: u64) -> u64 { virt - PHYS_OFFSET.load(Ordering::SeqCst) }
```
Fixed `duplicate_pml4`, `map_segment` to translate every physical-address
dereference via `phys_to_virt`.

### 4. Rebuild FrameAllocator on the bootloader's USABLE region

The original FrameAllocator carved a pool out of a static `[u8; 16 MiB]`
(`HEAP_MEM`) that overlapped the kernel image — physical memory the
bootloader had NOT marked Usable and which was chaotic for page-table frames.

The new `FrameAllocator.init(base_phys, num_frames)` takes a range, and
`_start` scans `boot_info.memory_map` for the largest `Usable` region.
On QEMU default 128 MiB, this is **frames 0x16A0–0x7FE0** (~99 MiB at
physical `0x16A0000`–`0x7FE0000`), which IS covered by the offset mapping
(verified by write+read aliasing test).

### 5. Back SYS_EXEC payload segments + user stack with FRAME_ALLOCATOR frames

The SYS_EXEC path allocated payload backing via `alloc::alloc::alloc()`,
which returns a virtual pointer into `HEAP_MEM` (~0x213DC0). That's in the
identity-mapped kernel-image range, NOT in the offset-mapped range, so
`virt_to_phys(heap_ptr)` **underflowed** (`0x213DC0 - 0x18000000000` wraps
to `0xFFFFFFE8000213DC0`). The resulting leaf PTE had bits 48–55 set
(`0xFE8`), which the MMU treats as reserved → #PF with error code `0x1c`.

Fixed by allocating payload segment + stack backing from `FRAME_ALLOCATOR`
(which returns true physical addresses in the USABLE region) and copying
ELF bytes via `phys_to_virt(frame)`.

## Verified result

- `cargo +nightly check --workspace`: **clean** (exit 0, only pre-existing static_mut warnings).
- Bootimage builds.
- QEMU boot: **no triple-fault, no reboot loop** (reboot count = 1).
- Full boot sequence completes: CR0/CR4 → serial → VGA → heap → frame allocator
  → inject_payloads → IDT → PIC → System → GDT → spawn × 2 → `sti`.
- CR3 switch **works**: `CR3=0x16a4000` (in USABLE region, clean).
- Leaf PTE for payload entry is **clean**: `0x00000000016A5007`
  (Present|Writable|User, physical `0x16A5000`, no reserved/NX bits).
- **Payload executes in ring 3** (CPL=3 confirmed), reaches RIP `0xFFFFFFFF80102480`
  (~9 KB into payload) before hitting a userspace #PF (CR2=0, null deref).
  That is a **payload logic bug**, not a kernel bug.

## Remaining work (separate from this kernel bring-up)

- **Payload null dereference**: the payload (shell.elf) reaches a null-pointer
  read around RIP `0xFFFFFFFF80102480`. The auto-benchmark ("10,000 SYS_YIELDs
  took...") does not complete because of this. Investigating it is userspace
  debugging, not kernel work.
- **Process isolation is weak**: all processes share the bootloader's full
  physical-memory mapping. A proper isolate-each-process design would build
  per-process PML4s that map only kernel + own user pages. Acceptable for
  bring-up; future work for real isolation.
- **C4, C5 from the prior NOTES**: spinlock IRQ masking, syscall pointer
  validation. Unchanged from prior session.

## Files changed this session

- `x86_64-kernel/Cargo.toml` — enable `map_physical_memory` feature.
- `x86_64-kernel/src/main.rs` — `_start(boot_info)`, `set_phys_offset`, scan
  memory_map for USABLE region, init FrameAllocator on it.
- `kernel-kit/src/paging.rs` — `PHYS_OFFSET`, `phys_to_virt`, `virt_to_phys`,
  `walk()`; fixed all phys-address dereferences in `duplicate_pml4`/`map_segment`.
- `kernel-kit/src/memory.rs` — rewrote `FrameAllocator` to take
  `(base_phys, num_frames)` and track up to 16384 frames.
- `kernel-orchestrator/src/syscall.rs` — SYS_EXEC PT_LOAD + user stack backing
  now comes from `FRAME_ALLOCATOR` instead of `alloc::alloc`.

## Key empirical facts (verified this session)

- `physical_memory_offset = 0x0000018000000000`
- USABLE region: frames `0x16A0..0x7FE0` (physical `0x16A0000..0x7FE0000`, ~99 MiB)
- Kernel symbols: `_start = 0x2085E0`, `HEAP_MEM = 0x213DC0` (identity-mapped,
  NOT offset-mapped — this is why heap pointers must not be passed to virt_to_phys)
- Aliasing test: write at `phys_to_virt(0x16A0000)`, read back at the same — OK.
- Final CR3 in QEMU: `0x16a4000` (clean USABLE region frame).
- Final leaf PTE for payload entry: `0x00000000016A5007` (clean).

Timestamp: 2026-07-17T15:45:22.340448

---

# GAP 2 IMPLEMENTED — IRQ-aware lock stack (2026-07-17)

## What was built

Added `IrqSpinlock<T>` to `kernel-kit/src/memory.rs` alongside the
existing `Spinlock<T>`. The stack follows the stage contract in
ATOM-STACK-KERNEL-DESIGN.md Appendix B:

    if_save      -> read RFLAGS.IF into a u8
    irq_disable  -> cli
    cas_acquire  -> atomic CAS (REQUIRES irqs_off=true — the ordering
                    hazard the operator flagged)
    [caller crit sec]
    release_restore -> store=false + conditional sti

Three helper primitives added:
  - `read_if() -> u8`        (RFLAGS bit 9)
  - `disable_irq()`          (cli)
  - `restore_if(saved_if)`   (conditional sti)

Two lock flavors, deliberately not unified:
  - `Spinlock<T>`    for non-IRQ-crossing callers (ROOT_FS, CURSOR, AtomHeap)
  - `IrqSpinlock<T>` for IRQ-crossing callers (SERIAL1, KEYBOARD_BUFFER,
                     FRAME_ALLOCATOR)

The split is from the mechanism dissolve (Appendix A): IRQ masking
costs cycles; not every caller needs it. Applying IRQ masking to
locks that never see IRQ context would be the over-engineering the
dissolve warns against.

## What was converted (3 statics, ~15 call sites)

  kernel-kit/src/serial.rs      SERIAL1          -> IrqSpinlock
  kernel-kit/src/io.rs          KEYBOARD_BUFFER  -> IrqSpinlock
  kernel-kit/src/memory.rs      FRAME_ALLOCATOR  -> IrqSpinlock

Each call site now captures `sif` from `lock()` and threads it to
`unlock(sif)`. Pattern: `let (obj, sif) = STATIC.lock(); obj.method();
STATIC.unlock(sif);`

## Stage-contract hazards caught and designed around

  1. cas_acquire MUST come after irq_disable (Appendix B hazard).
     Encoded as in_invariant(cas_acquire) = {irqs_off=true}.
  2. release_restore uses restore_if(saved_if), not unconditional sti,
     so a caller that was already IRQ-disabled is not re-enabled by us.
     This is a SEMANTIC invariant the shape check alone would miss.

## Verification (T3)

  - `cargo +nightly check --workspace`: clean, exit 0.
  - Bootimage builds.
  - QEMU boot: reboot count = 1 (no triple-fault, no deadlock).
  - Full boot sequence completes through `sti`; payload executes in
    ring 3 exactly as before the lock change.
  - The kernel runs IDENTICALLY to its pre-change behavior — which is
    the correct signature of a pure correctness fix. No behavior shift,
    no perf regression visible at this granularity.

## What this did NOT do (honest scope)

  - Did not add backoff (T2 — sound math but low-contention kernel
    may not need it; deferred until T3 contention measurement justifies).
  - Did not add lockdep watchdog (T0 for detection but threshold tuning
    needs workload data; deferred).
  - Did not unify Spinlock and IrqSpinlock (deliberate — different
    mechanisms, different costs).

## Currency not yet measured

The named currency for GAP 2 was "worst-case IRQ-disabled window
(cycles)" and "spin-wait count." This implementation did not add
instrumentation to measure those — the gate that mattered was binary
(no deadlock), and it cleared. Measuring the IRQ-disabled window
requires rdtsc bracketing around the cli...cas_acquire window, which
is future work once we have a workload that stresses contention.

## Files changed

  kernel-kit/src/memory.rs    +IrqSpinlock, +read_if/disable_irq/restore_if,
                              FRAME_ALLOCATOR type changed
  kernel-kit/src/serial.rs    SERIAL1 -> IrqSpinlock
  kernel-kit/src/io.rs        KEYBOARD_BUFFER -> IrqSpinlock
  kernel-orchestrator/src/syscall.rs   all SERIAL1/KEYBOARD_BUFFER/FRAME_ALLOCATOR sites updated
  x86_64-kernel/src/main.rs   all SERIAL1/KEYBOARD_BUFFER/FRAME_ALLOCATOR sites updated

---

# GAP 1 PARTIAL — Slab heap stack: implemented + self-test PASS (2026-07-17)

## What was built

Added `kernel-kit/src/slab.rs` (~550 lines) implementing the GAP 1 stack
from ATOM-STACK-KERNEL-DESIGN.md Appendix B:

    STAGE bucket_route       hash(layout.size) → bucket idx
    STAGE free_list_pop      LIFO pop (the queueing-theory invariant)
    STAGE avalanche_tag_write  write integrity hash to header
                              (HAZARD: must run AFTER pop, on user ptr,
                               not on free-list head — verified)
    ... user memory ...
    STAGE avalanche_tag_verify  re-read header, compare to written
    STAGE free_list_push     LIFO push

Design decisions (mechanism-matched, not name-matched):

  - 8 size classes (16/32/64/128/256/512/1024/2048 usable bytes) tuned
    to the kernel's T0-surveyed allocation profile (most allocs are
    24-256 B Vec/Context/String; few are KiB-scale).
  - 16-byte header (SlabHeader{tag:u64, bucket_plus_one:u8, pad}).
  - Free-list stores next-pointer IN the freed node's user area (classic
    slab trick; zero per-bucket heap overhead).
  - BumpAllocator fallback for sizes above the largest class — avoids
    paying buddy-split complexity (Appendix A rejected buddy as suspect).
  - SlabLocked wraps SlabHeap+BumpAllocator in a Spinlock and impls
    GlobalAlloc. Uses Spinlock not IrqSpinlock (T2: allocs happen from
    syscall/mainline, not IRQ context — matches existing AtomHeap).
  - Stats counters (alloc_count, slab_hits, slab_misses, tag_mismatches)
    expose the named currency for runtime measurement.

## Verification (T3 — measured on the kernel's own currency)

Added `slab_self_test()` called from `_start` on a private 64 KiB
region (the tail of HEAP_MEM, which the bump allocator never reaches).
The test does alloc(N=64) → free(N) → alloc(N) again and reports.

QEMU output (verbatim):
    slab_self_test: pass1_alloc=64 pass1_hits=0 dealloc=64
                    pass2_hits=64 misses=0 mismatch=0
    slab_self_test: GATE PASS (free-list round-trip OK)

Gate rationale: pass2_hits must == N (64) for the free-list round-trip
to be correct. It does. misses=0 confirms no size fell through to bump
on the second pass; mismatch=0 confirms the avalanche-tag VERIFY stage
caught no corruption.

The Appendix B hazard (tag-write-before-pop would corrupt the
free-list head) is verified designed-around: a wrong ordering would
have shown pass2_hits=0 + mismatch>0.

Kernel boots stable (reboot count = 1), full sequence completes,
payload still runs in ring 3 — slab_self_test is invisible to normal
operation, which is the correct signature for a non-invasive test.

## What is NOT done (honest scope)

  - Slab is NOT yet wired as the active #[global_allocator]. The kernel
    still uses AtomHeap(BumpAllocator). Wiring it globally requires
    touching the #[global_allocator] static and re-verifying every
    existing allocation path still works under the slab (Vec, Box,
    String, with_capacity, etc).
  - OOM-after-N benchmark vs the documented BumpAllocator baseline
    (~65k for a freeing workload) has NOT been run yet. That requires
    a workload that exercises the global allocator under load — best
    done after the slab is the active allocator.

## Currency reported (real measurement)

  pass2_hits / pass2_attempts = 64/64 = 100% free-list reuse.
  This is the slab's design goal — O(1) alloc/free for fixed sizes
  with no bump growth. Measured, not asserted.

## Files changed

  kernel-kit/src/slab.rs    NEW (~550 lines)
  kernel-kit/src/lib.rs     +pub mod slab
  x86_64-kernel/src/main.rs +slab_self_test call in _start

---

# GAP 1 COMPLETE — SlabLocked is the active global allocator (2026-07-17)

## What was done

Promoted SlabLocked from "self-tested but not wired" to the kernel's
active #[global_allocator]. Every Vec, String, Box allocation now
routes through bucket_route → free_list_pop → avalanche_tag_write
→ avalanche_tag_verify → free_list_push.

  x86_64-kernel/src/main.rs:
    #[global_allocator] static ALLOCATOR: AtomHeap
        -> static ALLOCATOR: SlabLocked = SlabLocked::new();
    ALLOCATOR.0.lock().init(...) -> ALLOCATOR.init(heap_start, HEAP_BYTES)

  kernel-kit/src/slab.rs:
    + oom_after_n_benchmark() — measures N (alloc/free cycles before
      OOM) on equal private regions for slab vs bump.

## Measured currency (T3 — kernel's own benchmark, QEMU)

The OOM-after-N benchmark runs alloc(64B) → free → repeat, on private
256 KiB regions, capped at 100,000 cycles. Verbatim QEMU output:

    oom_after_n: slab=100000 (oom=no, capped=100000)  bump=4096 (oom=yes)
    oom_after_n: GATE PASS — slab ran indefinitely, bump OOMed

Decoded:
  SlabHeap:    100,000 cycles, no OOM, hit the cap (would keep going).
  BumpAllocator: 4,096 cycles, then OOM.
                Matches math: 256 KiB / 64 B = 4,096 exactly.
  Improvement: ≥ 24× (100,000 / 4,096, capped — true ratio is unbounded
              because slab recycles and bump can't).

The bump baseline is the documented behavior from NOTES.md (dealloc
is a no-op → OOM-after-N = heap_bytes / mean_alloc_size). The slab
crushes it because its free-list actually recycles — which is the
design goal stated in ATOM-STACK-KERNEL-DESIGN.md GAP 1.

## Side-effect verification

The kernel's normal boot path is now a real test of the slab under
mixed workloads: Vec::new() then push (24B + realloc to 4KiB when
the kernel_stack Vec grows), String::from("shell.elf"), the
AtomNode tree for ROOT_FS, Context structs during spawn_process,
etc. All of these ran correctly:

  "Heap Allocation Test: PASS!"        (Vec::new + push(42))
  "PIC Initialized."
  "Orchestrator Initialized."
  "Ring 3 Multi-Tasking Spawned."      (2x spawn_process, each allocates
                                        kernel_stack Vec + Context)
  "System running autonomously..."
  payload runs to RIP 0xFFFFFFFF80102480 (unchanged from pre-slab)

Reboot count = 1 (no triple-fault, no allocator-induced hang). The
slab is mechanically invisible to behavior except that it doesn't
OOM under freeing workloads — exactly the correct signature.

## What this DID NOT prove (honest scope)

  - Did NOT measure latency (cycles per alloc). The currency measured
    is OOM-after-N, which is a throughput/recycling test. Latency
    would need rdtsc-bracketing around individual allocs.
  - Did NOT measure fragmentation under sustained mixed-size workloads.
    The slab's bump fallback (for sizes > 2048B) still can't reclaim,
    so a workload dominated by large allocations would eventually OOM.
    Real mixed workloads need testing.
  - Did NOT measure under contention (the slab's Spinlock serializes).
    Single-CPU kernel has no contention today; SMP would change this.
  - The benchmark uses Layout(64, 8) — a single fixed size that hits
    bucket 4 (256B class). Workloads with skewed size distributions
    need separate measurement.

## Files changed this step

  x86_64-kernel/src/main.rs    ALLOCATOR type + init + benchmark call
  kernel-kit/src/slab.rs       +oom_after_n_benchmark()

## Currency summary (one real number)

  Slab vs Bump, alloc(64B)/free loop, 256 KiB region, kernel boot:
    slab: 100,000+ cycles (capped, no OOM)
    bump: 4,096 cycles (OOM)
    ratio: ≥ 24×, measured on the kernel's own runtime.

This is the first measured "atom-stack beats baseline" result for the
kernel. The doctrine (compose cross-domain atoms in a fixed chain,
measure the named currency) produced a real win on a real workload.

---

# Payload #PF root cause FOUND — per-process AS isolation bug (2026-07-17)

## The symptom

Payload (shell.elf) faults at RIP=0xFFFFFFFF80102480 (memset) with
CR2=0, error code 0x0004 (user-mode, not-present). This is NOT a null
deref — it's an instruction-fetch failure. The address 0x80102480 is
inside shell.elf's .text PT_LOAD (vaddr 0x80100000, memsz 0x24c5).

## Diagnostic that found it

Added a walk()-based PTE dump right after SYS_EXEC's mapping, for both
spawned payloads. QEMU output:

    P2480=00000000016AF007 L=0 [0]L=0 00000000016A7007   <- shell.elf
    P2480=0000000000000000 L=0 [0]L=0 00000000016B7007   <- daemon.elf

Decoded:
  shell.elf's mapping of 0x80102480 -> phys 0x16AF000, valid. ✓
  daemon.elf's mapping of 0x80102480 -> ZERO (not present). ✗
  Both map their own .text start (0x80100000) correctly.

So the per-payload mapping is correct in isolation. The fault occurs
because **the scheduler switches CR3 between address spaces during
payload execution**.

## Root cause

Both payloads (shell.elf, daemon.elf) are linked at the same virtual
address (0xFFFFFFFF80100010 entry). Each gets its own CR3 via
duplicate_pml4 + map_segment for its own PT_LOAD segments. The mappings
are correct WITHIN each address space.

But the timer IRQ fires during payload execution, the scheduler does a
context switch (which loads a different CR3 — see main.rs
timer_interrupt_handler), and the resumed task's RIP now resolves
against a different address space. Specifically: shell's
`callq 0x80102480` (memset) is correct in shell's CR3, but if the
scheduler has loaded daemon's CR3 when shell's RIP advances to that
call, the fetch faults because daemon's CR3 doesn't map 0x80102480.

This is the per-process address-space isolation problem, exactly the
"process isolation is weak" item flagged in NOTES.md from the
original fix session. Each payload gets its own PML4 (good), but the
SCHEDULER doesn't preserve RIP/CR3 pairing across switches — it loads
the next task's CR3 but the resumed task's RIP may be in a region
its new (wrong) CR3 doesn't map.

## Why this is GAP 5-scale, not a quick fix

A correct fix requires one of:
  (a) All payloads share a single address space (no per-process CR3).
      Simplest but loses isolation. Would require duplicate_pml4 to
      return the kernel CR3 always, and map_segment to write into the
      shared PML4.
  (b) The scheduler saves+restores the FULL task state including the
      CR3 the task was running under, and reloads that specific CR3
      on resume (not just the "next task's" CR3). This is what
      proper task_struct.ctx.page_table_root is supposed to encode,
      and the code attempts it, but the bug shows the wiring is off.
  (c) Don't preempt during user-mode execution (cooperative only) —
      the payload must SYS_YIELD to switch. This kills the benchmark
      semantics.

Option (b) is the correct fix and is real scheduling work — it
overlaps with GAP 5 (scheduler) which was already on the list.

## Fix attempts during this session

  - Added FrameAllocator::alloc_contiguous(n) — was a real bug (the
    PT_LOAD allocation got non-contiguous frames, breaking the
    phys_base+i*4K assumption in map_segment). Fixed and verified:
    shell's mapping now shows a clean valid PTE for 0x80102480.
  - But this was a contributing factor, not THE cause. The actual
    cause is the scheduler CR3 switch.

## Honest state

  - Kernel boots, switches CR3, drops to ring 3: VERIFIED.
  - Slab allocator active + 24× measured: VERIFIED.
  - IRQ-aware locks: VERIFIED.
  - Single payload would run cleanly (its mappings are correct in
    its own CR3).
  - Multi-payload concurrent execution: BROKEN by scheduler CR3
    mismanagement. CI benchmark (which needs the payload to run to
    completion) does not pass.

## Recommended next direction

Workload unblock options:
  (1) Spawn only ONE payload (comment out the daemon spawn) and
      verify the benchmark string prints. This isolates the test
      from the multi-CR3 bug and proves the slab+lock+mapping path
      works end-to-end.
  (2) Commit to GAP 5 (scheduler) and fix the CR3-save/restore
      properly. Larger but correct.

(1) first — it gives a clean measurement of the slab/lock/mapping
work without the scheduler bug masking it. Then (2) if you want the
multi-payload case to work.

---

# Payload #PF debug — additional finding + honest stop point (2026-07-17)

## What was tried after the scheduler hypothesis

Tested single-payload mode (commented out the daemon spawn). The #PF
PERSISTED at the same RIP (0x80102480 = memset), CR3 = shell's own
CR3 (0x16a4000). So the "scheduler switches CR3" hypothesis was
WRONG — even with only one payload, shell faults calling memset.

## The deeper diagnostic

Added a probe that prints, for the .text PT_LOAD: first_frame (where
the ELF bytes were copied), the leaf PTE for 0x80102480, and the
expected phys for that address. Output:

    ff=00000000016A6000   <- first_frame allocated for .text
    pte=00000000016A8007  <- leaf PTE: Present+Writable+User, phys 0x16A8000
    exp=00000000016A8480  <- first_frame + 0x2480 = where memset bytes are

These three values are CONSISTENT:
  - bytes were copied to first_frame+0..filesz (0x16A6000 + 0x24c5).
  - memset at .text+0x2480 lives at first_frame+0x2480 = 0x16A8480.
  - 0x16A8480 is in page 0x16A8000-0x16A9000.
  - the leaf PTE for 0x80102000-0x80103000 points at 0x16A8000.

So the leaf PTE correctly maps the page containing memset's bytes.

## The remaining mystery

Despite the leaf PTE being correct, the CPU still #PFs on the fetch
with CR2=0 (which on x86-64 means the WALK failed, not that the leaf
target was unreadable — a normal not-present leaf would set CR2=RIP).

This suggests one of:
  (a) the intermediate page-table page (PDPT or PD entry pointing at
      the PT page) is itself wrong, so the MMU's walk dereferences a
      null/wrong entry before reaching the leaf.
  (b) the PT page allocated by map_segment for the .text region was
      later overwritten or its frame was reused by another allocation
      (frame-allocator reuse bug).
  (c) the contiguous-frame range allocated for .text was actually NOT
      contiguous, so first_frame+0x2480 doesn't land in the page the
      leaf PTE points at (this would be a bug in alloc_contiguous).

I could not isolate which in this session. The diagnostic surface
needed (dump intermediate PML4/PDPT/PD entries for the .text vaddr,
check frame allocator bitmap state) requires deeper instrumentation
than I had time for.

## Honest stop point

  - Real wins delivered: GAP 2 (locks), GAP 1 (slab, 24x measured),
    contiguous-frame allocator, root-cause isolation of the payload
    #PF (not the scheduler; the page-table walk for .text is wrong
    in some way I haven't fully pinned).
  - The payload #PF is a genuine kernel bug in the SYS_EXEC page-table
    construction path, deeper than a one-line fix.
  - Daemon spawn restored to its original state; the kernel now has
    the slab+lock+contig improvements integrated and the workload
    still boots + spawns + faults as documented.

Recommended next step (when resuming): dump the FULL page-table
chain (PML4[511] -> PDPT[?] -> PD[?] -> PT[?]) for 0x80102480 from
QEMU monitor (`info tlb` or `xp/8gx <PT phys>`) to see exactly which
level has the wrong entry. That's the diagnostic that would resolve
hypothesis (a) vs (b) vs (c).

---

# Payload #PF FIXED — shared boundary page overwrite (2026-07-17)

## The actual root cause (not the scheduler hypothesis — that was wrong)

The payload's ELF has 4 PT_LOAD segments:
  .text   vaddr 0x80100000 memsz 0x24c5  (r-x)  -> ends at 0x801024c5
  .rodata vaddr 0x801024d0 memsz 0x0164  (r--)  -> starts at 0x801024d0
  .got    vaddr 0x80102638 memsz 0x0008  (rw-)
  .data   vaddr 0x80102640 memsz 0x0120  (rw-)

.text and .rodata SHARE page 0x80102000-0x80103000:
  - .text's tail (0x80102480 = memset) lives in that page.
  - .rodata's start (0x801024d0) lives in the SAME page.

The old SYS_EXEC mapped each PT_LOAD separately via map_segment.
When .rodata was mapped second, its map_segment call OVERWROTE the
leaf PTE for page 0x80102000-0x80103000, replacing .text's mapping
(first_frame+0x2000) with .rodata's mapping (rodata_first_frame).
Result: memset's instruction fetch hit an unmapped/wrong page → #PF.

This is a legitimate ELF layout — segments don't have to be page-
aligned relative to each other, only to their own alignment (2**12).
The kernel loader must handle boundary-shared pages correctly.

## The fix (verified)

Replaced the per-PT_LOAD loop with a span-based approach:

  PASS 1: scan all PT_LOAD phdrs to find [vmin, vmax).
  Allocate ONE contiguous run of frames covering the whole span
  page-aligned: span_nframes = ceil((vmax - vmin) / 4096).
  PASS 2: copy each PT_LOAD's bytes at its offset within the span.
  Map the whole span with ONE map_segment call.

Each leaf PTE is now written exactly once. The shared boundary page
correctly contains .text's tail AND .rodata's start, because both
were copied into the same physical frame at their respective offsets.

## Verification (T3)

QEMU output after fix:
  - Kernel boots normally, slab benchmark passes.
  - "Ring 3 Multi-Tasking Spawned."
  - "System running autonomously. Awaiting hardware events..."
  - Payload output: "1111" (the '1' is the first character of the
    benchmark string "10,000 SYS_YIELDs took...")
  - **0 #PF exceptions, 0 reboots, 40s+ stable runtime.**

The payload now executes SYS_WRITE syscalls and produces output.
It does NOT complete the benchmark (loops on the first character)
due to a SEPARATE issue: the syscall handler appears to clobber
a register the payload uses as a loop counter. That's a syscall-
path bug, not a mapping bug — and is exactly what GAP 3
(syscall/sysret + proper register preservation) would address.

## What was wrong with my earlier "scheduler CR3" hypothesis

I tested single-payload mode (commented out daemon) and the fault
persisted, which I documented as "deeper than the scheduler."
That was correct — the scheduler hypothesis was wrong. The actual
cause was the PT_LOAD boundary overwrite, which I found via the
walk_verbose diagnostic showing the leaf PTE pointing at the wrong
physical frame.

## Files changed this step

  kernel-orchestrator/src/syscall.rs  SYS_EXEC rewritten: span-based
                                      PT_LOAD mapping (PASS 1 scan,
                                      alloc_contiguous, PASS 2 copy,
                                      single map_segment call).
  kernel-kit/src/paging.rs           walk_verbose added+removed
                                      (was the diagnostic that found
                                      the bug; not needed in final).

## Currency

  #PF count: 2 -> 0   (verifiable in diag-serial.log)
  Payload output: none -> "1111..." (proof of execution)
  Stable runtime: <1s (fault) -> 40s+ (timeout)

---

# Context-switch state preservation bug — diagnosed, not yet fixed (2026-07-17)

## Symptom

After the payload #PF fix (span-based PT_LOAD mapping), the payload
no longer faults. It runs, executes SYS_WRITE syscalls, and produces
serial output. BUT: the benchmark string "10,000 SYS_YIELDs took..."
does not fully appear. Only four '1' characters print, then the
payload appears stuck.

## Diagnostic

Added a RIP probe to the timer handler (every 50 ticks, print the
saved RIP from the trap frame). Results:

    RIP=FFFFFFFF80100787
    RIP=FFFFFFFF8010072A
    RIP=FFFFFFFF80100787
    RIP=FFFFFFFF8010072A
    ...

The RIP oscillates between 0x787 and 0x72A — two addresses in the
shell's main input loop (SYS_READ → test → SYS_YIELD → loop). The
payload reached the shell loop, which means the benchmark DID start
executing. But the backward RIP movement (0x787 → 0x72A) proves
the timer's context switch is corrupting the saved task state.

## Root cause (identified, fix in progress)

Both `timer_interrupt_handler` and `syscall_interrupt_handler` write
to `task.rsp` via `switch_context`. The timer saves the user-mode
frame pointer; the syscall handler was ALSO saving its kernel frame
pointer to the same field. When they alternate (timer fires between
syscalls), `task.rsp` bounces between two different frame pointers.

Partial fix applied: the syscall handler no longer writes `task.rsp`
for non-switching syscalls (SYS_WRITE etc). Only SYS_YIELD / SYS_EXIT
trigger the save. This didn't resolve the oscillation, suggesting
the timer handler's own save/restore has a separate issue — possibly
that `switch_context` returns a stale rsp from a previous tick's
save, or the kernel stack pointer (TSS.esp0) changes between ticks.

## What was verified during diagnosis

  - IDT gate type for vector 0x80 is 0x8E (Interrupt Gate, not Trap
    Gate) — so IF is cleared on int 0x80 entry. The timer CANNOT
    nest inside a syscall handler. Confirmed the two handlers don't
    race within a single interrupt.
  - Masking the timer entirely (PICS mask IRQ0) causes NO output at
    all — the scheduler never dispatches any task. So the timer IS
    required for task dispatch.
  - Single-payload mode (daemon commented out) produces the same
    `1111` output — not a multi-task issue.
  - Payload disassembly confirmed the benchmark print is fully
    unrolled (no loop counter to corrupt). The '1' characters
    come from the first SYS_WRITE call (benchmark string starts
    with '1'), and the repetition indicates the task resumes at
    the wrong RIP after a timer-triggered context switch.

## Honest stop point

  - Payload #PF: FIXED (span-based PT_LOAD mapping). Real win.
  - Payload execution: WORKS (no faults, runs syscalls, produces
    output). Real win.
  - Benchmark completion: BLOCKED by context-switch state
    preservation bug in the timer handler's save/restore path.
    The fix requires reworking how the timer saves and restores
    the full task context (RSP + all GPRs + RIP) without
    interference from the syscall handler's own rsp management.

This is the GAP 5 (scheduler) territory — the context switch
mechanism needs proper save/restore of the complete task state,
not just the rsp field.

## Files in final state

All diagnostic code removed. Daemon spawn restored. Syscall handler
has the partial fix (no task.rsp overwrite for non-switching syscalls).
Workspace compiles clean. Kernel boots, spawns both payloads, no #PF.
