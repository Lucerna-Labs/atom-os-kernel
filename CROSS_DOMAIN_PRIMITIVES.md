# Cross-Domain Mathematical Primitives for Atom OS Kernel

## Overview

This document describes the cross-domain mathematical primitives that have been implemented in the Atom OS kernel. Each primitive borrows mechanisms from other domains (signal processing, cognitive science, control theory, information theory) and implements them using only the 8 root atoms defined in the Atom Doctrine.

**Key Principle:** All primitives are **100% dependency-free** - they use only the existing kernel infrastructure and the 8 root atoms.

## Implemented Primitives

### 1. Salience + Biased Competition Scheduler (Cognitive Science)

**File:** `kernel-orchestrator/src/salience_scheduler.rs`

**Domain:** Cognitive Neuroscience (Itti-Koch model)

**Mechanism:** 
- Combines bottom-up urgency (ticks waited) with top-down goal templates (lock contention, I/O readiness)
- Uses multiplicative gain: `score = urgency * boost`
- Selects task with maximum score

**Math:**
```
score[i] = waited_ticks[i] * boost_factor[i]
winner = argmax(score)
```

**Atoms Used:**
- `scan`: Iterate over tasks to find the best one
- `project`: Map task properties to boost factors
- `combine`: Multiply urgency by boost (using integer arithmetic)
- `order`: Compare scores to find maximum

**Trust Level:** T1 (math is sound, regime fit needs T3 verification)

**Stage Contract:**
```
bottom_up_scan -> top_down_goal_template -> combine_multiplicative -> pick_argmax
```

**Features:**
- Fixed-point arithmetic (16.16) for boost factors
- Three goal templates: Normal, LockContention, IoBound
- Lock contention boost: 2.0x priority for lock holders
- I/O bound boost: 1.5x priority for I/O ready tasks
- Integer-only implementation (no floating point)

**Usage Example:**
```rust
use kernel_orchestrator::salience_scheduler::{SalienceScheduler, GoalTemplate};

let mut scheduler = SalienceScheduler::new();

// Set goal template to boost lock holders
scheduler.set_goal_template(GoalTemplate::LockContention);

// Mark task 0 as holding a contended lock
scheduler.mark_lock_contention(0, true);

// Spawn tasks
scheduler.spawn(Context::new(1, rsp1, stack1, cr3_1)).unwrap();
scheduler.spawn(Context::new(2, rsp2, stack2, cr3_2)).unwrap();

// On each timer tick
scheduler.tick();

// Switch context (called from timer IRQ)
let new_rsp = scheduler.switch_context(old_rsp);
```

**Verification:**
- Unit tests verify basic functionality
- Lock contention boosting works correctly
- Stage contract is well-formed (verified in documentation)

---

## Design Patterns for Cross-Domain Primitives

### Pattern 1: Mechanism Dissolve

The process for identifying cross-domain primitives:

1. **Identify the kernel problem** (e.g., "fair task selection")
2. **Strip domain-specific names** (e.g., "scheduler" -> "resource allocation")
3. **Express as mathematical operation** (e.g., "pick max from weighted scores")
4. **Find matching mechanisms in other domains** (e.g., "salience + biased competition")
5. **Verify the math matches** (e.g., both use argmax of weighted sum)
6. **Implement using root atoms** (scan, project, combine, order)
7. **Verify stage contracts** (no hazards, proper ordering)

### Pattern 2: Stage Contract Composition

Each cross-domain primitive should:

1. Define clear stages with input/output shapes
2. Specify invariants (what must be true at each stage)
3. Identify what each stage preserves/destroys
4. Verify adjacent stages are compatible
5. Check for hazards (ordering issues, invariant violations)

### Pattern 3: Atom-Based Implementation

All implementations must use only the 8 root atoms:
- `scan`: Traverse/find in collections
- `hash`: Identify/map values
- `fold`: Reduce/accumulate
- `project`: Map/transform
- `scale`: Resize/multiply
- `compare`: Check boundaries/conditions
- `combine`: Merge/join
- `order`: Sort/select based on priority

---

## Future Primitive Candidates

Based on the ATOM-STACK-KERNEL-DESIGN.md analysis, here are additional cross-domain primitives that could be implemented:

### 2. FIR Filter for TLB Staleness Tolerance (Signal Processing)

**Domain:** Digital Signal Processing

**Mechanism:**
- Tolerate stale TLB entries temporarily
- Repair lazily on TLB miss
- Use FIR filter to decide which entries to keep

**Math:**
```
output[n] = Σ h[k] * confidence[n-k]
keep_entry = (output >= threshold)
```

**Atoms:** fold, scale, compare, project

**Status:** Identified in design doc, not yet implemented (requires T3 verification)

### 3. PID Controller for Adaptive CPU Throttling (Control Theory)

**Domain:** Control Systems Engineering

**Mechanism:**
- Proportional-Integral-Derivative feedback loop
- Adjust CPU frequency based on load error
- Maintain target load with minimal oscillation

**Math:**
```
output = Kp * error + Ki * ∫error + Kd * d(error)/dt
```

**Atoms:** scale, fold, combine, compare

**Status:** Identified, not yet implemented

### 4. Minimal Perfect Hash for Syscall Dispatch (Information Theory)

**Domain:** Data Compression / Information Theory

**Mechanism:**
- Create perfect hash function for syscall numbers
- O(1) lookup with zero collisions
- For small universe (16 syscalls), use identity mapping

**Math:**
```
h: {1,...,N} -> {0,...,N-1} (bijective)
```

**Atoms:** project, hash, compare

**Status:** Identified, not yet implemented (current dispatch uses linear search)

### 5. Exponential Moving Average for Load Prediction (Statistics)

**Domain:** Time Series Analysis

**Mechanism:**
- Recursive filter for load prediction
- More weight to recent observations
- Exponentially forgetting past

**Math:**
```
EWMA = α * new_value + (1-α) * old_ewma
```

**Atoms:** scale, combine

**Status:** Identified, not yet implemented (requires floating point or fixed-point)

---

## Implementation Guidelines

### Rule 1: No Dependencies

All primitives must:
- Use only `no_std` compatible code
- Not add any external crates
- Use only the 8 root atoms from `kernel_kit::atoms`
- Use only types from the existing kernel

### Rule 2: Stage Contract Compliance

Each primitive must:
- Document its stage contract
- Verify adjacent stage compatibility
- Check for hazards (ordering, invariants)
- Include trust level annotation (T0-T4)

### Rule 3: Testability

Each primitive must:
- Include unit tests (where possible)
- Define measurable currency
- Have a verification plan (T3)

### Rule 4: Documentation

Each primitive must include:
- Domain of origin
- Mechanism description
- Mathematical formulation
- Atoms used
- Trust level
- Stage contract
- Hazard analysis

---

## Verification Status

| Primitive | Domain | Status | Trust Level | T3 Verification |
|-----------|--------|--------|-------------|-----------------|
| Salience Scheduler | Cognitive Science | ✅ Implemented | T1 | ⏳ Pending |
| FIR Filter (TLB) | Signal Processing | 📋 Designed | T1 | ⏳ Pending |
| PID Controller | Control Theory | 📋 Designed | T0 | ⏳ Pending |
| Perfect Hash | Information Theory | 📋 Designed | T0 | ⏳ Pending |
| EWMA | Statistics | 📋 Designed | T0 | ⏳ Pending |

---

## Integration Plan

### Phase 1: Salience Scheduler (Current)
- ✅ Implemented
- ✅ Compiles
- ⏳ Integrate into main kernel (replace existing scheduler)
- ⏳ T3 verification (measure fairness, latency)

### Phase 2: FIR Filter for TLB
- Implement TLB entry tracking
- Add confidence scoring
- Integrate with CR3 switch path
- T3 verification (measure TLB miss rate)

### Phase 3: PID Controller
- Implement CPU throttle controller
- Add load measurement
- Integrate with timer IRQ
- T3 verification (measure stability, response time)

### Phase 4: Perfect Hash Dispatch
- Implement hash-based syscall dispatch
- Replace linear search in syscall handler
- T3 verification (measure syscall latency)

---

## Performance Expectations

| Primitive | Expected Improvement | Currency |
|-----------|---------------------|----------|
| Salience Scheduler | Better fairness | Jain's fairness index |
| FIR Filter (TLB) | Fewer TLB flushes | TLB miss count |
| PID Controller | Smoother throttling | Load variance |
| Perfect Hash | Faster dispatch | Syscall latency |
| EWMA | Better prediction | Prediction error |

---

## References

- ATOM-STACK-KERNEL-DESIGN.md: Original design document with gap analysis
- Cognitive Primitive Catalog: Itti-Koch salience model
- Signal Processing: FIR filter design
- Control Theory: PID controller tuning
- Information Theory: Perfect hash functions
