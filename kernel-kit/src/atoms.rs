//! The Eight Root OS Atoms
//!
//! Following the Atom Doctrine, all OS behavior decomposes into these eight atoms:
//! scan, hash, fold, project, scale, compare, combine, order.
//!
//! These are functional primitives that take inputs and return outputs, with no
//! policy decisions.

/// 1. Scan: Traverses memory or data structures (e.g., page tables, runqueues).
pub fn scan<T, F, R>(data: &[T], mut f: F) -> Option<R>
where
    F: FnMut(&T) -> Option<R>,
{
    for item in data {
        if let Some(res) = f(item) {
            return Some(res);
        }
    }
    None
}

/// 2. Hash: Identifies tasks or maps virtual to physical addresses.
pub fn hash(id: usize) -> usize {
    // A simple, fast integer hash (e.g., fxhash style or a simple mix)
    let mut x = id as u64;
    x ^= x >> 27;
    x = x.wrapping_mul(0x2545F4914F6CDD1D);
    x ^= x >> 28;
    (x ^ (x >> 31)) as usize
}

/// 3. Fold: Reduces a sequence of states or instructions into a final state.
pub fn fold<T, S, F>(iter: impl Iterator<Item = T>, init: S, f: F) -> S
where
    F: FnMut(S, T) -> S,
{
    iter.fold(init, f)
}

/// 4. Project: Maps a large context to a subset (e.g., virtual to physical addr, context to register).
pub fn project<T, U, F>(item: T, f: F) -> U
where
    F: Fn(T) -> U,
{
    f(item)
}

/// 5. Scale: Resizes blocks of memory or scaling priority.
pub fn scale(base: usize, multiplier: usize) -> usize {
    base.wrapping_mul(multiplier)
}

/// 6. Compare: Enforces boundaries and isolation (privilege checks).
pub fn compare<T: PartialOrd>(a: &T, b: &T) -> bool {
    a < b
}

/// 7. Combine: Merges a thread's saved context with the CPU, or merges two states.
pub fn combine<A, B, C, F>(a: A, b: B, f: F) -> C
where
    F: Fn(A, B) -> C,
{
    f(a, b)
}

/// 8. Order: Sorts or selects based on priority.
pub fn order<T, F>(a: &T, b: &T, cmp: F) -> bool
where
    F: Fn(&T, &T) -> core::cmp::Ordering,
{
    cmp(a, b) == core::cmp::Ordering::Less
}
