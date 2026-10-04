use kernel_kit::context::{Context, TaskState};
pub const MAX_TASKS: usize = kernel_kit::abi::MAX_PROCESSES;

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
        let current_slot = self.current;
        for (index, slot) in self.tasks.iter_mut().enumerate() {
            // Resource-free orphans can release their slot immediately.
            if slot.as_ref().is_some_and(|t| t.state == TaskState::Terminated && t.parent == 0
                && t.space.is_none() && t.kernel_stack_pages == 0) { *slot = None; }
            if slot.is_none() { *slot = Some(ctx); return Ok(()); }
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
                return Ok(());
            }
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
                    // E21 spider: a pid the normality cone has
                    // condemned is never scheduled. A task that never
                    // runs cannot act — the veil dial as policy. The
                    // sensor consults only task-side state here, so
                    // this read cannot reenter the syscall tap.
                    if kernel_sense::quarantined(task.id as u64) {
                        continue;
                    }
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
        // E35: the crypt master key is derived once, at the field's
        // first heartbeat, from the timestamp counter — per-boot
        // entropy (v1 honest label: TSC seed; v2 derives from the
        // QRNG lane). RAM pages encrypted under it read as noise to
        // a cold-boot snapshot.
        if self.ticks == 1 {
            let seed = unsafe { core::arch::x86_64::_rdtsc() };
            kernel_crypt::init(seed ^ 0xE35_B007);
        }
        // E21 thermodynamic release: condemnation erodes with wall
        // time, so a false positive recovers and a real rogue is
        // re-condemned the moment it resumes.
        kernel_sense::tick();
        // E37: the wire is felt on the kernel's own heartbeat — the
        // polled NIC means packets exist when the web looks, never on
        // the device's clock.
        kernel_net::heartbeat();
        // E22 fail-dead key: passive erosion every tick, and the
        // spider's condemnation signal destroys the key immediately.
        kernel_key::tick();
        kernel_instant::tick();
        if let Some(intruder) = kernel_sense::take_intrusion_signal() {
            // E25: if the intruder IS the lane task, the lane's trust
            // dies with it — real material never travels again (the
            // lane keeps serving honey, forever labeled). This is
            // DISTRUST, not destruction: it rides the raw signal
            // outside the judge, because the lane's doctrine is
            // possession-is-proof and a single condemnation of the
            // holder ends the ceremony.
            if intruder == kernel_lane::lane_pid() {
                kernel_lane::revoke();
            }
            // E36 the judge: condemn-starve already happened (the
            // quarantined pid is never scheduled); DESTRUCTION waits
            // for certified evidence — a latched drift verdict, a
            // latched parasite seam plus a foreign-budget trace, or
            // three re-condemnation episodes. Insufficient evidence
            // abstains: the pid stays starved, the keys stay alive,
            // and the thermodynamic release still frees a false
            // positive. Patience costs safety nothing; a wrong
            // cascade costs the whole boot (one life per key).
            if kernel_sense::adjudicate(intruder) {
                // The wire has one reader (the kernel); the destruction
                // fans out to every key species: perishable, instant,
                // and the crypt layer's master key.
                kernel_key::spider_destroy();
                kernel_instant::spider_destroy();
                kernel_crypt::destroy();
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
        for index in 0..MAX_TASKS {
            if Some(index) == self.current { continue; }
            if let Some(task) = &mut self.tasks[index] {
                // E34: dying context's taint entries are forgotten.
                kernel_taint::forget(task.id as u64);
                if task.state == TaskState::Terminated {
                    task.release_resources();
                    if task.parent == 0 || task.waited { self.tasks[index] = None; }
                }
            }
        }
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
