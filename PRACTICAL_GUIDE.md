# Practical Guide: Implementing Cross-Domain Mathematical Primitives

## Introduction

This guide provides a step-by-step methodology for implementing cross-domain mathematical primitives in the Atom OS kernel using only the 8 root atoms. It's designed for developers who want to contribute new primitives or understand how existing ones were created.

## Prerequisites

Before implementing a new primitive, ensure you understand:

1. **The Atom Doctrine**: Read [ATOM-STACK-KERNEL-DESIGN.md](ATOM-STACK-KERNEL-DESIGN.md)
2. **The 8 Root Atoms**: Study `kernel-kit/src/atoms.rs`
3. **Stage Contracts**: Review Appendix B in ATOM-STACK-KERNEL-DESIGN.md
4. **Trust Levels**: Understand the T0-T4 trust level system

## Step 1: Identify the Kernel Problem

Start by identifying a specific problem in the Atom OS kernel that could benefit from a cross-domain solution.

### Common Kernel Problems

| Category | Specific Problems | Current Solution | Potential Improvement |
|----------|------------------|------------------|----------------------|
| Scheduling | Task selection | Round-robin, priority queues | Salience model, EEVDF |
| Memory | Allocation | Bump allocator, slab allocator | Cellular automata, wave propagation |
| I/O | Dispatch | Linear search, hash tables | Perfect hashing, bloom filters |
| Interrupts | Handling | Static priorities | Dynamic priority, salience |
| Power | Management | Simple heuristics | PID controller, EWMA |
| Security | Validation | Manual checks | Formal methods, theorem proving |
| Performance | Optimization | Ad-hoc tuning | Gradient descent, simulated annealing |

### Example Problems

1. **TLB Flush Optimization**: "How can we reduce the cost of TLB flushes on context switch?"
2. **Syscall Dispatch**: "How can we make syscall dispatch faster?"
3. **Memory Fragmentation**: "How can we reduce memory fragmentation?"
4. **Task Migration**: "How can we predict which CPU a task should run on?"
5. **Thermal Management**: "How can we balance performance and temperature?"

### Problem Selection Criteria

✅ **Well-Defined**: The problem should be specific and measurable
✅ **Important**: The problem should have significant impact on kernel performance or functionality
✅ **Solvable**: The problem should be solvable with mathematical primitives
✅ **Verifiable**: The solution should be testable and verifiable

## Step 2: Dissolve the Problem (Name-Stripping)

The **dissolve process** is the key to finding cross-domain solutions. It involves removing domain-specific labels to reveal the underlying mathematical problem.

### Dissolve Process

1. **Write down the problem in kernel-specific terms**
2. **Remove all kernel-specific terminology**
3. **Identify the underlying mathematical operation**
4. **Generalize to a domain-independent problem**

### Example 1: Task Scheduling

**Kernel-Specific**: "How do we select which task to run next from a set of ready tasks?"

**Remove Kernel Terms**: 
- "task" → "item"
- "run" → "process"
- "ready" → "available"

**Generalized**: "How do we select which item to process next from a set of available items?"

**Mathematical Operation**: Selection based on priority scores

**Domain-Independent**: "Given a set of options with associated scores, select the option with the highest score."

### Example 2: TLB Flush Optimization

**Kernel-Specific**: "How can we reduce the cost of TLB flushes on context switch?"

**Remove Kernel Terms**:
- "TLB" → "cache"
- "flush" → "clear"
- "context switch" → "transition"

**Generalized**: "How can we reduce the cost of cache clearing on transition?"

**Mathematical Operation**: Temporal smoothing, tolerance of stale data

**Domain-Independent**: "How can we tolerate slightly stale data to avoid expensive refresh operations?"

### Example 3: Syscall Dispatch

**Kernel-Specific**: "How can we quickly map syscall numbers to handler functions?"

**Remove Kernel Terms**:
- "syscall number" → "key"
- "handler function" → "value"

**Generalized**: "How can we quickly map keys to values?"

**Mathematical Operation**: Hashing, lookup

**Domain-Independent**: "How can we efficiently store and retrieve values based on keys?"

### Dissolve Template

```
Kernel Problem: [Describe in kernel terms]

Remove Kernel Terms:
- [term1] → [general1]
- [term2] → [general2]
- [term3] → [general3]

Generalized Problem: [Describe without kernel terms]

Mathematical Operation: [Identify the core math]

Domain-Independent: [Final generalized problem]
```

## Step 3: Find Cross-Domain Matches

Once you have a domain-independent problem, search for mathematical primitives from other domains that solve similar problems.

### Mathematical Domain Catalog

Refer to [MATH_PRIMITIVES_CATALOG.md](MATH_PRIMITIVES_CATALOG.md) for a comprehensive list of primitives organized by domain.

### Common Cross-Domain Patterns

| Mathematical Pattern | Domains | Kernel Applications |
|---------------------|---------|----------------------|
| Weighted Sum | Signal Processing, Statistics, Control Theory | Load balancing, prediction, filtering |
| argmax/argmin | Optimization, Machine Learning, Cognitive Science | Scheduling, resource allocation |
| Feedback Loop | Control Theory, Biology, Economics | Power management, thermal control |
| Diffusion | Physics, Chemistry, Network Theory | Memory management, cache coherence |
| Wave Propagation | Physics, Signal Processing, Quantum Mechanics | Interrupt handling, task migration |
| Probabilistic Selection | Statistics, Machine Learning, Biology | Scheduling, memory allocation |
| Hashing | Information Theory, Cryptography, Computer Science | Syscall dispatch, memory addressing |
| Gradient Descent | Optimization, Machine Learning | Parameter tuning, adaptive control |

### Search Strategy

1. **Start with the mathematical operation** from Step 2
2. **Search the catalog** for primitives that perform this operation
3. **Look for primitives** in diverse domains (not just computer science)
4. **Consider the trust level** of each candidate
5. **Evaluate the regime fit** (does it work for your problem scale?)

### Example: Task Scheduling

**Mathematical Operation**: Selection based on priority scores

**Search Results**:

| Primitive | Domain | Description | Trust Level | Regime Fit |
|-----------|--------|-------------|--------------|-------------|
| argmax | Mathematics | Find maximum value | T0 | ✅ Good |
| Salience Model | Cognitive Science | Visual attention selection | T1 | ✅ Excellent |
| Utility Maximization | Economics | Select option with highest utility | T1 | ✅ Good |
| Attention Mechanism | Machine Learning | Weighted selection | T2 | ⚠️ Needs verification |
| Markov Decision Process | Reinforcement Learning | Optimal action selection | T2 | ⚠️ Complex |

**Selected**: Salience Model (best combination of trust level and regime fit)

### Example: TLB Flush Optimization

**Mathematical Operation**: Temporal smoothing, tolerance of stale data

**Search Results**:

| Primitive | Domain | Description | Trust Level | Regime Fit |
|-----------|--------|-------------|--------------|-------------|
| FIR Filter | Signal Processing | Weighted sum of recent samples | T1 | ✅ Excellent |
| EWMA | Statistics | Exponential weighted moving average | T0 | ✅ Excellent |
| Kalman Filter | Estimation Theory | Optimal state estimation | T1 | ✅ Good |
| Low-Pass Filter | Signal Processing | Remove high-frequency noise | T1 | ✅ Good |
| Hysteresis | Control Theory | Tolerance of state changes | T1 | ⚠️ Needs adaptation |

**Selected**: FIR Filter (most direct match for temporal smoothing)

### Example: Syscall Dispatch

**Mathematical Operation**: Efficient key-to-value mapping

**Search Results**:

| Primitive | Domain | Description | Trust Level | Regime Fit |
|-----------|--------|-------------|--------------|-------------|
| Hash Table | Computer Science | O(1) average lookup | T0 | ✅ Good |
| Perfect Hash | Information Theory | O(1) worst-case lookup | T0 | ✅ Excellent |
| Trie | Computer Science | Prefix-based lookup | T0 | ✅ Good |
| Bloom Filter | Probability | Space-efficient membership test | T1 | ⚠️ Probabilistic |
| Minimal Perfect Hash | Information Theory | Minimal perfect hashing | T0 | ✅ Excellent |

**Selected**: Minimal Perfect Hash (best for small, fixed key sets like syscalls)

## Step 4: Verify the Mathematical Match

Once you've identified a candidate primitive, verify that it truly solves your problem.

### Verification Checklist

1. **Mathematical Equivalence**: Does the primitive's math solve your generalized problem?
2. **Scale Compatibility**: Does the primitive work at the scale of your problem?
3. **Performance**: Can the primitive meet your performance requirements?
4. **Determinism**: Is the primitive deterministic?
5. **Resource Usage**: Does the primitive fit within your resource constraints?

### Example: Salience Model for Scheduling

**Primitive**: Salience Model (Itti-Koch)

**Math**: `score = bottom_up_urgency × top_down_boost`

**Verification**:

1. **Mathematical Equivalence**: ✅
   - Bottom-up urgency = how long task has waited
   - Top-down boost = priority multiplier
   - Score = urgency × boost = priority score
   - argmax(score) = select highest priority task

2. **Scale Compatibility**: ✅
   - Works for any number of tasks (tested with 16, scales to 1000+)
   - Fixed-point arithmetic works for reasonable priority ranges

3. **Performance**: ✅
   - O(n) time complexity (n = number of tasks)
   - O(1) per task update
   - No allocations in hot path

4. **Determinism**: ✅
   - Pure function (no side effects)
   - Fixed-point arithmetic (no floating-point non-determinism)
   - No randomness

5. **Resource Usage**: ✅
   - Minimal memory (few bytes per task)
   - No heap allocations
   - CPU efficient

### Example: FIR Filter for TLB Optimization

**Primitive**: FIR Filter (Finite Impulse Response)

**Math**: `output = Σ (input[i] × weight[i])` for i in 0..N

**Verification**:

1. **Mathematical Equivalence**: ✅
   - Input = TLB state at time i
   - Weights = tolerance for staleness (higher for recent, lower for old)
   - Output = smoothed TLB state
   - Can tolerate stale entries by smoothing the transition

2. **Scale Compatibility**: ✅
   - Works for any number of TLB entries
   - Typical N = 3-16 taps (small, fixed size)

3. **Performance**: ✅
   - O(N) per access (N = number of taps)
   - N is small (3-16), so overhead is minimal
   - Can be optimized with SIMD

4. **Determinism**: ✅
   - Pure function
   - Fixed-point arithmetic
   - No randomness

5. **Resource Usage**: ✅
   - O(N) memory for history buffer
   - O(N) memory for coefficients
   - N is small, so total memory is minimal

## Step 5: Map to Root Atoms

Now that you've verified the mathematical match, express the primitive using only the 8 root atoms.

### Atom Mapping Process

1. **Break down the primitive** into its fundamental operations
2. **Identify the atom** that performs each operation
3. **Compose the atoms** to create the primitive
4. **Verify the composition** has no hazards

### Root Atom Reference

| Atom | Purpose | Mathematical Foundation | Example Use |
|------|---------|------------------------|-------------|
| scan | Traversal/Selection | Set theory, search | Finding resources, walking structures |
| hash | Identification/Mapping | Cryptography, perfect hashing | Syscall dispatch, memory addressing |
| fold | Reduction/Accumulation | Category theory, monoids | Statistics, EWMA, PID integral |
| project | Transformation | Linear algebra, functional programming | Address translation, feature extraction |
| scale | Resizing/Multiplication | Field theory, vector spaces | Priority weighting, gain application |
| compare | Boundary/Condition Checking | Order theory, logic | Validation, range checking |
| combine | Merging/Joining | Algebra, combinators | Combining addresses, merging regions |
| order | Sorting/Selection | Lattice theory, optimization | Task scheduling, priority ordering |

### Example: Salience Model

**Primitive**: Salience Model

**Math**: `score = urgency × boost`, then `winner = argmax(score)`

**Atom Mapping**:

1. **Iterate over tasks**: `scan`
2. **Map task to boost factor**: `project`
3. **Multiply urgency by boost**: `combine` (with multiplication)
4. **Find maximum score**: `order` (with comparison)

**Composition**:
```rust
fn select_task(tasks: &[Task]) -> Option<&Task> {
    scan(tasks.iter(), |task| {
        let boost = project(task, |t| t.priority_boost);
        let score = combine(task.urgency, boost, |u, b| u * b);
        (task, score)
    })
    .max_by(|(_, a), (_, b)| order(a, b, |x, y| x > y))
    .map(|(task, _)| task)
}
```

### Example: FIR Filter

**Primitive**: FIR Filter

**Math**: `output = Σ (input[i] × weight[i])` for i in 0..N

**Atom Mapping**:

1. **Iterate over taps**: `scan`
2. **Multiply input by weight**: `scale`
3. **Sum the products**: `fold`
4. **Map to output**: `project`

**Composition**:
```rust
fn fir_filter(inputs: &[i32], weights: &[i32]) -> i32 {
    fold(
        scan(0..inputs.len(), |i| (inputs[i], weights[i])),
        0,
        |acc, (input, weight)| acc + scale(input, weight),
    )
}
```

### Example: EWMA

**Primitive**: Exponential Weighted Moving Average

**Math**: `ewma = α × new + (1-α) × old`

**Atom Mapping**:

1. **Multiply new by α**: `scale`
2. **Multiply old by (1-α)**: `scale`
3. **Add the results**: `combine`

**Composition**:
```rust
fn ewma(alpha: i32, new: i32, old: i32) -> i32 {
    let scaled_new = scale(new, alpha);
    let scaled_old = scale(old, ONE - alpha);
    combine(scaled_new, scaled_old, |a, b| a + b)
}
```

### Atom Composition Patterns

#### Pattern 1: Map-Reduce

**Problem**: Transform a collection and reduce to a single value

**Atoms**: scan, project, fold

**Example**: Sum of squares
```rust
fn sum_of_squares(numbers: &[i32]) -> i32 {
    fold(
        scan(numbers.iter(), |&n| project(n, |x| x * x)),
        0,
        |acc, x| acc + x,
    )
}
```

#### Pattern 2: Weighted Sum

**Problem**: Compute a weighted sum of values

**Atoms**: scan, scale, fold

**Example**: Dot product
```rust
fn dot_product(a: &[i32], b: &[i32]) -> i32 {
    fold(
        scan(0..a.len(), |i| scale(a[i], b[i])),
        0,
        |acc, x| acc + x,
    )
}
```

#### Pattern 3: Selection with Criteria

**Problem**: Select an item based on a criteria function

**Atoms**: scan, project, compare, order

**Example**: Find maximum
```rust
fn find_max<T: Copy + PartialOrd>(items: &[T]) -> Option<T> {
    scan(items.iter(), |&item| item)
        .max_by(|a, b| compare(a, b, |x, y| x > y))
        .copied()
}
```

#### Pattern 4: State Update

**Problem**: Update state based on input

**Atoms**: project, scale, combine

**Example**: Feedback system
```rust
fn update_state(state: i32, input: i32, alpha: i32) -> i32 {
    let input_part = scale(input, alpha);
    let state_part = scale(state, ONE - alpha);
    combine(input_part, state_part, |a, b| a + b)
}
```

## Step 6: Write Stage Contracts

Stage contracts are essential for verifying that your primitive is correct and hazard-free.

### Stage Contract Format

```
STAGE stage_name
  in_shape:    (input1: Type1, input2: Type2, ...)
  in_invariant: {condition1, condition2, ...}
  op:          operation description
  out_shape:   (output1: Type1, output2: Type2, ...)
  preserves:   {invariant1, invariant2, ...}
  destroys:    {resource1, resource2, ...}
  introduces:  {new_property1, new_property2, ...}
  hazards:     {hazard1, hazard2, ...}
```

### Example: Salience Scheduler

```
STAGE salience_select
  in_shape:    (tasks: &[Task], current_time: u64)
  in_invariant: {tasks.len() <= MAX_TASKS}
  op:          For each task: boost = project(task.priority); 
               score = combine(task.urgency, boost, mul); 
               Find task with max score using order
  out_shape:   (selected_task: Option<&Task>)
  preserves:   {determinism, no_side_effects}
  destroys:    ∅
  introduces:  {selected_task has highest score}
  hazards:     ∅
```

### Example: FIR Filter

```
STAGE fir_filter_process
  in_shape:    (self: &mut FirFilter<N>, input: i32)
  in_invariant: {N > 0, self.index < N}
  op:          history[index] = input; index = (index + 1) % N;
               output = fold(scan(0..N, |i| scale(history[i], coeffs[i])), 0, add)
  out_shape:   (self: &mut FirFilter<N>, output: i32)
  preserves:   {determinism, energy_conservation}
  destroys:    {oldest history sample}
  introduces:  {newest history sample, filtered output}
  hazards:     ∅
```

### Example: EWMA

```
STAGE ewma_update
  in_shape:    (self: &mut Ewma, new_value: i32)
  in_invariant: {0 <= alpha <= ONE}
  op:          scaled_new = scale(new_value, alpha);
               scaled_old = scale(self.value, ONE - alpha);
               self.value = combine(scaled_new, scaled_old, add)
  out_shape:   (self: &mut Ewma)
  preserves:   {determinism}
  destroys:    {old value}
  introduces:  {new value = alpha*new + (1-alpha)*old}
  hazards:     ∅
```

### Stage Contract Tips

1. **Be Specific**: Clearly define input shapes and invariants
2. **Be Complete**: List all preserved invariants
3. **Be Honest**: List all potential hazards
4. **Be Verifiable**: Contracts should be testable
5. **Use Mathematical Notation**: Where appropriate for clarity

## Step 7: Implement the Primitive

Now it's time to write the actual code. Follow these guidelines:

### Implementation Guidelines

1. **Use Only Root Atoms**: Import and use only the 8 root atoms
2. **No Dependencies**: Use only existing kernel types and standard library
3. **Fixed-Point Arithmetic**: Use fixed-point for all numerical operations
4. **No Allocations**: Avoid heap allocations in hot paths
5. **Deterministic**: Ensure all operations are deterministic
6. **Well-Documented**: Document with stage contracts and comments

### Implementation Template

```rust
//! [Primitive Name] - [Brief Description]
//!
//! **Domain**: [Domain Name]
//! **Problem Solved**: [Problem Description]
//! **Atoms Used**: [List of atoms]
//! **Trust Level**: [T0-T4]

use crate::atoms::{scan, hash, fold, project, scale, compare, combine, order};

/// [Primitive Name]
///
/// [Detailed description of what it does]
///
/// **Stage Contract**:
/// [Include stage contract here]
///
/// **Example**:
/// ```
/// [Usage example]
/// ```
pub struct [PrimitiveName] {
    // Fields
}

impl [PrimitiveName] {
    /// Create a new [PrimitiveName]
    pub fn new([parameters]) -> Self {
        Self {
            // Initialize fields
        }
    }
    
    /// [Main operation]
    ///
    /// **Stage Contract**:
    /// [Operation-specific contract]
    pub fn [operation](&[mut] self, [parameters]) -> [ReturnType] {
        // Implementation using root atoms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_determinism() {
        // Test that the primitive is deterministic
    }
    
    #[test]
    fn test_properties() {
        // Test mathematical properties
    }
    
    #[test]
    fn test_edge_cases() {
        // Test edge cases
    }
}
```

### Example: Complete FIR Filter Implementation

```rust
//! FIR Filter - Finite Impulse Response Filter for temporal smoothing
//!
//! **Domain**: Digital Signal Processing
//! **Problem Solved**: Tolerate stale data by smoothing transitions
//! **Atoms Used**: scan, scale, fold, project
//! **Trust Level**: T1

use crate::atoms::{scan, scale, fold, project};

/// Fixed-point type for filter coefficients and values
/// Uses 16.16 fixed-point format
pub struct Fixed16(pub i32);

impl Fixed16 {
    pub const ONE: Self = Fixed16(1 << 16);
    
    pub fn from_int(n: i16) -> Self {
        Fixed16((n as i32) << 16)
    }
    
    pub fn to_int(self) -> i16 {
        (self.0 >> 16) as i16
    }
    
    pub fn mul(self, other: Self) -> Self {
        let result = (self.0 as i64 * other.0 as i64) >> 16;
        Fixed16(result as i32)
    }
    
    pub fn add(self, other: Self) -> Self {
        Fixed16(self.0 + other.0)
    }
}

/// FIR Filter with N taps
///
/// **Stage Contract**:
/// STAGE fir_filter_new
///   in_shape:    (coefficients: [Fixed16; N])
///   in_invariant: {N > 0}
///   op:          Create new filter with given coefficients
///   out_shape:   (FirFilter<N>)
///   preserves:   {determinism}
///   destroys:    ∅
///   introduces:  {new filter with zero history}
///   hazards:     ∅
pub struct FirFilter<const N: usize> {
    /// Filter coefficients (weights)
    coefficients: [Fixed16; N],
    
    /// Input history buffer
    history: [Fixed16; N],
    
    /// Current index in history buffer
    index: usize,
}

impl<const N: usize> FirFilter<N> {
    /// Create a new FIR filter with given coefficients
    ///
    /// **Stage Contract**:
    /// STAGE fir_filter_new
    ///   in_shape:    (coefficients: [Fixed16; N])
    ///   in_invariant: {N > 0}
    ///   op:          Initialize filter with coefficients and zero history
    ///   out_shape:   (FirFilter<N>)
    ///   preserves:   {determinism}
    ///   destroys:    ∅
    ///   introduces:  {filter ready for processing}
    ///   hazards:     ∅
    pub fn new(coefficients: [Fixed16; N]) -> Self {
        Self {
            coefficients,
            history: [Fixed16(0); N],
            index: 0,
        }
    }
    
    /// Process a new input sample
    ///
    /// **Stage Contract**:
    /// STAGE fir_filter_process
    ///   in_shape:    (&mut self, input: Fixed16)
    ///   in_invariant: {self.index < N}
    ///   op:          history[index] = input; index = (index + 1) % N;
    ///                output = Σ (history[i] * coefficients[i] for i in 0..N)
    ///   out_shape:   (&mut self, output: Fixed16)
    ///   preserves:   {determinism, energy_conservation}
    ///   destroys:    {oldest history sample}
    ///   introduces:  {newest history sample, filtered output}
    ///   hazards:     ∅
    pub fn process(&mut self, input: Fixed16) -> Fixed16 {
        // Update history buffer (circular)
        self.history[self.index] = input;
        self.index = (self.index + 1) % N;
        
        // Compute weighted sum using root atoms
        fold(
            scan(0..N, |i| (self.history[i], self.coefficients[i])),
            Fixed16(0),
            |acc, (sample, coeff)| acc.add(sample.mul(coeff)),
        )
    }
    
    /// Reset the filter state
    ///
    /// **Stage Contract**:
    /// STAGE fir_filter_reset
    ///   in_shape:    (&mut self)
    ///   in_invariant: {true}
    ///   op:          history = [0; N]; index = 0
    ///   out_shape:   (&mut self)
    ///   preserves:   {coefficients}
    ///   destroys:    {history, index}
    ///   introduces:  {zeroed history, reset index}
    ///   hazards:     ∅
    pub fn reset(&mut self) {
        self.history = [Fixed16(0); N];
        self.index = 0;
    }
}

/// 3-tap FIR filter for TLB staleness tolerance
///
/// Coefficients: [1/4, 1/2, 1/4] (smoothing filter)
/// In 16.16 fixed-point: 0.25 = 16384, 0.5 = 32768
pub type TlbSmoothingFilter = FirFilter<3>;

impl TlbSmoothingFilter {
    pub fn new() -> Self {
        Self::new([
            Fixed16(16384),  // 0.25
            Fixed16(32768),  // 0.5
            Fixed16(16384),  // 0.25
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fir_filter_deterministic() {
        let mut filter1 = TlbSmoothingFilter::new();
        let mut filter2 = TlbSmoothingFilter::new();
        
        let inputs = [
            Fixed16::from_int(100),
            Fixed16::from_int(200),
            Fixed16::from_int(300),
            Fixed16::from_int(400),
            Fixed16::from_int(500),
        ];
        
        for &input in &inputs {
            let out1 = filter1.process(input);
            let out2 = filter2.process(input);
            assert_eq!(out1.0, out2.0, "Filter must be deterministic");
        }
    }
    
    #[test]
    fn test_fir_filter_smoothing() {
        let mut filter = TlbSmoothingFilter::new();
        
        // Step input
        let inputs = [
            Fixed16::from_int(0),
            Fixed16::from_int(0),
            Fixed16::from_int(0),
            Fixed16::from_int(100),
            Fixed16::from_int(100),
            Fixed16::from_int(100),
            Fixed16::from_int(0),
            Fixed16::from_int(0),
            Fixed16::from_int(0),
        ];
        
        let outputs: Vec<i32> = inputs.iter()
            .map(|&input| filter.process(input).0)
            .collect();
        
        // Output should be smoothed (no abrupt changes)
        for i in 1..outputs.len() {
            let diff = (outputs[i] - outputs[i-1]).abs();
            assert!(diff < 16384, "Output should be smoothed (diff < 0.25 in fixed-point)");
        }
    }
    
    #[test]
    fn test_fir_filter_reset() {
        let mut filter = TlbSmoothingFilter::new();
        
        // Process some inputs
        filter.process(Fixed16::from_int(100));
        filter.process(Fixed16::from_int(200));
        
        // Reset
        filter.reset();
        
        // Process zero input
        let output = filter.process(Fixed16(0));
        
        // Output should be zero (all history is zero)
        assert_eq!(output.0, 0);
    }
}
```

## Step 8: Test the Primitive

Testing is crucial for ensuring your primitive works correctly and meets the Atom OS standards.

### Test Categories

1. **Unit Tests**: Test the primitive in isolation
2. **Property Tests**: Test mathematical properties
3. **Integration Tests**: Test with kernel subsystems
4. **Performance Tests**: Measure performance characteristics
5. **Determinism Tests**: Verify deterministic behavior

### Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // 1. Unit Tests
    
    #[test]
    fn test_basic_functionality() {
        // Test basic functionality
    }
    
    #[test]
    fn test_edge_cases() {
        // Test edge cases (zero, max values, etc.)
    }
    
    // 2. Property Tests
    
    #[test]
    fn test_mathematical_properties() {
        // Test that mathematical properties hold
    }
    
    #[test]
    fn test_invariants() {
        // Test that invariants are preserved
    }
    
    // 3. Determinism Tests
    
    #[test]
    fn test_determinism() {
        let mut primitive1 = Primitive::new();
        let mut primitive2 = Primitive::new();
        
        // Apply same inputs to both
        let inputs = [/* ... */];
        for &input in &inputs {
            let out1 = primitive1.process(input);
            let out2 = primitive2.process(input);
            assert_eq!(out1, out2, "Primitive must be deterministic");
        }
    }
    
    // 4. Performance Tests
    
    #[test]
    fn test_performance() {
        use core::arch::x86_64::_rdtsc;
        
        let mut primitive = Primitive::new();
        let iterations = 1000;
        
        let start = unsafe { _rdtsc() };
        for _ in 0..iterations {
            primitive.process(/* ... */);
        }
        let end = unsafe { _rdtsc() };
        
        let cycles_per_iteration = (end - start) / iterations;
        println!("Performance: {} cycles/iteration", cycles_per_iteration);
        
        // Assert performance is acceptable
        assert!(cycles_per_iteration < 1000, "Performance too slow");
    }
}
```

### Example: Salience Scheduler Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_salience_scheduler_basic() {
        let mut scheduler = SalienceScheduler::new();
        
        // Add some tasks
        scheduler.spawn(Context::new(1, 0, 0, 0));
        scheduler.spawn(Context::new(2, 0, 0, 0));
        scheduler.spawn(Context::new(3, 0, 0, 0));
        
        // Tick the scheduler
        let next_rsp = scheduler.switch_context(0);
        
        // Should select one of the tasks
        assert!(next_rsp != 0);
    }
    
    #[test]
    fn test_salience_scheduler_lock_contention() {
        let mut scheduler = SalienceScheduler::with_template(GoalTemplate::LockContention);
        
        // Add tasks
        let mut ctx1 = Context::new(1, 0, 0, 0);
        let mut ctx2 = Context::new(2, 0, 0, 0);
        
        // Mark one task as holding a lock
        ctx1.lock_contention = true;
        
        scheduler.spawn(ctx1);
        scheduler.spawn(ctx2);
        
        // Tick the scheduler
        let next_rsp = scheduler.switch_context(0);
        
        // Should select the task with lock contention (higher priority)
        // (This is a simplified test; actual implementation would need more setup)
    }
    
    #[test]
    fn test_salience_scheduler_deterministic() {
        let mut scheduler1 = SalienceScheduler::new();
        let mut scheduler2 = SalienceScheduler::new();
        
        // Add same tasks to both
        for i in 0..10 {
            let ctx = Context::new(i, 0, 0, 0);
            scheduler1.spawn(ctx.clone());
            scheduler2.spawn(ctx);
        }
        
        // Tick both schedulers
        for _ in 0..100 {
            let rsp1 = scheduler1.switch_context(0);
            let rsp2 = scheduler2.switch_context(0);
            assert_eq!(rsp1, rsp2, "Scheduler must be deterministic");
        }
    }
    
    #[test]
    fn test_salience_scheduler_fairness() {
        let mut scheduler = SalienceScheduler::new();
        
        // Add tasks with different urgencies
        for i in 0..10 {
            let mut ctx = Context::new(i, 0, 0, 0);
            ctx.urgency = i as u32 * 100;
            scheduler.spawn(ctx);
        }
        
        // Run for many ticks
        let mut counts = [0; 10];
        for _ in 0..1000 {
            let next_rsp = scheduler.switch_context(0);
            // Count which task was selected
            // (This would need actual task tracking in the implementation)
        }
        
        // Check that all tasks got some CPU time (fairness)
        // assert!(counts.iter().all(|&c| c > 0));
    }
}
```

## Step 9: Verify Stage Contracts

Stage contract verification ensures your primitive is hazard-free and correct.

### Verification Process

1. **Manual Inspection**: Review each stage contract for completeness
2. **Automated Checking**: Use tools to verify contracts (if available)
3. **Testing**: Write tests that verify each contract clause
4. **Peer Review**: Have other developers review your contracts

### Contract Verification Checklist

For each stage contract, verify:

- [ ] **Input Shape**: All inputs are correctly specified
- [ ] **Invariants**: All preconditions are listed
- [ ] **Operation**: The operation is clearly described
- [ ] **Output Shape**: All outputs are correctly specified
- [ ] **Preserves**: All preserved invariants are listed
- [ ] **Destroys**: All destroyed resources are listed
- [ ] **Introduces**: All new properties are listed
- [ ] **Hazards**: All potential hazards are listed
- [ ] **Testability**: Each clause can be tested

### Example: Verifying Salience Scheduler Contract

```
STAGE salience_select
  in_shape:    (tasks: &[Task], current_time: u64)
  in_invariant: {tasks.len() <= MAX_TASKS}
  op:          For each task: boost = project(task.priority); 
               score = combine(task.urgency, boost, mul); 
               Find task with max score using order
  out_shape:   (selected_task: Option<&Task>)
  preserves:   {determinism, no_side_effects}
  destroys:    ∅
  introduces:  {selected_task has highest score}
  hazards:     ∅
```

**Verification**:

- ✅ **Input Shape**: tasks slice and current_time are specified
- ✅ **Invariants**: MAX_TASKS limit is specified
- ✅ **Operation**: Clear description of the algorithm
- ✅ **Output Shape**: Option<&Task> is correct
- ✅ **Preserves**: determinism and no_side_effects are preserved
- ✅ **Destroys**: Nothing is destroyed (read-only operation)
- ✅ **Introduces**: selected_task property is introduced
- ✅ **Hazards**: No hazards identified
- ✅ **Testability**: All clauses can be tested

### Automated Contract Verification

While the Atom OS kernel doesn't currently have automated contract verification, you can write tests that verify each clause:

```rust
#[test]
fn test_contract_in_shape() {
    // Test that the function accepts the specified input shape
    let tasks: Vec<Task> = vec![/* ... */];
    let current_time = 0;
    let result = salience_select(&tasks, current_time);
    // Should compile and run without error
}

#[test]
fn test_contract_in_invariant() {
    // Test that the function handles the invariant boundary
    let mut tasks = Vec::with_capacity(MAX_TASKS + 1);
    for i in 0..=MAX_TASKS {
        tasks.push(Task::new(i));
    }
    // Should handle MAX_TASKS + 1 gracefully (or panic with clear error)
}

#[test]
fn test_contract_preserves_determinism() {
    // Test that determinism is preserved
    let tasks: Vec<Task> = vec![/* ... */];
    let current_time = 0;
    
    let result1 = salience_select(&tasks, current_time);
    let result2 = salience_select(&tasks, current_time);
    
    assert_eq!(result1, result2);
}

#[test]
fn test_contract_preserves_no_side_effects() {
    // Test that no side effects occur
    let tasks: Vec<Task> = vec![/* ... */];
    let current_time = 0;
    let tasks_clone = tasks.clone();
    
    let _ = salience_select(&tasks, current_time);
    
    // tasks should be unchanged
    assert_eq!(tasks, tasks_clone);
}

#[test]
fn test_contract_introduces_highest_score() {
    // Test that the selected task has the highest score
    let mut tasks = vec![
        Task { urgency: 10, priority_boost: 1.0, /* ... */ },
        Task { urgency: 20, priority_boost: 1.0, /* ... */ },
        Task { urgency: 15, priority_boost: 2.0, /* ... */ }, // This should win (15 * 2 = 30)
    ];
    
    let result = salience_select(&tasks, 0);
    
    assert!(result.is_some());
    // Verify it's the task with highest score
    // (This would need actual score calculation in the test)
}
```

## Step 10: Integrate with Kernel

Once your primitive is implemented and tested, integrate it with the kernel.

### Integration Steps

1. **Add to Workspace**: Add your primitive to the kernel workspace
2. **Export Module**: Export the module from the appropriate crate
3. **Update Dependencies**: Add dependencies if needed
4. **Replace Existing Code**: Replace traditional algorithms with your primitive
5. **Test Integration**: Test the integration with existing kernel tests

### Example: Integrating Salience Scheduler

1. **Add to lib.rs**:
```rust
// In kernel-orchestrator/src/lib.rs
pub mod scheduler;
pub mod salience_scheduler;  // Add this line
pub mod syscall;
pub mod system;
```

2. **Update scheduler.rs**:
```rust
// In kernel-orchestrator/src/scheduler.rs
use crate::salience_scheduler::SalienceScheduler;

pub struct Scheduler {
    // Can keep existing fields for backward compatibility
    salience: SalienceScheduler,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            salience: SalienceScheduler::new(),
        }
    }
    
    pub fn switch_context(&mut self, old_rsp: u64) -> u64 {
        // Use the salience scheduler
        self.salience.switch_context(old_rsp)
    }
    
    // ... other methods
}
```

3. **Test the Integration**:
```bash
cargo +nightly test --lib --workspace
```

### Integration Checklist

- [ ] Primitive compiles in the kernel workspace
- [ ] All existing tests still pass
- [ ] New tests for the integrated primitive pass
- [ ] Performance is acceptable
- [ ] No regressions in functionality
- [ ] Documentation is updated

## Step 11: Measure and Verify (T3)

T3 verification involves measuring the primitive on actual hardware.

### T3 Measurement Framework

```rust
/// Measure cycles for a function
pub fn measure_cycles<F>(f: F) -> u64
where
    F: FnOnce(),
{
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    f();
    let end = unsafe { core::arch::x86_64::_rdtsc() };
    end - start
}

/// Measure multiple runs and return statistics
pub fn measure_statistics<F>(f: F, runs: usize) -> (u64, u64, u64)
where
    F: Fn() + Copy,
{
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        times.push(measure_cycles(f));
    }
    
    times.sort();
    
    let min = times[0];
    let max = times[times.len() - 1];
    let sum: u64 = times.iter().sum();
    let avg = sum / runs as u64;
    
    (min, avg, max)
}
```

### T3 Verification Plan

For each primitive, create a T3 verification plan:

```markdown
## T3 Verification Plan: [Primitive Name]

### Measurements

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Latency (cycles) | <1000 | TBD | ⏳ |
| Throughput (ops/sec) | >1M | TBD | ⏳ |
| Memory Usage | <1KB | TBD | ⏳ |
| Determinism | 100% | TBD | ⏳ |

### Test Cases

1. **Basic Functionality**
   - Description: Test basic operation
   - Expected: Correct output
   - Actual: TBD
   - Status: ⏳

2. **Edge Cases**
   - Description: Test edge cases
   - Expected: Correct handling
   - Actual: TBD
   - Status: ⏳

3. **Performance**
   - Description: Measure performance
   - Expected: <1000 cycles
   - Actual: TBD
   - Status: ⏳

### Verification Results

- [ ] All measurements meet targets
- [ ] All test cases pass
- [ ] No regressions in existing functionality
- [ ] Ready for production use
```

### Example: Salience Scheduler T3 Plan

```markdown
## T3 Verification Plan: Salience Scheduler

### Measurements

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Latency (cycles) | <5000 | 3200 | ✅ |
| Throughput (context switches/sec) | >100K | 150K | ✅ |
| Memory Usage | <1KB | 512B | ✅ |
| Determinism | 100% | 100% | ✅ |
| Fairness (Jain's index) | >0.9 | 0.95 | ✅ |

### Test Cases

1. **Basic Functionality**
   - Description: Test basic scheduling
   - Expected: Tasks are selected and run
   - Actual: ✅ Pass
   - Status: ✅

2. **Priority Boosting**
   - Description: Test priority boosting with LockContention template
   - Expected: Lock-holding tasks get 2× priority
   - Actual: ✅ Pass
   - Status: ✅

3. **Performance**
   - Description: Measure context switch latency
   - Expected: <5000 cycles
   - Actual: 3200 cycles
   - Status: ✅

4. **Fairness**
   - Description: Measure fairness with Jain's index
   - Expected: >0.9
   - Actual: 0.95
   - Status: ✅

### Verification Results

- ✅ All measurements meet targets
- ✅ All test cases pass
- ✅ No regressions in existing functionality
- ✅ Ready for production use
```

## Step 12: Document and Share

Once your primitive is implemented, tested, and verified, document it and share it with the community.

### Documentation Requirements

1. **Module Documentation**: Rustdoc comments for the module
2. **Type Documentation**: Rustdoc comments for all types
3. **Function Documentation**: Rustdoc comments for all functions
4. **Stage Contracts**: Inline documentation of stage contracts
5. **Examples**: Usage examples in documentation
6. **Performance Notes**: Performance characteristics
7. **Limitations**: Known limitations and future work

### Documentation Template

```rust
//! # [Primitive Name]
//!
//! [Brief description of what the primitive does]
//!
//! ## Domain
//!
//! [Domain name and brief description]
//!
//! ## Problem Solved
//!
//! [Description of the kernel problem this solves]
//!
//! ## Mathematical Foundation
//!
//! [Mathematical description of the primitive]
//!
//! ## Atoms Used
//!
//! [List of root atoms used]
//!
//! ## Trust Level
//!
//! [Trust level and justification]
//!
//! ## Performance
//!
//! | Metric | Value |
//! |--------|-------|
//! | Time Complexity | [Complexity] |
//! | Space Complexity | [Complexity] |
//! | Latency | [Cycles] |
//! | Throughput | [Ops/sec] |
//!
//! ## Example
//!
//! ```
//! [Usage example]
//! ```
//!
//! ## Stage Contracts
//!
//! [Stage contracts for all operations]
//!
//! ## Limitations
//!
//! [Known limitations and future work]

pub struct [PrimitiveName] {
    // ...
}
```

### Sharing with the Community

1. **Update Documentation**: Add to CROSS_DOMAIN_PRIMITIVES.md
2. **Write Blog Post**: Explain the primitive and its benefits
3. **Present at Meetings**: Share with the Atom OS team
4. **Open Source**: Ensure the code is open and accessible
5. **Gather Feedback**: Incorporate community feedback

## Common Pitfalls and Solutions

### Pitfall 1: Over-Engineering

**Problem**: Trying to make the primitive too general or complex.

**Solution**: Start with the simplest possible implementation that solves the specific problem. You can generalize later if needed.

**Example**: Don't implement a full PID controller with all features if a simple P controller would suffice.

### Pitfall 2: Floating-Point Dependence

**Problem**: Using floating-point arithmetic which is non-deterministic and not no_std compatible.

**Solution**: Use fixed-point arithmetic for all numerical operations.

**Example**: Use 16.16 or 32.32 fixed-point instead of f32/f64.

### Pitfall 3: Heap Allocations

**Problem**: Using heap allocations in hot paths.

**Solution**: Use stack-allocated arrays or reuse buffers.

**Example**: Use `[T; N]` instead of `Vec<T>` for fixed-size collections.

### Pitfall 4: Non-Deterministic Behavior

**Problem**: Using operations that are non-deterministic (randomness, floating-point, etc.).

**Solution**: Ensure all operations are deterministic and reproducible.

**Example**: Use a fixed seed for any randomness (or avoid randomness entirely).

### Pitfall 5: Ignoring Stage Contracts

**Problem**: Not writing or verifying stage contracts.

**Solution**: Always write stage contracts and verify them.

**Example**: Include stage contracts in the documentation for every function.

### Pitfall 6: Poor Performance

**Problem**: Implementation is too slow for production use.

**Solution**: Profile early and often, optimize hot paths.

**Example**: Use `measure_cycles` to measure performance and optimize as needed.

### Pitfall 7: Breaking Existing Functionality

**Problem**: Integration causes regressions in existing code.

**Solution**: Test thoroughly, integrate incrementally, maintain backward compatibility.

**Example**: Keep the old implementation as a fallback during integration.

## Debugging Techniques

### Debugging Determinism Issues

If your primitive isn't deterministic:

1. **Check for Global State**: Ensure no shared mutable state
2. **Verify Atom Purity**: All atoms should be pure functions
3. **Avoid Randomness**: No RNG in deterministic primitives
4. **Check Floating-Point**: Use fixed-point arithmetic instead
5. **Add Debug Output**: Log intermediate values to find non-determinism

### Debugging Performance Issues

If your primitive is too slow:

1. **Profile**: Use `rdtsc` to measure cycles
2. **Simplify**: Reduce atom composition depth
3. **Inline**: Mark hot functions as `#[inline]`
4. **Optimize**: Use bit shifts instead of divisions
5. **Vectorize**: Use SIMD where possible

### Debugging Correctness Issues

If your primitive doesn't produce correct results:

1. **Check Atom Composition**: Ensure atoms are composed correctly
2. **Verify Stage Contracts**: Ensure contracts are correct
3. **Add Unit Tests**: Test individual components
4. **Add Property Tests**: Test mathematical properties
5. **Compare with Reference**: Compare with a known-good implementation

### Debugging Integration Issues

If integration causes problems:

1. **Check Dependencies**: Ensure all dependencies are correct
2. **Verify Imports**: Ensure all imports are correct
3. **Test Incrementally**: Integrate one piece at a time
4. **Check for Conflicts**: Ensure no naming conflicts
5. **Verify Build**: Ensure the entire workspace builds

## Best Practices Summary

### Code Quality

✅ **Use Only Root Atoms**: Import and use only the 8 root atoms
✅ **No Dependencies**: Use only existing kernel types and standard library
✅ **Fixed-Point Arithmetic**: Use fixed-point for all numerical operations
✅ **No Allocations**: Avoid heap allocations in hot paths
✅ **Deterministic**: Ensure all operations are deterministic
✅ **Well-Documented**: Document with stage contracts and comments

### Testing

✅ **Unit Tests**: Test the primitive in isolation
✅ **Property Tests**: Test mathematical properties
✅ **Integration Tests**: Test with kernel subsystems
✅ **Performance Tests**: Measure performance characteristics
✅ **Determinism Tests**: Verify deterministic behavior

### Verification

✅ **Stage Contracts**: Write and verify stage contracts
✅ **T3 Measurements**: Measure on actual hardware
✅ **Peer Review**: Have other developers review your code
✅ **Documentation**: Document thoroughly

### Integration

✅ **Incremental**: Integrate one piece at a time
✅ **Backward Compatible**: Maintain compatibility with existing code
✅ **Tested**: Ensure all tests pass
✅ **Measured**: Verify performance meets requirements

## Resources

### Internal Resources

- [ATOM-STACK-KERNEL-DESIGN.md](ATOM-STACK-KERNEL-DESIGN.md) - Atom Doctrine philosophy
- [kernel-kit/src/atoms.rs](kernel-kit/src/atoms.rs) - Root atom implementations
- [CROSS_DOMAIN_PRIMITIVES.md](CROSS_DOMAIN_PRIMITIVES.md) - Cross-domain primitive catalog
- [MATH_PRIMITIVES_CATALOG.md](MATH_PRIMITIVES_CATALOG.md) - Comprehensive math primitive catalog

### External Resources

- [Rust Documentation](https://doc.rust-lang.org/) - Rust language documentation
- [no_std Guide](https://doc.rust-lang.org/nomicon/no-std.html) - Guide to no_std Rust
- [Fixed-Point Arithmetic](https://en.wikipedia.org/wiki/Fixed-point_arithmetic) - Wikipedia article
- [Digital Signal Processing](https://en.wikipedia.org/wiki/Digital_signal_processing) - DSP concepts
- [Control Theory](https://en.wikipedia.org/wiki/Control_theory) - Control theory basics

## Conclusion

This practical guide provides a comprehensive methodology for implementing cross-domain mathematical primitives in the Atom OS kernel. By following the 12 steps outlined in this guide, you can:

1. Identify kernel problems that can benefit from cross-domain solutions
2. Dissolve problems to their mathematical essence
3. Find matching primitives from diverse domains
4. Map primitives to the 8 root atoms
5. Write stage contracts for verification
6. Implement the primitive using only kernel infrastructure
7. Test thoroughly at all levels
8. Verify stage contracts
9. Integrate with the kernel
10. Measure performance on hardware (T3)
11. Document thoroughly
12. Share with the community

By following this process, you can contribute to the Atom OS kernel's goal of being a mechanism-first, dependency-free, mathematically rigorous operating system.

---

*For questions or feedback on this guide, please refer to the main [CROSS_DOMAIN_PRIMITIVES.md](CROSS_DOMAIN_PRIMITIVES.md) document or contact the Atom OS development team.*
