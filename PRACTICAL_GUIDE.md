# Practical Guide: Using Cross-Domain Primitives in Atom OS

## 🎯 Quick Start

### Adding a New Primitive to the Kernel

1. **Identify the problem** you want to solve
2. **Find matching mechanisms** in other domains (use MATH_PRIMITIVES_CATALOG.md)
3. **Verify the math** matches your problem
4. **Map to root atoms** (scan, hash, fold, project, scale, compare, combine, order)
5. **Define stage contracts** with invariants
6. **Implement** using the templates below
7. **Test** with unit tests
8. **Verify** with T3 measurements

---

## 📋 Step-by-Step: Implementing a New Primitive

### Example: Implementing a Min-Heap for Priority Scheduling

#### Step 1: Identify the Problem
**Problem:** The current scheduler uses round-robin, but we want priority-based scheduling.

**Requirement:** Need a data structure that can efficiently extract the highest-priority task.

#### Step 2: Find Matching Mechanisms
From MATH_PRIMITIVES_CATALOG.md:
- **Min-Heap** (Data Structures) - O(log n) insert, O(1) min extraction
- **Priority Queue** (Data Structures) - Abstract interface
- **Binary Heap** (Data Structures) - Tree-based implementation

#### Step 3: Verify the Math
Min-Heap properties:
- Complete binary tree
- Parent ≤ children (min-heap property)
- Operations: insert, extract-min, peek, decrease-key

This matches our requirement for priority-based task selection.

#### Step 4: Map to Root Atoms
| Operation | Root Atom | Implementation |
|-----------|-----------|----------------|
| Insert | combine | Add element to heap |
| Extract-min | order | Find and remove minimum |
| Peek | project | Get minimum without removal |
| Heapify | fold | Maintain heap property |
| Compare priorities | compare | Compare task priorities |

#### Step 5: Define Stage Contracts

```rust
/// STAGE insert
///   in_shape:    heap, task
///   in_invariant:{heap is valid}
///   op:          add task to heap, maintain heap property
///   out_shape:   heap (updated)
///   preserves:   {all existing tasks}
///   destroys:    ∅
///   introduces:  {task is in heap}

/// STAGE extract_min
///   in_shape:    heap
///   in_invariant:{heap is not empty}
///   op:          remove and return task with minimum priority
///   out_shape:   (heap (updated), task)
///   preserves:   {all tasks except minimum}
///   destroys:    {minimum task in heap}
///   introduces:  {task is returned}

/// STAGE peek
///   in_shape:    heap
///   in_invariant:{heap is not empty}
///   op:          return task with minimum priority
///   out_shape:   task
///   preserves:   {heap}
///   destroys:    ∅
///   introduces:  {task is minimum}
```

#### Step 6: Implement

```rust
//! Min-Heap - Data Structures Primitive
//!
//! Mechanism: Complete binary tree where parent ≤ children
//!
//! Math: For node i, left child = 2i+1, right child = 2i+2
//!       Parent = floor((i-1)/2)
//!
//! Atoms used:
//!   - combine: Add element to heap
//!   - order: Extract minimum
//!   - project: Peek at minimum
//!   - compare: Compare priorities
//!   - fold: Heapify (maintain heap property)
//!
//! Trust level: T0 (provably correct)

use kernel_kit::atoms::{combine, order, project, compare, fold};

const MAX_TASKS: usize = 16;

/// Min-Heap for priority scheduling
pub struct MinHeap<T: Ord> {
    data: [Option<T>; MAX_TASKS],
    size: usize,
}

impl<T: Ord> MinHeap<T> {
    pub const fn new() -> Self {
        Self {
            data: [None; MAX_TASKS],
            size: 0,
        }
    }

    /// STAGE insert: Add element to heap
    pub fn insert(&mut self, value: T) -> Result<(), ()> {
        if self.size >= MAX_TASKS {
            return Err(());
        }
        
        // Add to end
        self.data[self.size] = Some(value);
        self.size += 1;
        
        // STAGE fold: Heapify up (maintain heap property)
        self.heapify_up(self.size - 1);
        
        Ok(())
    }

    /// STAGE extract_min: Remove and return minimum
    pub fn extract_min(&mut self) -> Option<T> {
        if self.size == 0 {
            return None;
        }
        
        // Get minimum (root)
        let min = self.data[0].take();
        
        // Move last element to root
        if self.size > 1 {
            self.data[0] = self.data[self.size - 1].take();
        }
        self.size -= 1;
        
        // STAGE fold: Heapify down (maintain heap property)
        self.heapify_down(0);
        
        min
    }

    /// STAGE project: Peek at minimum
    pub fn peek(&self) -> Option<&T> {
        self.data[0].as_ref()
    }

    /// STAGE compare + combine: Heapify up
    fn heapify_up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            
            // STAGE compare: Check if heap property is violated
            if compare(&self.data[index], &self.data[parent]) {
                // STAGE combine: Swap parent and child
                self.data.swap(index, parent);
                index = parent;
            } else {
                break;
            }
        }
    }

    /// STAGE compare + combine: Heapify down
    fn heapify_down(&mut self, mut index: usize) {
        loop {
            let left = 2 * index + 1;
            let right = 2 * index + 2;
            let mut smallest = index;
            
            // STAGE compare: Find smallest among index, left, right
            if left < self.size && compare(&self.data[left], &self.data[smallest]) {
                smallest = left;
            }
            if right < self.size && compare(&self.data[right], &self.data[smallest]) {
                smallest = right;
            }
            
            if smallest != index {
                // STAGE combine: Swap
                self.data.swap(index, smallest);
                index = smallest;
            } else {
                break;
            }
        }
    }
}

/// Stage contract verification
///
/// Stack: insert -> heapify_up
///         extract_min -> heapify_down
///         peek
///
/// All stages are well-formed:
/// - insert -> heapify_up: Shape compatible, no invariant destruction
/// - extract_min -> heapify_down: Shape compatible, preserves heap property
/// - peek: Read-only, no side effects
```

#### Step 7: Test

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_min_heap_basic() {
        let mut heap = MinHeap::new();
        
        heap.insert(5).unwrap();
        heap.insert(3).unwrap();
        heap.insert(8).unwrap();
        
        assert_eq!(heap.peek(), Some(&3));
        assert_eq!(heap.extract_min(), Some(3));
        assert_eq!(heap.peek(), Some(&5));
    }

    #[test]
    fn test_min_heap_ordering() {
        let mut heap = MinHeap::new();
        
        heap.insert(10).unwrap();
        heap.insert(20).unwrap();
        heap.insert(5).unwrap();
        heap.insert(15).unwrap();
        
        assert_eq!(heap.extract_min(), Some(5));
        assert_eq!(heap.extract_min(), Some(10));
        assert_eq!(heap.extract_min(), Some(15));
        assert_eq!(heap.extract_min(), Some(20));
    }
}
```

#### Step 8: Verify with T3

```rust
// In main.rs or a benchmark module
fn benchmark_min_heap() {
    use kernel_kit::salience_scheduler::MinHeap;
    
    let mut heap = MinHeap::new();
    
    // Measure insert time
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    for i in 0..1000 {
        heap.insert(i).unwrap();
    }
    let end = unsafe { core::arch::x86_64::_rdtsc() };
    let insert_time = end - start;
    
    // Measure extract_min time
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    for _ in 0..1000 {
        heap.extract_min();
    }
    let end = unsafe { core::arch::x86_64::_rdtsc() };
    let extract_time = end - start;
    
    // Print results (to serial for CI)
    print_to_serial(&format!("Min-Heap insert: {} cycles\n", insert_time));
    print_to_serial(&format!("Min-Heap extract: {} cycles\n", extract_time));
}
```

---

## 📚 Common Patterns

### Pattern 1: Filter/Controller (State Machine)

Many primitives maintain **internal state** that is updated with each observation:

```rust
pub struct Filter {
    state: StateType,
    params: ParamsType,
}

impl Filter {
    pub fn update(&mut self, observation: ObservationType) -> OutputType {
        // STAGE 1: Transform observation
        let transformed = project(observation, |obs| /* transform */);
        
        // STAGE 2: Update state
        self.state = combine(self.state, transformed, |state, obs| /* update */);
        
        // STAGE 3: Produce output
        project(self.state, |state| /* produce output */)
    }
}
```

**Examples:** EWMA, PID Controller, FIR Filter, Kalman Filter

### Pattern 2: Map-Reduce (Fold-Based)

Many primitives can be expressed as **map followed by reduce**:

```rust
// Example: Sum of squares
let sum_of_squares = fold(data.iter(), 0, |acc, &x| {
    let squared = scale(x, x);  // Map: x -> x²
    combine(acc, squared, |a, b| a + b)  // Reduce: sum
});
```

**Examples:** Mean, Variance, Norm, Dot Product, Convolution

### Pattern 3: Priority-Based Selection (Order)

Many primitives need to **select the best** from a collection:

```rust
// Example: Find task with highest priority
let best_task = order(&tasks, |a, b| a.priority > b.priority);

// Example: Find task with lowest CPU usage
let least_used = order(&tasks, |a, b| a.cpu_usage < b.cpu_usage);
```

**Examples:** Salience Scheduler, Min-Heap, Priority Queue

### Pattern 4: Validation Pipeline (Compare)

Many primitives need to **validate inputs**:

```rust
// Example: Validate syscall number
let is_valid = compare(&syscall_num, &MAX_SYSCALL) && 
               !compare(&0, &syscall_num);

// Example: Validate pointer is in user space
let is_user = compare(&ptr, &USER_SPACE_START) &&
              !compare(&ptr, &USER_SPACE_END);
```

**Examples:** Syscall validation, Memory bounds checking, Permission checking

### Pattern 5: Transformation Pipeline (Project)

Many primitives **transform data** through multiple stages:

```rust
// Example: Virtual to physical address
let phys_addr = project(virt_addr, |vaddr| {
    vaddr - PHYS_OFFSET.load(Ordering::SeqCst)
});

// Example: Extract fields from register
let (low, mid, high) = project(addr, |a| {
    (a as u16, (a >> 16) as u16, (a >> 32) as u32)
});
```

**Examples:** Address translation, Register decomposition, Feature extraction

---

## 🎯 Primitive Selection Guide

### For Scheduling Problems

| Problem | Primitive | When to Use | Complexity |
|---------|-----------|-------------|------------|
| Simple round-robin | Round Robin | Equal priority tasks | O(1) |
| Priority-based | Priority Queue | Different priority tasks | O(log n) |
| Fairness | Salience + Biased Competition | Cognitive-inspired fairness | O(n) |
| Real-time | Earliest Deadline First | Deadline-constrained tasks | O(n) |
| Load balancing | Weighted Round Robin | Multi-core systems | O(1) |
| Adaptive | PID Controller | Dynamic workloads | O(1) per update |

### For Memory Management Problems

| Problem | Primitive | When to Use | Complexity |
|---------|-----------|-------------|------------|
| Fast allocation | Slab Allocator | Fixed-size allocations | O(1) |
| General allocation | Bump Allocator | Simple, no free | O(1) |
| Fragmentation reduction | Buddy System | Power-of-2 allocations | O(log n) |
| Cache management | LRU Cache | Temporal locality | O(1) |
| Memory prediction | EWMA | Predictive allocation | O(1) |

### For Performance Problems

| Problem | Primitive | When to Use | Complexity |
|---------|-----------|-------------|------------|
| Fast dispatch | Perfect Hash | Dense syscall numbers | O(1) |
| TLB optimization | FIR Filter | Tolerate stale entries | O(1) |
| CPU throttling | PID Controller | Adaptive frequency | O(1) |
| Load prediction | EWMA | Smoothing | O(1) |

### For Reliability Problems

| Problem | Primitive | When to Use | Complexity |
|---------|-----------|-------------|------------|
| Error detection | CRC | Storage, network | O(n) |
| Error correction | Reed-Solomon | Critical data | O(n) |
| Redundancy | Mirroring | High availability | O(1) |
| Self-healing | Checkpoint/Restore | Fault tolerance | O(n) |

---

## 🔧 Debugging Primitives

### Common Issues and Fixes

#### Issue 1: Stacking-Order Hazard
**Symptom:** Primitive works in isolation but fails when composed.

**Cause:** Stages are in the wrong order, violating invariants.

**Fix:** Check stage contracts. Ensure `in_invariant(stage_n+1)` is not destroyed by `stage_n`.

**Example:** In the slab allocator, `avalanche_tag_write` must come **after** `free_list_pop`, not before.

#### Issue 2: Invariant Violation
**Symptom:** Primitive produces incorrect results in certain cases.

**Cause:** An invariant assumed by a stage is not maintained.

**Fix:** Add explicit invariant checks or modify the primitive to maintain invariants.

**Example:** In the PID controller, the integral term can wind up. Solution: Add anti-windup.

#### Issue 3: Performance Regression
**Symptom:** Primitive is slower than expected.

**Cause:** Inefficient implementation or wrong algorithm choice.

**Fix:** Profile with `rdtsc`, analyze complexity, consider alternative primitives.

**Example:** Linear search vs hash table for syscall dispatch.

#### Issue 4: Memory Safety
**Symptom:** Primitive causes memory corruption or panics.

**Cause:** Unsafe operations or incorrect bounds checking.

**Fix:** Add bounds checks, use safe abstractions, verify with tests.

**Example:** In the Min-Heap, check bounds before accessing array elements.

#### Issue 5: Race Conditions
**Symptom:** Primitive works in single-threaded but fails with IRQs or multi-core.

**Cause:** Shared state accessed concurrently without synchronization.

**Fix:** Use `IrqSpinlock` for IRQ-crossing primitives, `Spinlock` otherwise.

**Example:** The FIR filter for TLB needs `IrqSpinlock` if accessed from timer IRQ.

---

## 📊 Verification Checklist

### Before Implementation
- [ ] Mechanism dissolve completed (names stripped, math identified)
- [ ] Matching mechanisms found in other domains
- [ ] Math verification (is the mechanism sound?)
- [ ] Atom mapping (can it be expressed with 8 root atoms?)
- [ ] Stage contracts defined (stages, shapes, invariants)
- [ ] Hazard check (stacking-order, invariant destruction)
- [ ] Trust level assigned (T0-T3)

### During Implementation
- [ ] Uses only 8 root atoms
- [ ] No external dependencies
- [ ] `no_std` compatible
- [ ] Follows Rust best practices
- [ ] Includes documentation
- [ ] Includes stage contract comments

### After Implementation
- [ ] Unit tests pass
- [ ] Compiles without warnings
- [ ] T3 verification planned
- [ ] Integration points identified

### After Integration
- [ ] T3 measurements completed
- [ ] Performance meets expectations
- [ ] No regressions in existing functionality
- [ ] Documentation updated

---

## 🎓 Best Practices

### 1. Start Simple
Begin with **T0 primitives** (provably correct) before attempting T1 or T2.

### 2. Verify Stage Contracts
Always define and verify stage contracts **before** implementation.

### 3. Use Fixed-Point Arithmetic
Avoid floating point in the kernel. Use **16.16 or 32.32 fixed-point** instead.

```rust
// 16.16 fixed-point: 1.0 = 0x10000
let one: u64 = 0x10000;
let half: u64 = 0x8000;
let two: u64 = 0x20000;

// Multiply two 16.16 numbers, result is 32.32
let product = (a as u128 * b as u128) >> 16;

// Divide two 16.16 numbers
let quotient = ((a as u128 << 16) / b as u128) as u64;
```

### 4. Prefer Integer Operations
Use integer arithmetic wherever possible for **speed and determinism**.

### 5. Document Everything
Include:
- Mechanism description
- Mathematical formulation
- Atoms used
- Stage contracts
- Trust level
- Hazard analysis

### 6. Test Thoroughly
- Unit tests for basic functionality
- Edge case tests
- T3 measurements for performance
- Integration tests

### 7. Measure Performance
Use `rdtsc` to measure cycles:

```rust
let start = unsafe { core::arch::x86_64::_rdtsc() };
// Operation to measure
let end = unsafe { core::arch::x86_64::_rdtsc() };
let cycles = end - start;
```

### 8. Verify Correctness
- Check invariants at runtime (in debug builds)
- Use assertions liberally
- Fuzz test where possible

---

## 📚 Reference: Root Atom Quick Guide

### scan
**Purpose:** Traverse collections, find elements

**Signature:**
```rust
pub fn scan<T, F, R>(data: &[T], f: F) -> Option<R>
where
    F: FnMut(&T) -> Option<R>
```

**Use cases:**
- Finding free memory frames
- Walking page tables
- Searching for tasks
- Pattern matching

**Example:**
```rust
let free_frame = scan(&frames, |&free| if free { Some(index) } else { None });
```

---

### hash
**Purpose:** Identify values, create fingerprints

**Signature:**
```rust
pub fn hash(id: usize) -> usize
```

**Use cases:**
- Syscall dispatch
- Memory integrity checks
- Unique identifiers
- Load balancing

**Example:**
```rust
let index = hash(syscall_num) % NUM_SYSCALLS;
```

---

### fold
**Purpose:** Reduce/accumulate values

**Signature:**
```rust
pub fn fold<T, S, F>(iter: impl Iterator<Item = T>, init: S, f: F) -> S
where
    F: FnMut(S, T) -> S
```

**Use cases:**
- Summing values
- Calculating averages
- EWMA calculation
- PID integral term
- Convolution

**Example:**
```rust
let sum = fold(values.iter(), 0, |acc, &v| acc + v);
```

---

### project
**Purpose:** Map/transform values

**Signature:**
```rust
pub fn project<T, U, F>(item: T, f: F) -> U
where
    F: Fn(T) -> U
```

**Use cases:**
- Address translation
- Feature extraction
- Coordinate transformation
- Type conversion

**Example:**
```rust
let phys_addr = project(virt_addr, |v| v - PHYS_OFFSET);
```

---

### scale
**Purpose:** Resize/multiply values

**Signature:**
```rust
pub fn scale(base: usize, multiplier: usize) -> usize
```

**Use cases:**
- Priority weighting
- Gain application
- Vector scaling
- Matrix operations

**Example:**
```rust
let weighted_priority = scale(priority, boost_factor);
```

---

### compare
**Purpose:** Check boundaries/conditions

**Signature:**
```rust
pub fn compare<T: PartialOrd>(a: &T, b: &T) -> bool
```

**Use cases:**
- Range validation
- Threshold checks
- Sorting
- Priority comparison

**Example:**
```rust
let is_valid = compare(&value, &MAX) && !compare(&value, &MIN);
```

---

### combine
**Purpose:** Merge/join values

**Signature:**
```rust
pub fn combine<A, B, C, F>(a: A, b: B, f: F) -> C
where
    F: Fn(A, B) -> C
```

**Use cases:**
- Combining addresses and flags
- Merging memory regions
- Vector addition
- Matrix operations

**Example:**
```rust
let pte = combine(addr, flags, |a, f| (a & !0xFFF) | (f & 0xFFF));
```

---

### order
**Purpose:** Sort/select based on priority

**Signature:**
```rust
pub fn order<T, F>(a: &T, b: &T, cmp: F) -> bool
where
    F: Fn(&T, &T) -> core::cmp::Ordering
```

**Use cases:**
- Task selection
- Priority ordering
- Finding maxima/minima
- Sorting collections

**Example:**
```rust
let best_task = tasks.iter().max_by(|a, b| order(a, b, |x, y| x.priority.cmp(&y.priority)));
```

---

## 🎯 Final Checklist for Production

Before merging a new primitive into the kernel:

1. **Design**
   - [ ] Mechanism dissolve completed
   - [ ] Stage contracts defined and verified
   - [ ] Trust level assigned
   - [ ] Hazard analysis completed

2. **Implementation**
   - [ ] Uses only 8 root atoms
   - [ ] No external dependencies
   - [ ] `no_std` compatible
   - [ ] Follows Rust best practices
   - [ ] Well documented

3. **Testing**
   - [ ] Unit tests pass
   - [ ] Edge cases covered
   - [ ] No compiler warnings
   - [ ] Compiles with `--workspace`

4. **Verification**
   - [ ] T3 measurements completed
   - [ ] Performance meets expectations
   - [ ] No regressions in existing functionality

5. **Integration**
   - [ ] Integrated into appropriate module
   - [ ] Used in at least one place
   - [ ] Documentation updated
   - [ ] Changelog updated

---

## 🏆 Success Stories

### Salience Scheduler
- **Problem:** Round-robin scheduling doesn't handle priorities well
- **Solution:** Cognitive science mechanism (salience + biased competition)
- **Result:** Priority boosting for lock holders and I/O-ready tasks
- **Status:** ✅ Implemented, compiles, tested

### Next Candidates
1. **FIR Filter for TLB** - Reduce TLB flush overhead
2. **PID Controller** - Adaptive CPU throttling
3. **Perfect Hash** - Faster syscall dispatch
4. **EWMA** - Load prediction
5. **Min-Heap** - Priority queue for scheduler

---

## 📖 Additional Resources

- **ATOM-STACK-KERNEL-DESIGN.md** - Original design document
- **NOTES.md** - Debugging notes and history
- **CROSS_DOMAIN_PRIMITIVES.md** - Design documentation for cross-domain primitives
- **MATH_PRIMITIVES_CATALOG.md** - Complete catalog of 555 primitives
- **IMPLEMENTATION_SUMMARY.md** - Summary of what was implemented

---

*"The best way to predict the future is to invent it."* - Alan Kay

*"In the Atom OS kernel, we invent the future by composing mathematical primitives from across all domains."* - Adapted
