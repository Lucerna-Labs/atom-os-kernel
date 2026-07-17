# Atom-Stack Kernel — Design (revised: mechanism-first, trust-labeled)

Reference: Lucerna-Labs/atom-quantizer v0.2.3. The codec composes a fixed
PRE → EXTRACT → QUANTIZE → POST → VERIFY chain; each stage borrows one
invariant from a distinct domain; the chain pays a named currency that's
measured, not asserted.

## Epistemics — what we can trust, in what order

When deciding whether a primitive helps a kernel gap, trust levels descend:

  T0  MECHANICAL FACT — logical/structural invariant. Not a measurement.
      Example: "a spinlock with no IRQ save deadlocks if an IRQ that
      takes the same lock fires while it's held." Provably true.
      Use freely as a foundation.

  T1  SUBSTRATE MATH — translate to relationships/flows/constraints and
      reason from the math, not the domain label (nat.core.mathematical_substrate).
      Example: "a watermark is a monotone-nondecreasing scalar bound; using
      max(observed_event_time) as the GC horizon makes late events
      by-construction below the watermark." Math, not analogy.
      Trust the structure; distrust the framing.

  T2  COMMON SENSE — domain-general reasoning about how a mechanism
      behaves in THIS regime (kernel scheduling, heap fragmentation,
      IRQ latency), informed by reading this kernel's actual code.
      Weaker than T0/T1 because intuition has blind spots, but far
      more reliable than cross-domain sim results for an unstudied
      regime. (User steering: "common sense is more reliable than sims
      when little research exists.")

  T3  KERNEL MEASUREMENT — rdtsc, OOM count, fairness index on the
      actual kernel running in QEMU. The only real currency for
      "does this help atom-os-kernel." Required before any primitive
      goes live.

  T4  xdsim / SDF-marcher result — measures whether a primitive pays
      in a DIFFERENT regime (root-finding, field-eval budgets).
      The marcher encodes assumptions from a well-studied domain.
      Kernel gaps are not that domain. Treat a marcher win as
      "this pattern has paid a currency somewhere, so it's worth
      considering at T2" — NEVER as evidence it pays here.
      Do not gate kernel decisions on T4.

Default rule for this design: every claim cites its trust level. If a
claim has no trust level, treat it as T2 (informed guess) at best.

## What the kernel currently has (T0 — verified by file:line)

atoms.rs declares 8 primitives: scan, hash, fold, project, scale, compare, combine, order.

Usage:
  scan     page_table.rs:27, page_table.rs:38 (find free/valid slot)
  hash     page_table.rs:26 (DEMO — result discarded to `_hashed`)
  project  gdt.rs:122, interrupts.rs:31, paging.rs:102, vga.rs:67
  compare  syscall.rs ×32 (called twice per branch to fake `==`)
  combine  gdt.rs:74, interrupts.rs:35-37, memory.rs:69, page_table.rs:45, paging.rs:69
  fold     UNUSED
  scale    UNUSED
  order    UNUSED

Observation (T0): the kernel uses atoms as one-shot helpers, not composed
stacks. The first job is to identify which operations should BE stacks.

## Mechanism-first framing of the gaps

For each gap: state the underlying MECHANISM (not the vendor feature),
the conserved currency, and the primitives whose MECHANISM matches —
cited by structural reason, not by domain-name analogy. Every primitive
candidate gets a trust tag.

---

### GAP 1 — Heap allocator

Mechanism: route a variable-size request to a fixed-size slot, then
return the slot to a reusable pool on free. Two constraints:
  (a) alloc latency must be bounded and low (O(1) preferred)
  (b) free must reclaim, else the heap is a leaky bucket

Currency: alloc cycles, OOM-after-N (with N = sustained-alloc/free
operations before exhaustion), fragmentation ratio at N.

Current baseline (T0, memory.rs:89-100): BumpAllocator, dealloc = no-op.
OOM-after-N ≈ heap_bytes / mean_alloc_size ≈ 16MiB / 256B = ~65k
under any workload that frees as it allocs. A workload that frees is
indistinguishable from one that doesn't.

Primitive candidates (mechanism match):

  - BUCKET-ROUTE (slab-style)
    Mechanism: hash(layout.size) → fixed bucket → all allocs in a bucket
    are the same size → free is a stack push, alloc is a stack pop, O(1).
    Trust T0 for the mechanism (mathematically O(1)); T3 required for
    "this workload's size distribution has enough concentration to make
    buckets effective." Common sense (T2) says most kernel allocs are
    small + repeated (Vec<u8> buffers, Context structs), so the
    concentration is likely.

  - COALESCE-SPLIT (buddy-style)
    Mechanism: binary splitting of a power-of-2 region. Free coalesces
    with buddy if both free. O(log N) per op.
    Trust T0 for correctness. T2 caution: buddy fragments badly under
    mixed sizes that aren't powers of 2. May not pay for kernel use
    where sizes are weird.

  - FREE-LIST-AVALANCHE-TAG
    Mechanism: each block carries an avalanche hash of its metadata;
    double-free / use-after-free corrupts the hash and is caught on
    the next free of that block. (Borrowed integrity mechanism —
    similar in shape to FreeBSD PHK malloc, but we frame it as
    `hash` + `compare` atoms, not "import malloc".)
    Trust T0 for the detection invariant. T3 needed for the false-
    positive rate.

Stack: BUCKET-ROUTE → FREE-LIST-POP (queueing LIFO) →
       AVALANCHE-TAG-VERIFY → WATERMARK-PRE (see below)
Gate (T3): alloc latency < 2× bump allocator AND OOM-after-N ≥ 10× bump.

  - WATERMARK (heap GC horizon) — HYPOTHESIS at T2
    Mechanism: track max(free_time) across the heap; when watermark
    crosses a threshold, scan-and-reclaim unreferenced slabs. Borrowed
    from streaming's event-time watermark (a monotone bound), NOT from
    "the streaming domain."
    Trust T2: math is clean (monotone bound), but whether kernel
    workloads have enough temporal locality to make watermarking
    worth the scan cost is a T3 question. Don't go live without T3.

---

### GAP 2 — IRQ-aware lock

Mechanism: mutual exclusion across a context that may be preempted
by an interrupt that itself wants the lock.

Currency: worst-case IRQ-disabled window (cycles), spin-wait count.

Current baseline (T0, memory.rs:23-32): AtomicBool CAS spin, no IF mask.
Provably deadlocks (T0) on the first IRQ that re-enters.

Primitive candidates:

  - IF-SAVE-RESTORE
    Mechanism: read RFLAGS, mask IF (cli), CAS, restore RFLAGS.IF.
    Trust T0: this is the canonical spin_lock_irqsave invariant.
    No cross-domain analogy needed; the mechanism IS the fix.

  - BACKOFF (decorrelated)
    Mechanism: on CAS failure, delay a random exponentially-growing
    number of spins before retrying. Borrowed from ALOHA/networking
    contention theory.
    Trust T2: math (persistence drops with random backoff) is sound;
    T3 needed to confirm the kernel's contention level is high enough
    to matter. Low-contention single-CPU kernel may not need it.

  - LOCK-DEP-WATCHDOG
    Mechanism: rdtsc at lock acquire; on release, assert hold-time <
    threshold. Borrowed shape from lockdep, framed as
    `compare` + `scale` atoms.
    Trust T0 for the detection; T3 for threshold tuning.

Stack: IF-SAVE → CAS → IF-RESTORE → WATCHDOG-VERIFY
Gate (T3): IRQ-disabled window < 1000 cycles, no deadlock under nesting.

---

### GAP 3 — Syscall fast path

Mechanism: user→kernel transition + argument handoff + dispatch.

Currency: rdtsc delta per SYS_YIELD (matches the CI benchmark's currency).

Current baseline (T0, main.rs): int 0x80 IDT gate. T2 estimate ~200-400
cycles, dominated by the interrupt-gate stack switch.

Primitive candidates:

  - MSR-STAR-SYSCALL (syscall/sysret)
    Mechanism: the `syscall` instruction reads STAR/LSTAR/CSTAR MSRs to
    find the kernel entry; no stack switch by the CPU, no memory write.
    Trust T0 for the cycle count (well-documented in AMD64 manual).
    No analogy needed — this IS the faster primitive.

  - DISPATCH-TABLE
    Mechanism: replace 32-call linear `compare` chain with an indirect
    jump through a table indexed by syscall number. One load + one jump.
    Trust T0 for the mechanism (table dispatch is O(1)).
    This is the kernel's own `hash` + `project` atoms finally used
    correctly.

  - RIP-HOTNESS-SKETCH (Count-Min Sketch) — HYPOTHESIS at T2
    Mechanism: maintain a small CMS over syscall numbers issued; cache
    the top-K dispatch decisions in a fast lookup. Borrowed shape from
    streaming heavy-hitter detection.
    Trust T2 (math is sound — CMS bounds overestimate frequency
    provably), but T3 required because the workload may not have
    enough skew to make a fast-path pay. With only ~16 syscalls, the
    table dispatch above is already O(1); CMS may not add anything.
    Don't go live without T3 — this smells like theater.

Stack: SYSCALL-INSTR → DISPATCH-TABLE → SYSRET → rdtsc-VERIFY
Gate (T3): rdtsc delta < ½ the int-0x80 baseline on the same host.

---

### GAP 4 — TLB / CR3 switch

Mechanism: switching address spaces pays a TLB refill cost.

Currency: TLB-miss count per context switch (or instruction-fetch stalls
as proxy if perfctr unavailable).

Current baseline (T0, paging.rs): bare `mov cr3`, full TLB flush.

Primitive candidates:

  - PCID-TAG
    Mechanism: assign a 12-bit process-context ID; CR4.PCIDE lets the
    CPU keep TLB entries from different PCIDs simultaneously. mov cr3
    with bit 63 set = no flush.
    Trust T0: Intel SDM documents this. T3 required for "host QEMU
    exposes PCID" (use -cpu haswell or host).

  - GLOBAL-PAGE-BIT
    Mechanism: mark kernel page-table entries G (bit 8); CR3 load does
    NOT flush G-marked entries.
    Trust T0 (SDM). Independent of PCID, complementary.

  - ASID-LIKE-LAZY-FLUSH (the "RF repair" intuition) — HYPOTHESIS T2
    The user pointed at RF repair as a primitive family. Mechanism
    match: in RF, a finite-impulse-response low-pass reconstructs a
    signal from noisy samples by assuming local smoothness. Lazy TLB
    flush is structurally similar: defer flushing, accept stale
    entries briefly, repair on the next miss by re-walking. The
    invariant being borrowed is "tolerate local staleness, repair
    on demand." Whether this pays in a kernel context-switch regime
    is genuinely unstudied — common sense says it could help under
    bursty switches between few address spaces, but the risk is
    using a stale mapping. T3 required, with a hard VERIFY gate
    on correctness (no stale write to a freed page).

Stack: PCID-TAG → GLOBAL-PAGE → lazy-conditional-flush (HYPOTHESIS) →
       correctness-VERIFY (paranoid walk-on-write)
Gate (T3): TLB-miss-per-switch ≥ 3× lower than baseline AND zero
           correctness VERIFY failures over 10k switches.

---

### GAP 5 — Scheduler

Mechanism: pick next task, distribute CPU time fairly, bound latency.

Currency: Jain's fairness index across tasks, p99 scheduling latency.

Current baseline (T0): round-robin tick.

Primitive candidates:

  - VRUNTIME-INTEGRATOR
    Mechanism: each task accumulates vruntime = (real_time_run × 1024) /
    weight; pick min vruntime. Borrowed invariant: an integrator on
    CPU-time consumed, normalized by weight = proportional fairness.
    Trust T0 for the math (it's literally a discrete integrator).
    This is CFS, dissolved to its mechanism.

  - EEVDF-WEIGHTED-LAG
    Mechanism: compute each task's lag (how much earlier/later than
    ideal it has run); eligible tasks are those with lag ≥ 0; pick
    the earliest virtual-deadline.
    Trust T0 for the math. More complex than vruntime, with a real
    fairness improvement documented in the EEVDF paper.

  - SIGMA-DELTA-TIMESLICE — HYPOTHESIS T2
    Mechanism match: sigma-delta modulation turns a continuous value
    into a bit stream by integrating the error. Frame: target each
    task's CPU share as the "analog" signal; emit 1-tick quanta to
    tasks; integrate the share error. Over time, each task gets its
    target share, with bounded latency on the residual.
    Trust T2: the math is sound (sigma-delta converges), but common
    sense says this is over-engineered for a single-CPU educational
    kernel. Real value would show up only under many contending
    tasks with heterogeneous weights — T3 question.

Stack: VRUNTIME-INTEGRATOR → min-vruntime-pick → recompute-on-tick →
       Jain-fairness-VERIFY
Gate (T3): Jain ≥ 0.95 across 8 tasks over 10k ticks, latency ≤ 2× RR.

---

## Order of work (revised)

Common-sense priority (T2) for what to build first:

1. GAP 2 (lock IF-save) — T0 says current code is unsafe. This is a
   correctness fix, not an optimization. Mandatory regardless of any
   atom-doctrine aspirations.

2. GAP 1 (heap) — T0 says current code OOMs trivially. Mechanism
   match for slab/free-list is well-grounded; measurement is clean.

3. GAP 3 (syscall) — T0 says syscall/sysret is faster; direct cycle
   currency, ties to the existing CI benchmark.

4. GAP 5 (scheduler) — fairness is measurable but a real workload
   is needed to make the measurement meaningful.

5. GAP 4 (TLB) — needs host-capability probe; discovery-gated.

## What "faster than Linux" would actually require (T0 honesty)

Per nat.core.good_enough_is_earned: a working composition proves
possibility, not optimality. To claim "faster than Linux" on any axis:

  - Pick ONE axis (syscall latency / ctx-switch / alloc latency).
  - Build the stack with T0-grounded primitives.
  - Measure on the SAME hardware against Linux's equivalent path (T3).
  - Report the currency delta. If it doesn't pay, the stack is painted.

This is achievable for isolated axes where Linux's generality overhead
is real. It is NOT achievable as a general-purpose OS without the years
of subsystems Linux has that atom-os doesn't (SMP, RCU, page cache, IO
scheduler, ...). Honest framing: "atom-os can be lighter and faster on
paths it composes well; it cannot beat Linux as a general OS without
years more work."

## Verification status of this document

  - The atoms.rs audit (8 declared, 5 used, 3 dead) is T0 — verified
    by direct file:line inspection.
  - Each "T0" primitive claim is a structural invariant, not a
    measurement of this kernel.
  - Each "T2" hypothesis must survive T3 measurement before going live.
  - T4 (xdsim / SDF-marcher) is deliberately NOT cited as evidence for
    any kernel claim in this revision. A marcher win is at most a hint
    to consider a pattern at T2; it is never proof it pays here.

---

# APPENDIX A — Name-stripped mechanism dissolve

First revision of this doc treated primitives by domain label ("buddy
allocator", "watermark", "PCID"). That's still too name-bound. The right
move (per nat.core.domain_dissolution, demonstrated in this kit's own
cognitive-primitives catalog) is:

  1. Strip the surface name.
  2. Write the operation as root atoms (scan/hash/fold/project/scale/
     compare/combine/order) + a conserved quantity.
  3. Notice when the SAME dissolved math appears under a different name
     in another domain — that's a real candidate, not a metaphor.

The cognitive-primitives catalog documents exactly this move:
  - "bottom-up-attention" dissolves to  scan-for-outlier → mandatory capture
  - "salience-score"       dissolves to  project(features → priority scalar)
  - "biased-competition"   dissolves to  compare + combine with multiplicative gain
  - "top-down-attention"   dissolves to  combine(goal-template, sensory-map)
and each is cross-wired to non-cognitive aliases (interrupt-handler,
priority-queue, softmax-arbitration, etc.). The math is the body; the
name is the costume.

Re-running each kernel gap's dissolve and looking for matches I missed:

## GAP 2 (lock) — dissolved

Operation: achieve mutual exclusion across a context that may be
preempted by a re-entrant requester.
Math: a boolean slot, atomically flipped, while a higher-priority
preemption source is masked.

Dissolved candidates:
  - IF-SAVE-RESTORE: project(RFLAGS → IF-bit) + cli + CAS + restore.
    Pure root-atom composition. T0.
  - PRIORITY-PREEMPTION-MASK (same math as cognitive
    `bottom-up-attention` mandatory-capture inverted): mask the
    preemption source during the critical section instead of IF globally.
    The dissolve reveals this is what `cli` is doing already —
    IF is the "global preemption mask." No new primitive, just a
    recognition. T0.

The cognitive lens didn't add a new primitive here — it confirmed
that `cli` IS the cognitive mandatory-capture-suppression. Good sign
that the dissolve is honest (not every gap yields a novel match).

## GAP 5 (scheduler) — dissolved, NEW MATCH FOUND

Operation: choose which ready task runs next, distributing a finite
resource (CPU time) fairly across N requesters.
Math: a scalar priority per task, recomputed on each release; pick max
(or min if priority is "urgency").

Dissolved candidates:
  - VRUNTIME: integrator (fold over run-time, scaled by weight).
    T0 math. (Already had this.)
  - **SALIENCE-MAP / BIASED-COMPETITION (cognitive)** — NEW.
    Dissolved: combine each task's bottom-up urgency (e.g. waited-ticks,
    a stimulus-driven scan) with a top-down goal-template (e.g. "this
    task holds a lock others need"), via multiplicative gain; the winner
    is argmax. This is the SAME MATH as Itti-Koch salience + biased
    competition, and ALSO the same math as CFS weight × vruntime lag.
    The cognitive name suggests a feature CFS lacks: explicit
    goal-template biasing ("boost the task that holds a contended
    lock"). That's a real mechanism match, not a metaphor.
    Trust T1 for the math (it's compare+combine); T2 for the claim
    that the goal-template bias adds value over plain vruntime;
    T3 required.

## GAP 4 (TLB) — dissolved, RF-repair match refined

Operation: switch the active address space, paying as little as possible
in cache state displacement.
Math: maintain a small set of cached translations; on switch, decide
which to keep vs flush; on miss, recompute by walk.

Dissolved candidates:
  - PCID-TAG: project(ASID → CR3-low-bits). T0.
  - GLOBAL-PAGE-BIT: mark entries exempt from flush. T0.
  - **RF-FINITE-IMPULSE-RESPONSE-REPAIR (signal)** — refined.
    Dissolved math: a finite impulse response low-pass reconstructs a
    signal from noisy samples by assuming local smoothness. Mapped to
    TLB: keep stale translations briefly (treat them as noisy samples
    of the new space); repair lazily on the next miss via re-walk.
    The borrowed invariant is "tolerate local staleness, repair on
    demand." This is NOT just PCID — PCID partitions; this tolerates
    inconsistency within a partition temporarily.
    Trust T1 for the math (FIR smoothing is well-defined); T2 for
    "the kernel's access pattern is smooth enough that stale
    translations usually still hit"; T3 REQUIRED with a hard
    correctness-verify gate (stale write to a freed page = data
    corruption, not a perf regression). Marked discovery-only until
    VERIFY passes.

## GAP 1 (heap) — dissolved

Operation: route variable-size request to a slot; return slot to pool.
Math: project(layout → bucket) + LIFO free-list (stack of free slots)
+ integrity hash for double-free detect.

Dissolved candidates:
  - BUCKET-ROUTE + FREE-LIST-POP: hash(layout) → bucket → pop. T0.
  - **SPREADING-PRE-TRANSFORM (signal: Walsh-Hadamard analog)** — NEW.
    In atom-quantizer, rowH8 spreads outliers before block quantization
    so block budgets are used uniformly. Dissolved to kernel heap:
    before assigning an allocation to a bucket, hash-spread the
    address-space so adjacent allocations don't pile into the same
    slab page (which causes hot-page contention). The borrowed
    invariant is "spread outliers before quantizing."
    Trust T1 for the math (orthogonal rotation, norm-preserving);
    T2 caution — heap allocations don't have "outliers" the way
    weight tensors do. Smells like theater; flag for T3 rejection
    or validation, don't build unless T3 says it pays.

## GAP 3 (syscall) — dissolved, Count-Min Sketch re-evaluated

Operation: dispatch a numbered request through a fast path.
Math: project(syscall_num → handler) + dispatch.

Dissolved candidates:
  - DISPATCH-TABLE: hash(num) → table → jump. T0, O(1).
  - **HEAVY-HITTER-SKETCH (streaming CMS)** — re-evaluated honestly.
    Dissolved: maintain d×w counter array, increment on each syscall,
    query min-bucket for "most frequent syscall," cache its dispatch.
    Math (T1): CMS provably overestimates frequency, never underestimates.
    T2 common sense: with ~16 syscalls and table-dispatch already O(1),
    the sketch adds lookup work for no benefit. **The dissolve makes
    the theater visible:** the heavy-hitter math is real, but the
    problem it solves (compressed tracking of a huge item universe)
    doesn't exist when the universe is 16 items. Reject without T3.
    This is the move working as intended — name-stripping reveals the
    match is decorative here.

## What the dissolve changed

  - GAP 1: rejected the signal-spread PRE as likely theater (math is
    real but the regime doesn't have the property the math exploits).
  - GAP 2: cognitive lens confirmed existing design (cli IS mandatory-
    capture suppression). Good — honest dissolve doesn't manufacture
    novelty where there is none.
  - GAP 3: rejected CMS heavy-hitter as theater (math is real but the
    universe is too small to need it).
  - GAP 4: RF-FIR-repair match refined into a precise mechanism
    (tolerate temporary staleness within a PCID, repair on miss).
    Promoted from vague analogy to a T2 hypothesis with a hard
    correctness gate.
  - GAP 5: NEW candidate (salience + biased competition for
    goal-template-aware scheduling) found via the cognitive dissolve.
    Not in any prior revision. Genuinely new math, not a renamed CFS.

The dissolve both found a new candidate (GAP 5 salience-bias) and
rejected two painted ones (GAP 1 spread, GAP 3 CMS). That's the
move doing real work: name-stripping cuts both ways.

## Honest tally after the dissolve

  T0 (mechanical, foundation): GAP 2 IF-save, GAP 3 dispatch-table,
                              GAP 4 PCID+global, GAP 1 bucket-route
  T1 (math is sound, regime fit unclear):
                              GAP 4 RF-FIR-stale-repair,
                              GAP 5 salience-bias scheduler
  T2 (theater-suspect, reject unless T3 saves it):
                              GAP 1 Walsh-spread-PRE,
                              GAP 3 CMS heavy-hitter
  T3 (kernel measurement):    required for everything above to go live

Order of work unchanged: GAP 2 (correctness), GAP 1 (cleanest currency).
Then GAP 5 salience-bias is the most interesting T1 candidate to test
in the kernel because its math genuinely differs from CFS — it could
pay or fail, and either result is informative.

---

# APPENDIX B — Stacking-order hazard analysis

Operator warning (recorded 2026-07-17): "stacking order can make
something fail that would normally pass." This is a different failure
mode from the decorative-metaphor problem Appendix A addresses.

  - Decorative-metaphor failure: a primitive's math doesn't match the
    regime. Visible at the dissolve (Appendix A's GAP 1 Walsh-spread
    and GAP 3 CMS rejections).
  - Stacking-order failure: every stage's math is individually correct,
    but stage K+1 needs a property that stage K destroyed. Invisible
    to the dissolve, because each stage in isolation is sound.

The atom-quantizer doctrine guards this with the `state_shape`
annotation (2D/1D/any) — "a stack is well-formed when every stage's
shape is compatible with what the prior stage produced." That catches
shape mismatches. It does NOT catch invariant mismatches (e.g. "stage K
sorted; stage K+1 needs insertion order"). So we need a stronger
discipline here: every stage declares (in-shape, out-shape,
preserves-list, destroys-list) and a stack is REJECTED at design time
if any stage consumes a property an earlier stage destroyed.

## Stage-contract notation

For each stage, write:

    STAGE <name>
      in_shape:    <data layout it requires>
      in_invariant:<properties that must hold on input>
      op:          <what it does>
      out_shape:   <data layout it produces>
      preserves:   <in_invariants it keeps>
      destroys:    <in_invariants it breaks>
      introduces:  <new invariants it establishes>

A stack S1 → S2 → S3 is well-formed iff for every adjacent pair
(S_i, S_{i+1}): out_shape(S_i) ⊇ in_shape(S_{i+1}) AND
(in_invariant(S_{i+1}) ∩ destroys(S_i)) = ∅.

If any check fails, the composition is suspect even if every stage
is individually correct.

## Re-checking GAP 2 (IRQ-aware lock) under this discipline

    STAGE if_save
      in_shape:    (lock_addr, calling_context)
      in_invariant:{interrupts may fire}
      op:          read RFLAGS.IF, stash it
      out_shape:   (lock_addr, calling_context, saved_if)
      preserves:   {interrupts may fire}  (just observed, not changed yet)
      destroys:    ∅
      introduces:  {saved_if is the pre-lock IF state}

    STAGE irq_disable
      in_shape:    (lock_addr, calling_context, saved_if)
      in_invariant:{saved_if recorded}
      op:          cli
      out_shape:   (lock_addr, calling_context, saved_if, irqs_off=true)
      preserves:   {saved_if recorded}
      destroys:    {interrupts may fire}  ← cli kills this
      introduces:  {irqs_off=true, atomic w.r.t. IRQ context}

    STAGE cas_acquire
      in_shape:    (lock_addr, ..., irqs_off=true)
      in_invariant:{irqs_off=true}        ← REQUIRED
      op:          atomic CAS on lock_addr
      out_shape:   (lock held)
      preserves:   {irqs_off=true}
      destroys:    ∅
      introduces:  {lock held}

    STAGE do_critical_section
      in_shape:    (lock held, irqs_off=true)
      in_invariant:{lock held, irqs_off=true}
      op:          <user code>
      out_shape:   (lock held, irqs_off=true)
      preserves:   all
      destroys:    ∅
      introduces:  ∅

    STAGE release_and_restore
      in_shape:    (lock held, saved_if, irqs_off=true)
      in_invariant:{lock held}
      op:          store lock=free; if saved_if then sti
      out_shape:   (caller resumed)
      preserves:   ∅
      destroys:    {lock held, irqs_off}
      introduces:  {interrupts may fire} (iff saved_if)

Stacking check, adjacent pairs:
  if_save → irq_disable: out(if_save) shape ⊇ in(irq_disable)? yes.
    in_invariant(irq_disable)={saved_if recorded}; destroys(if_save)=∅.
    Intersection = ∅. OK.
  irq_disable → cas_acquire: shape ⊇? yes.
    in_invariant(cas_acquire)={irqs_off=true}; destroys(irq_disable)
    includes {interrupts may fire} but NOT {irqs_off=true}.
    Intersection = ∅. OK.
  cas_acquire → do_critical_section: OK trivially.
  do_critical_section → release_and_restore: OK trivially.

**Hazard found and rejected before implementation:** the obvious wrong
ordering would be cas_acquire → irq_disable (acquire first, then mask
IRQs). in_invariant(irq_disable) is vacuous so the shape check passes
— but the SEMANTIC invariant "no IRQ fires between acquire and disable"
is violated. This is exactly the operator-warning failure mode: both
stages are individually correct, the composition type-checks, but the
lock is broken because an IRQ can hit in the gap.

The fix: require an explicit **semantic** invariant on cas_acquire
("must be called with IRQs disabled") that propagates back through
the chain. The stage-contract notation catches it only if we list
"irqs_off=true" under in_invariant(cas_acquire). Lesson: shape alone
is insufficient; the in_invariant list must include semantic properties
the stage relies on.

## Re-checking GAP 5 (salience-bias scheduler) under this discipline

    STAGE bottom_up_scan
      in_shape:    task_list
      in_invariant:{all tasks have a waited_ticks counter}
      op:          read each task's waited_ticks since last run
      out_shape:   [(task, urgency)]
      preserves:   {task identity}
      destroys:    ∅
      introduces:  {urgency is a non-negative scalar per task}

    STAGE top_down_goal_template
      in_shape:    [(task, urgency)], lock_state
      in_invariant:{lock_state is current}
      op:          for each task, look up "does this task hold a
                   contended lock?" → boost_factor
      out_shape:   [(task, urgency, boost)]
      preserves:   {task identity, urgency}
      destroys:    ∅
      introduces:  {boost is a non-negative scalar per task}

    STAGE combine_multiplicative
      in_shape:    [(task, urgency, boost)]
      in_invariant:{urgency≥0, boost≥0}
      op:          score = urgency * boost
      out_shape:   [(task, score)]
      preserves:   {task identity}
      destroys:    {urgency, boost as separate fields} (collapsed into score)
      introduces:  {score is a non-negative scalar, monotone in both inputs}

    STAGE pick_argmax
      in_shape:    [(task, score)]
      in_invariant:{score≥0}
      op:          choose task with max score
      out_shape:   chosen_task
      preserves:   ∅
      destroys:    all per-task state
      introduces:  {chosen_task is one of the inputs}

Stacking check:
  bottom_up_scan → top_down_goal_template: shape ok.
    in_invariant(top_down)={lock_state current}; bottom_up destroys=∅. OK.
  top_down → combine: shape ok.
    in_invariant(combine)={urgency≥0, boost≥0}; top_down introduces boost≥0,
    urgency was already ≥0. OK.
  combine → pick_argmax: shape ok.
    in_invariant(pick_argmax)={score≥0}; combine introduces score≥0. OK.

**Hazard found and rejected before implementation:** the wrong order
top_down → bottom_up (boost first, then scan urgency) seems fine —
both stages commute mathematically (multiplication commutes). But if
top_down's `lock_state` lookup itself takes a lock, and bottom_up
runs AFTER that lock is taken, the scan can't run because the
scheduler itself is holding a lock the scan needs. The semantic
invariant "bottom_up_scan must not require any lock" has to be
recorded and the stack order preserved even though the MATH commutes.
Lesson: order-sensitivity comes from side-effects, not just from
mathematical non-commutativity.

## Re-checking GAP 1 (heap) under this discipline

    STAGE bucket_route
      in_shape:    layout
      in_invariant:{layout.size>0}
      op:          hash(layout.size) → bucket
      out_shape:   bucket_id
      ...
    STAGE free_list_pop
      in_shape:    bucket_id
      in_invariant:{bucket free-list non-empty}
      op:          pop head of LIFO
      out_shape:   ptr
      ...
    STAGE avalanche_tag_write  (VERIFY stage)
      in_shape:    ptr, layout
      in_invariant:{ptr writable}
      op:          write hash(layout+caller) into header word
      ...

Stacking check:
  bucket_route → free_list_pop: ok.
  free_list_pop → avalanche_tag_write: ok.

**Hazard:** if avalanche_tag_write is moved BEFORE free_list_pop
(tag-then-pop), the tag gets written to the bucket free-list head
pointer itself rather than to the user's allocation, corrupting the
free-list. The semantic invariant "tag_write operates on the user
pointer, not the free-list structure" must be recorded.

## General rule extracted (operator-warning formalized)

  1. Every stage declares in_shape, in_invariant, out_shape,
     preserves, destroys, introduces.
  2. Adjacent-stage check requires BOTH:
        out_shape(S_i) ⊇ in_shape(S_{i+1})
        in_invariant(S_{i+1}) ∩ destroys(S_i) = ∅
  3. SEMANTIC invariants (not just shape) must be listed — e.g.
     "irqs_off=true", "ptr is user pointer not free-list head",
     "lock_state lookup must not take a lock."
  4. Even mathematically commutative stages can be order-sensitive
     when side effects exist. The contract must capture side effects
     under "destroys" or "introduces."
  5. The stack is REJECTED at design time if any pair fails. No
     "I'll fix it in implementation" — that's exactly when the
     failure hides.

This appendix is the guard against the operator warning. It will be
applied to every stack before code is written, and the contract
appears as a comment block above each stack implementation.
