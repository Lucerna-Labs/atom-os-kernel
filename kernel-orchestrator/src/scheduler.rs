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
        // Save the old stack pointer to the current task
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

        // If no other task is ready, just return the old rsp (continue executing)
        old_rsp
    }
}
