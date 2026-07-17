use kernel_kit::context::{Context, TaskState};

const MAX_TASKS: usize = 16;

/// The Scheduler manages preemptive context switching by swapping hardware stack pointers (rsp).
pub struct Scheduler {
    tasks: [Option<Context>; MAX_TASKS],
    current_task: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        const INIT_NONE: Option<Context> = None;
        Self {
            tasks: [INIT_NONE; MAX_TASKS],
            current_task: 0,
        }
    }

    pub fn current_task(&self) -> Option<&Context> {
        self.tasks[self.current_task].as_ref()
    }

    pub fn current_task_mut(&mut self) -> Option<&mut Context> {
        self.tasks[self.current_task].as_mut()
    }

    /// Spawns a new task.
    pub fn spawn(&mut self, ctx: Context) -> Result<(), ()> {
        for i in 0..MAX_TASKS {
            if self.tasks[i].is_none() {
                self.tasks[i] = Some(ctx);
                return Ok(());
            }
        }
        Err(())
    }

    /// Called by the Timer Interrupt. Takes the interrupted task's stack pointer,
    /// saves it, and returns the next task's stack pointer.
    pub fn switch_context(&mut self, old_rsp: u64) -> u64 {
        // Count ALL Ready tasks (including the current one).
        let total_ready: usize = self.tasks.iter()
            .filter(|t| t.as_ref().map_or(false, |c| c.state == TaskState::Ready))
            .count();

        // Count OTHER ready tasks (excluding current).
        let other_ready: usize = self.tasks.iter()
            .enumerate()
            .filter(|(i, t)| *i != self.current_task && t.as_ref().map_or(false, |c| c.state == TaskState::Ready))
            .count();

        // The current task's state.
        let current_is_running = self.tasks.get(self.current_task)
            .and_then(|t| t.as_ref())
            .map_or(false, |c| c.state == TaskState::Running);

        // Skip the save/restore ONLY when:
        //   - current task is Running (it's the active task, not the idle loop)
        //   - no OTHER task is Ready to switch to
        // This avoids corrupting the running task's saved state via the
        // timer handler's rsp save when there's nothing to switch to.
        if current_is_running && other_ready == 0 {
            return old_rsp;
        }

        // If the current task is Running and there IS another Ready task,
        // save the current state before switching.
        if current_is_running {
            if let Some(ctx) = &mut self.tasks[self.current_task] {
                ctx.rsp = old_rsp;
                ctx.state = TaskState::Ready;
            }
        }

        // Note: if current_is_running is false (e.g. kernel idle, first
        // dispatch, or task already Ready), we don't save old_rsp — the
        // idle loop's stack is not a task context.
        if let Some(ctx) = &mut self.tasks[self.current_task] {
            if ctx.state == TaskState::Running {
                ctx.rsp = old_rsp;
                ctx.state = TaskState::Ready;
            }
        }

        // Find the next ready task (simple Round Robin)
        for i in 1..=MAX_TASKS {
            let next_idx = (self.current_task + i) % MAX_TASKS;
            if let Some(ctx) = &mut self.tasks[next_idx] {
                if ctx.state == TaskState::Ready {
                    self.current_task = next_idx;
                    ctx.state = TaskState::Running;
                    return ctx.rsp;
                }
            }
        }

        // No ready task found despite ready_count > 0 (race edge case).
        // Fall through without modifying state.
        old_rsp
    }
}
