use kernel_kit::context::{Context, TaskState};
pub const MAX_TASKS: usize = 16;

/// The returned stack and selected context form one indivisible transition.
pub struct Scheduler {
    pub(crate) tasks: [Option<Context>; MAX_TASKS],
    pub(crate) current: Option<usize>,
    pub idle_rsp: u64,
    pub ticks: u64,
}
impl Scheduler {
    pub const fn new() -> Self {
        Self { tasks: [const { None }; MAX_TASKS], current: None, idle_rsp: 0, ticks: 0 }
    }
    pub fn current_task(&self) -> Option<&Context> { self.current.and_then(|i| self.tasks[i].as_ref()) }
    pub fn current_task_mut(&mut self) -> Option<&mut Context> { self.current.and_then(|i| self.tasks[i].as_mut()) }
    pub fn task(&self, pid: usize) -> Option<&Context> { self.tasks.iter().flatten().find(|t| t.id == pid) }
    pub fn task_mut(&mut self, pid: usize) -> Option<&mut Context> { self.tasks.iter_mut().flatten().find(|t| t.id == pid) }
    pub fn has_slot(&self) -> bool { self.tasks.iter().any(|t| t.is_none()) }
    pub fn spawn(&mut self, ctx: Context) -> Result<(), Context> {
        for slot in &mut self.tasks {
            // Resource-free orphans can release their slot immediately.
            if slot.as_ref().is_some_and(|t| t.state == TaskState::Terminated && t.parent == 0
                && t.space.is_none() && t.kernel_stack_pages == 0) { *slot = None; }
            if slot.is_none() { *slot = Some(ctx); return Ok(()); }
        }
        Err(ctx)
    }
    pub fn switch_context(&mut self, old_rsp: u64) -> u64 {
        let start = self.current.map_or(MAX_TASKS - 1, |i| i);
        if let Some(task) = self.current_task_mut() {
            task.rsp = old_rsp;
            if task.state == TaskState::Running { task.state = TaskState::Ready; }
        } else { self.idle_rsp = old_rsp; }
        for step in 1..=MAX_TASKS {
            let index = (start + step) % MAX_TASKS;
            if let Some(task) = &mut self.tasks[index] {
                if task.state == TaskState::Ready {
                    task.state = TaskState::Running;
                    self.current = Some(index);
                    return task.rsp;
                }
            }
        }
        self.current = None;
        assert_ne!(self.idle_rsp, 0, "idle context must be captured before running tasks");
        self.idle_rsp
    }
    pub fn timer_tick(&mut self, rsp: u64) -> u64 {
        self.ticks = self.ticks.wrapping_add(1);
        kernel_kit::io::INPUT_CLOCK.store(self.ticks as u32, core::sync::atomic::Ordering::Relaxed);
        for task in self.tasks.iter_mut().flatten() {
            if task.state == TaskState::Blocked && task.wait_for.is_none() && task.sleep_until <= self.ticks {
                task.state = TaskState::Ready;
            }
        }
        self.switch_context(rsp)
    }
    pub fn collect(&mut self) {
        for index in 0..MAX_TASKS {
            if Some(index) == self.current { continue; }
            if let Some(task) = &mut self.tasks[index] {
                if task.state == TaskState::Terminated {
                    task.release_resources();
                    if task.parent == 0 || task.waited { self.tasks[index] = None; }
                }
            }
        }
    }
}
