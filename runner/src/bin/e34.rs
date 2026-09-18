//! E34 host gates — the taint layer, verified directly.

use kernel_taint::{forget, gate_exec_spawn, mark_tainted, promote, propagate_taint, reset, set_input_focus, status, TaintState};

fn main() {
    // T1: tainted refuses; only input-focus promotes.
    reset();
    set_input_focus(1);
    assert!(mark_tainted(7));
    assert!(!gate_exec_spawn(7));
    assert!(!promote(99, 7), "non-input caller rejected");
    assert!(promote(1, 7));
    assert!(gate_exec_spawn(7));
    println!("T1 PASS: tainted refuses exec/spawn; input-focus promotion clears");

    // T2: unknown pid is clean.
    assert!(gate_exec_spawn(42));
    assert_eq!(status(42), TaintState::Clean);
    println!("T2 PASS: untracked pid is clean (the mark is taint, not its absence)");

    // T3: taint propagates to children; clean doesn't spread.
    mark_tainted(5);
    assert!(propagate_taint(5, 6));
    assert!(!gate_exec_spawn(6));
    assert!(!propagate_taint(99, 8));
    assert!(gate_exec_spawn(8));
    println!("T3 PASS: children inherit taint; clean sources don't spread it");

    // T4: forget cleans pid reuse.
    mark_tainted(11);
    assert!(!gate_exec_spawn(11));
    forget(11);
    assert!(gate_exec_spawn(11));
    println!("T4 PASS: pid reuse starts clean");

    // T5: without input-focus, nobody promotes.
    reset();
    mark_tainted(3);
    assert!(!promote(0, 3));
    println!("T5 PASS: no input-focus registered — nobody can promote");

    println!("\nE34 HOST PASS: derived content stays data until the human says otherwise");
}
