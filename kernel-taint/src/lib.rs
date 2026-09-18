//! kernel-taint — E34, the taint layer: derived content stays data.
//!
//! Wall 4 of zero-click resistance (E32's architecture, now in the
//! atom OS). The four walls: (1) Rust memory safety — parser bugs
//! hard; (2) zero-authority construction — the lane/keeper model,
//! keys that don't exist without ceremony; (3) behavior sensing —
//! kernel-sense (spatial) + the rhythm organ (temporal); (4) THIS —
//! anything derived from untrusted input carries a TAINT mark:
//! readable, displayable, NEVER executable.
//!
//! The promotion event — the only path from Tainted to Promoted — is
//! structurally gated: it requires the caller to BE the keyboard
//! input handler context. No syscall, no network, no timer path can
//! emit it. In a real system the keyboard interrupt handler calls
//! `promote` directly on a human key event (a confirmation affordance
//! rendered by the OS). Here: the promotion syscall verifies the
//! caller is the pid registered as the input-focus context.
//!
//! Doctrine (same as every crate in this kernel): dumb mechanism,
//! zero policy. WHAT is untrusted is decided by the marking seams;
//! WHO may promote is decided by caller identity (structural). The
//! registry just remembers.
//!
//! Honest labels: v1 tracks taint at PID granularity (not per-fd —
//! the atom OS's file table is simpler than Redox's scheme model,
//! and pid-level covers the zero-click threat: the exploited process
//! is the tainted thing); the promotion caller identity is a
//! registered pid (set by the kernel at boot to the input context).

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The input-focus pid: the only context whose promotion calls are
/// honored. Set once at boot; u64::MAX = unset (nobody can promote).
static INPUT_FOCUS_PID: AtomicU64 = AtomicU64::new(u64::MAX);

/// Per-tracked-pid taint slots (fixed capacity, the house pattern).
const TRACKED: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaintState {
    /// Clean: trusted origin, or unknown (untracked IS trusted — the
    /// mark is taint, not its absence).
    Clean,
    /// Derived from untrusted input: readable, displayable, NOT
    /// executable, NOT spawnable.
    Tainted,
    /// Explicitly promoted by the human input event.
    Promoted,
}

struct Slot {
    pid: AtomicU64,
    tainted: AtomicBool,
    promoted: AtomicBool,
}

impl Slot {
    const EMPTY: Self = Self {
        pid: AtomicU64::new(u64::MAX),
        tainted: AtomicBool::new(false),
        promoted: AtomicBool::new(false),
    };
}

static SLOTS: [Slot; TRACKED] = [
    Slot::EMPTY, Slot::EMPTY, Slot::EMPTY, Slot::EMPTY,
    Slot::EMPTY, Slot::EMPTY, Slot::EMPTY, Slot::EMPTY,
    Slot::EMPTY, Slot::EMPTY, Slot::EMPTY, Slot::EMPTY,
    Slot::EMPTY, Slot::EMPTY, Slot::EMPTY, Slot::EMPTY,
];

/// Register the input-focus pid (the keyboard context). Called once
/// at boot by the kernel; the only caller that can set this.
pub fn set_input_focus(pid: u64) {
    INPUT_FOCUS_PID.store(pid, Ordering::Release);
}

fn find_or_claim(pid: u64) -> Option<usize> {
    for (index, slot) in SLOTS.iter().enumerate() {
        if slot.pid.load(Ordering::Acquire) == pid {
            return Some(index);
        }
    }
    for (index, slot) in SLOTS.iter().enumerate() {
        if slot.pid.load(Ordering::Acquire) == u64::MAX {
            if slot
                .pid
                .compare_exchange(u64::MAX, pid, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(index);
            }
        }
    }
    None // table full: the loud already carry the verdict; honest drop
}

/// Mark a pid tainted (the network/egress ingress seam).
pub fn mark_tainted(pid: u64) -> bool {
    match find_or_claim(pid) {
        Some(index) => {
            SLOTS[index].tainted.store(true, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Taint propagation: a pid spawned FROM a tainted pid inherits the
/// mark. Called at the spawn seam.
pub fn propagate_taint(from_pid: u64, to_pid: u64) -> bool {
    if state_of(from_pid) == TaintState::Tainted {
        return mark_tainted(to_pid);
    }
    false
}

/// THE HUMAN PROMOTION EVENT. The only path from Tainted to Promoted.
/// The caller MUST be the registered input-focus pid (structural
/// check — no network/scheme path matches).
pub fn promote(caller_pid: u64, target_pid: u64) -> bool {
    if caller_pid != INPUT_FOCUS_PID.load(Ordering::Acquire) {
        return false;
    }
    for slot in SLOTS.iter() {
        if slot.pid.load(Ordering::Acquire) == target_pid
            && slot.tainted.load(Ordering::Acquire)
            && !slot.promoted.load(Ordering::Acquire)
        {
            slot.promoted.store(true, Ordering::Release);
            return true;
        }
    }
    false
}

/// The execution/spawn gate: Clean and Promoted pass; Tainted is
/// refused. Unknown pids are Clean (untracked IS trusted).
pub fn gate_exec_spawn(pid: u64) -> bool {
    state_of(pid) != TaintState::Tainted
}

fn state_of(pid: u64) -> TaintState {
    for slot in SLOTS.iter() {
        if slot.pid.load(Ordering::Acquire) == pid {
            if slot.tainted.load(Ordering::Acquire) {
                return if slot.promoted.load(Ordering::Acquire) {
                    TaintState::Promoted
                } else {
                    TaintState::Tainted
                };
            }
            return TaintState::Clean;
        }
    }
    TaintState::Clean
}

/// Taint state for audit.
pub fn status(pid: u64) -> TaintState {
    state_of(pid)
}

/// Forget a dying pid (pid reuse starts clean).
pub fn forget(pid: u64) {
    for slot in SLOTS.iter() {
        if slot.pid.load(Ordering::Acquire) == pid {
            slot.pid.store(u64::MAX, Ordering::Release);
            slot.tainted.store(false, Ordering::Release);
            slot.promoted.store(false, Ordering::Release);
        }
    }
}

/// Host-test support: full reset.
#[cfg(feature = "std")]
pub fn reset() {
    INPUT_FOCUS_PID.store(u64::MAX, Ordering::Release);
    for slot in SLOTS.iter() {
        slot.pid.store(u64::MAX, Ordering::Release);
        slot.tainted.store(false, Ordering::Release);
        slot.promoted.store(false, Ordering::Release);
    }
}

#[cfg(feature = "std")]
mod tests {
    use super::*;

    #[test]
    fn gate_t1_tainted_refuses_until_input_focus_promotes() {
        reset();
        set_input_focus(1);
        assert!(mark_tainted(7));
        assert!(!gate_exec_spawn(7), "tainted must refuse exec/spawn");
        assert!(!promote(99, 7), "non-input-focus caller cannot promote");
        assert!(promote(1, 7), "input-focus promotes");
        assert!(gate_exec_spawn(7), "promoted passes");
    }

    #[test]
    fn gate_t2_unknown_pid_is_clean() {
        reset();
        assert!(gate_exec_spawn(42));
        assert_eq!(status(42), TaintState::Clean);
    }

    #[test]
    fn gate_t3_taint_propagates_to_children() {
        reset();
        mark_tainted(5);
        assert!(propagate_taint(5, 6), "child inherits");
        assert!(!gate_exec_spawn(6), "tainted child refused");
        assert!(!propagate_taint(99, 8), "clean source does not taint");
        assert!(gate_exec_spawn(8));
    }

    #[test]
    fn gate_t4_forget_cleans_pid_reuse() {
        reset();
        mark_tainted(11);
        assert!(!gate_exec_spawn(11));
        forget(11);
        assert!(gate_exec_spawn(11), "pid reuse starts clean");
    }

    #[test]
    fn gate_t5_no_input_focus_nobody_promotes() {
        reset();
        mark_tainted(3);
        assert!(!promote(0, 3), "with no input-focus registered, nobody promotes");
    }
}
