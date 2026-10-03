use crate::scheduler::Scheduler;
use kernel_kit::context::TaskState;
use kernel_kit::trap::TrapFrame;

pub struct System {
    pub scheduler: Scheduler,
    pub kernel_root: u64,
    next_pid: usize,
    /// Process that owns the framebuffer and receives all input.
    pub display_owner: Option<usize>,
}
impl System {
    pub const fn new(kernel_root: u64) -> Self {
        Self { scheduler: Scheduler::new(), kernel_root, next_pid: 1, display_owner: None }
    }
    pub fn spawn_program(&mut self, parent: usize, name: &str) -> Result<usize, ()> {
        self.scheduler.collect();
        if !self.scheduler.has_slot() { return Err(()); }
        let pid = self.next_pid;
        let next = pid.checked_add(1).ok_or(())?;
        let context = crate::process::create(pid, parent, name, self.kernel_root).map_err(|_| ())?;
        if let Err(mut context) = self.scheduler.spawn(context) {
            context.release_resources();
            return Err(());
        }
        self.next_pid = next;
        Ok(pid)
    }
    pub fn schedule_tick(&mut self, rsp: u64) -> u64 {
        self.scheduler.collect();
        self.scheduler.timer_tick(rsp)
    }
    pub fn exit_current(&mut self, code: u64) {
        if let Some(pid) = self.scheduler.current_task().map(|task| task.id) { self.terminate(pid, code); }
    }
    /// Ends another live process. Its resources are reclaimed by `collect`
    /// once it is not the running task; its parent can still `wait` for it.
    pub fn kill(&mut self, pid: usize) -> Result<(), ()> {
        let target = self.scheduler.task(pid).ok_or(())?;
        if target.state == TaskState::Terminated { return Err(()); }
        self.terminate(pid, crate::abi::KILLED_STATUS);
        Ok(())
    }
    fn terminate(&mut self, pid: usize, code: u64) {
        if self.display_owner == Some(pid) { self.release_display(); }
        let Some(task) = self.scheduler.task_mut(pid) else { return; };
        task.exit_code = code;
        task.state = TaskState::Terminated;
        task.wait_for = None;
        let mut waited = false;
        for task in self.scheduler.tasks.iter_mut().flatten() {
            if task.id != pid && task.wait_for == Some(pid) {
                unsafe { (*(task.rsp as *mut TrapFrame)).rax = code; }
                task.wait_for = None;
                task.state = TaskState::Ready;
                waited = true;
            }
            if task.parent == pid { task.parent = 0; }
        }
        if waited { self.scheduler.task_mut(pid).unwrap().waited = true; }
    }
    /// Returns the screen to text mode and input to the console. The owner's
    /// framebuffer mapping disappears with its address space.
    pub fn release_display(&mut self) {
        if self.display_owner.take().is_some() {
            kernel_kit::display::disable();
            kernel_kit::input::clear();
        }
    }
    pub fn wait(&mut self, pid: usize) -> Result<Option<u64>, ()> {
        let parent = self.scheduler.current_task().ok_or(())?.id;
        let child = self.scheduler.task_mut(pid).ok_or(())?;
        if child.parent != parent || child.waited { return Err(()); }
        if child.state == TaskState::Terminated {
            child.waited = true;
            return Ok(Some(child.exit_code));
        }
        let caller = self.scheduler.current_task_mut().unwrap();
        caller.wait_for = Some(pid);
        caller.state = TaskState::Blocked;
        Ok(None)
    }
}
