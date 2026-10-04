//! Salience + Biased Competition Scheduler - Cognitive Science Primitive
//!
//! Cross-domain primitive from COGNITIVE SCIENCE (Itti-Koch model)
//!
//! Mechanism: Combine bottom-up urgency (waited time) with top-down
//! goal templates (lock contention, priority boost) using multiplicative gain.
//! The winner is argmax(score).
//!
//! Math: score = bottom_up_urgency * top_down_boost
//!       winner = argmax(score)
//!
//! Atoms used:
//!   - scan: iterate over tasks
//!   - project: extract urgency/boost features
//!   - combine: multiply urgency * boost
//!   - order: pick maximum score (via comparison)
//!
//! Trust level: T1 (math is sound, regime fit needs T3 verification)
//!
//! This is a NO_STD compatible implementation that uses only integer arithmetic.

extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use kernel_kit::context::{Context, TaskState};

const MAX_TASKS: usize = 16;

/// Goal template for top-down boosting
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GoalTemplate {
    /// Normal operation: no special boosting
    Normal,
    /// Boost tasks holding contended locks
    LockContention,
    /// Boost I/O-bound tasks
    IoBound,
}

/// Salience Scheduler using integer arithmetic (no floating point)
///
/// Uses fixed-point arithmetic with 16.16 bit representation:
///   - Urgency: ticks waited (integer)
///   - Boost: 16.16 fixed point (1.0 = 0x10000)
///   - Score: urgency * boost (scaled)
pub struct SalienceScheduler {
    /// Tasks array - using Vec to avoid Copy requirement
    tasks: Vec<Option<Context>>,
    /// Current task index
    current_task: usize,
    /// Ticks since each task last ran (bottom-up urgency)
    waited_ticks: [u64; MAX_TASKS],
    /// Goal template for top-down boosting
    goal_template: GoalTemplate,
    /// Lock contention map: which tasks hold contended locks
    lock_contention: [bool; MAX_TASKS],
    /// I/O readiness: which tasks are ready for I/O
    io_ready: [bool; MAX_TASKS],
}

impl SalienceScheduler {
    pub fn new() -> Self {
        let mut tasks = Vec::with_capacity(MAX_TASKS);
        for _ in 0..MAX_TASKS {
            tasks.push(None);
        }
        
        Self {
            tasks,
            current_task: 0,
            waited_ticks: [0; MAX_TASKS],
            goal_template: GoalTemplate::Normal,
            lock_contention: [false; MAX_TASKS],
            io_ready: [false; MAX_TASKS],
        }
    }

    /// Set the goal template for top-down boosting
    pub fn set_goal_template(&mut self, template: GoalTemplate) {
        self.goal_template = template;
    }

    /// Mark a task as holding a contended lock
    pub fn mark_lock_contention(&mut self, task_id: usize, contended: bool) {
        if task_id < MAX_TASKS {
            self.lock_contention[task_id] = contended;
        }
    }

    /// Mark a task as I/O ready
    pub fn mark_io_ready(&mut self, task_id: usize, ready: bool) {
        if task_id < MAX_TASKS {
            self.io_ready[task_id] = ready;
        }
    }

    /// Spawn a new task
    pub fn spawn(&mut self, ctx: Context) -> Result<(), ()> {
        for i in 0..MAX_TASKS {
            if self.tasks[i].is_none() {
                self.tasks[i] = Some(ctx);
                self.waited_ticks[i] = 0;
                return Ok(());
            }
        }
        Err(())
    }

    /// Get current task
    pub fn current_task(&self) -> Option<&Context> {
        self.tasks[self.current_task].as_ref()
    }

    /// Get current task (mutable)
    pub fn current_task_mut(&mut self) -> Option<&mut Context> {
        self.tasks[self.current_task].as_mut()
    }

    /// Called on each timer tick to increment waited_ticks for non-running tasks
    pub fn tick(&mut self) {
        for i in 0..MAX_TASKS {
            if let Some(ref ctx) = self.tasks[i] {
                if ctx.state == TaskState::Ready && i != self.current_task {
                    self.waited_ticks[i] += 1;
                }
            }
        }
    }

    /// Compute boost factor for a task (16.16 fixed point)
    /// Returns: boost * 0x10000 (so 1.0 = 0x10000)
    fn compute_boost(&self, task_id: usize) -> u64 {
        match self.goal_template {
            GoalTemplate::Normal => 0x10000, // 1.0
            GoalTemplate::LockContention => {
                if self.lock_contention[task_id] {
                    0x20000 // 2.0
                } else {
                    0x10000 // 1.0
                }
            }
            GoalTemplate::IoBound => {
                if self.io_ready[task_id] {
                    0x18000 // 1.5
                } else {
                    0x10000 // 1.0
                }
            }
        }
    }

    /// Compute score for a task: score = urgency * boost
    /// Uses 16.16 fixed point arithmetic
    /// Returns: score (as u128 to avoid overflow)
    fn compute_score(&self, task_id: usize) -> u128 {
        let urgency = self.waited_ticks[task_id] as u128;
        let boost = self.compute_boost(task_id) as u128;
        
        // score = urgency * boost / 0x10000 (to scale back)
        // But we want to compare scores, so we can just use urgency * boost
        // (the division by 0x10000 is monotonic)
        urgency * boost
    }

    /// Find the task with the highest score
    /// Uses scan-like iteration with comparison
    fn find_best_task(&self) -> Option<usize> {
        let mut best_task = None;
        let mut best_score: u128 = 0;
        
        for i in 0..MAX_TASKS {
            if let Some(ref ctx) = self.tasks[i] {
                if ctx.state == TaskState::Ready && i != self.current_task {
                    let score = self.compute_score(i);
                    if score > best_score {
                        best_score = score;
                        best_task = Some(i);
                    }
                }
            }
        }
        
        best_task
    }

    /// Switch context using salience + biased competition
    pub fn switch_context(&mut self, old_rsp: u64) -> u64 {
        // Count ready tasks (excluding current)
        let ready_count = self.tasks.iter()
            .enumerate()
            .filter(|(i, t)| {
                *i != self.current_task && 
                t.as_ref().map_or(false, |c| c.state == TaskState::Ready)
            })
            .count();

        // If no other ready tasks, keep current
        if ready_count == 0 {
            return old_rsp;
        }

        // Find the best task using salience scoring
        if let Some(winner) = self.find_best_task() {
            // Save current task state
            if let Some(ctx) = &mut self.tasks[self.current_task] {
                if ctx.state == TaskState::Running {
                    ctx.rsp = old_rsp;
                    ctx.state = TaskState::Ready;
                }
            }

            // Switch to winner
            self.current_task = winner;
            if let Some(ctx) = &mut self.tasks[winner] {
                ctx.state = TaskState::Running;
                self.waited_ticks[winner] = 0; // Reset urgency
                return ctx.rsp;
            }
        }

        old_rsp
    }
}

// Stage contract verification for salience scheduler
//
// Stack: bottom_up_scan -> top_down_goal_template -> combine_multiplicative -> pick_argmax
//
// All operations use only root atoms:
//   - scan: iteration over tasks (implicit in find_best_task)
//   - project: mapping task_id to boost factor (compute_boost)
//   - combine: multiplication of urgency * boost (compute_score)
//   - order: comparison to find maximum (find_best_task)
//
// The stack is well-formed:
//   1. bottom_up_scan produces (task_id, urgency)
//   2. top_down_goal_template produces boost for each task
//   3. combine_multiplicative produces score = urgency * boost
//   4. pick_argmax selects the task with maximum score
//
// No hazards: all operations are pure functions of their inputs,
// and the ordering is correct (bottom-up before top-down).
