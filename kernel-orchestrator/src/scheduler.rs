use kernel_kit::context::{Context, TaskState};
pub const MAX_TASKS: usize = kernel_kit::abi::MAX_PROCESSES;

/// The returned stack and selected context form one indivisible transition.
pub struct Scheduler {
    pub(crate) tasks: [Option<Context>; MAX_TASKS],
    pub(crate) current: Option<usize>,
    pub idle_rsp: u64,
    pub ticks: u64,
    pub busy_ticks: u64,
    pub idle_ticks: u64,
    pub context_switches: u64,
    pub syscalls: u64,
    pub tsc_khz: u64,
    pub has_terminated: bool,
    tsc_origin: u64,
    tsc_origin_tick: u64,
}
impl Scheduler {
    pub const fn new() -> Self {
        Self {
            tasks: [const { None }; MAX_TASKS], current: None, idle_rsp: 0, ticks: 0,
            busy_ticks: 0, idle_ticks: 0, context_switches: 0, syscalls: 0,
            tsc_khz: 0, has_terminated: false, tsc_origin: 0, tsc_origin_tick: 0,
        }
    }
    pub fn current_task(&self) -> Option<&Context> { self.current.and_then(|i| self.tasks[i].as_ref()) }
    pub fn current_task_mut(&mut self) -> Option<&mut Context> { self.current.and_then(|i| self.tasks[i].as_mut()) }
    pub fn task(&self, pid: usize) -> Option<&Context> { self.tasks.iter().flatten().find(|t| t.id == pid) }
    pub fn task_mut(&mut self, pid: usize) -> Option<&mut Context> { self.tasks.iter_mut().flatten().find(|t| t.id == pid) }
    pub fn has_slot(&self) -> bool { self.tasks.iter().any(|t| t.is_none()) }
    pub fn spawn(&mut self, ctx: Context) -> Result<(), Context> {
        let is_terminated = ctx.state == TaskState::Terminated;
        let current_slot = self.current;
        for (index, slot) in self.tasks.iter_mut().enumerate() {
            // Resource-free orphans can release their slot immediately.
            if slot.as_ref().is_some_and(|t| t.state == TaskState::Terminated && t.parent == 0
                && t.space.is_none() && t.kernel_stack_pages == 0) { *slot = None; }
            if slot.is_none() {
                *slot = Some(ctx);
                if is_terminated { self.has_terminated = true; }
                return Ok(());
            }
        }
        // Pressure reaping: an exited child whose parent never waits
        // still holds a slot; under a FULL table, the oldest such
        // zombie yields it (collect() already released its pages).
        // Honest degradation: the parent's later wait() on that pid
        // misses — the alternative is an interactive OS that cannot
        // run anything after its demo fleet retires.
        if let Some(index) = self.tasks.iter().position(|t| {
            matches!(t, Some(task) if task.state == TaskState::Terminated)
        }) {
            if Some(index) != current_slot {
                self.tasks[index] = Some(ctx);
                if is_terminated { self.has_terminated = true; }
                return Ok(());
            }
        }
        Err(ctx)
    }
    pub fn switch_context(&mut self, old_rsp: u64) -> u64 {
        let prev = self.current;
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
                    if self.current != prev { self.context_switches = self.context_switches.wrapping_add(1); }
                    return self.tasks[index].as_ref().unwrap().rsp;
                }
            }
        }
        self.current = None;
        if self.current != prev { self.context_switches = self.context_switches.wrapping_add(1); }
        assert_ne!(self.idle_rsp, 0, "idle context must be captured before running tasks");
        self.idle_rsp
    }
    pub fn timer_tick(&mut self, rsp: u64) -> u64 {
        self.ticks = self.ticks.wrapping_add(1);
        kernel_kit::io::INPUT_CLOCK.store(self.ticks as u32, core::sync::atomic::Ordering::Relaxed);
        kernel_net::heartbeat();
        if let Some(index) = self.current {
            if let Some(task) = &mut self.tasks[index] {
                self.busy_ticks = self.busy_ticks.wrapping_add(1);
                task.cpu_ticks = task.cpu_ticks.wrapping_add(1);
            } else {
                self.idle_ticks = self.idle_ticks.wrapping_add(1);
            }
        } else {
            self.idle_ticks = self.idle_ticks.wrapping_add(1);
        }
        #[cfg(target_arch = "x86_64")]
        {
            let now = unsafe { core::arch::x86_64::_rdtsc() };
            if self.tsc_origin == 0 {
                self.tsc_origin = now;
                self.tsc_origin_tick = self.ticks;
            } else {
                let dt = self.ticks.saturating_sub(self.tsc_origin_tick);
                if dt >= 5 {
                    let khz = now.saturating_sub(self.tsc_origin) / (dt * 10);
                    if khz > 0 { self.tsc_khz = khz; }
                }
            }
        }
        for task in self.tasks.iter_mut().flatten() {
            if task.state == TaskState::Blocked && task.wait_for.is_none() && task.sleep_until <= self.ticks {
                task.state = TaskState::Ready;
            }
        }
        self.switch_context(rsp)
    }
    pub fn collect(&mut self) {
        if !self.has_terminated { return; }
        let mut remaining = false;
        for index in 0..MAX_TASKS {
            let is_current = Some(index) == self.current;
            if let Some(task) = &mut self.tasks[index] {
                if task.state == TaskState::Terminated {
                    if is_current {
                        remaining = true;
                        continue;
                    }
                    task.release_resources();
                    if task.parent == 0 || task.waited {
                        self.tasks[index] = None;
                    }
                }
            }
        }
        self.has_terminated = remaining;
    }
}

impl Scheduler {
    pub fn snapshot(&self) -> ([kernel_kit::abi::ProcessInfo; MAX_TASKS], usize) {
        use kernel_kit::abi::*;
        let mut records = [ProcessInfo::EMPTY; MAX_TASKS];
        let mut count = 0;
        for task in self.tasks.iter().flatten() {
            let info = &mut records[count]; count += 1;
            info.pid = task.id as u64; info.parent = task.parent as u64;
            info.exit_code = if task.state == TaskState::Terminated { task.exit_code } else { 0 };
            info.cpu_ticks = task.cpu_ticks;
            info.mem_frames = task.space.as_ref().map_or(0, |s| s.frame_count() as u64) + task.kernel_stack_pages as u64;
            info.syscalls = task.syscalls;
            info.state = match task.state {
                TaskState::Ready => PROCESS_READY, TaskState::Running => PROCESS_RUNNING,
                TaskState::Blocked if task.wait_for.is_some() => PROCESS_WAITING,
                TaskState::Blocked => PROCESS_SLEEPING, TaskState::Terminated => PROCESS_EXITED,
                TaskState::Trapped => PROCESS_TRAPPED,
            };
            for (dst, &byte) in info.name[..63].iter_mut().zip(task.arguments.iter().take_while(|&&b| b != 0)) { *dst = byte; }
        }
        (records, count)
    }
}
