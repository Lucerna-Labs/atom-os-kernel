use crate::scheduler::Scheduler;
use kernel_kit::memory::MemoryPool;

pub struct System {
    pub scheduler: Scheduler,
    pub memory: MemoryPool,
}

impl System {
    pub const fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
            memory: MemoryPool::new(),
        }
    }

    /// Called by the hardware timer interrupt wrapper.
    /// Passes the current physical stack pointer and returns the next one.
    pub fn schedule_tick(&mut self, current_rsp: u64) -> u64 {
        self.scheduler.switch_context(current_rsp)
    }
}
