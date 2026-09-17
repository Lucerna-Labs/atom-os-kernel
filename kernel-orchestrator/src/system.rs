use crate::scheduler::Scheduler;
use kernel_kit::context::TaskState;
use kernel_kit::trap::TrapFrame;

pub struct System {
    pub scheduler: Scheduler,
    pub kernel_root: u64,
    next_pid: usize,
}
impl System {
    pub const fn new(kernel_root: u64) -> Self {
        Self { scheduler: Scheduler::new(), kernel_root, next_pid: 1 }
    }
    pub fn spawn_program(&mut self, parent: usize, name: &str) -> Result<usize, ()> {
        self.spawn_with_args(parent, name, &[])
    }
    pub fn spawn_with_args(&mut self, parent: usize, name: &str, extra: &[u8]) -> Result<usize, ()> {
        let arguments = kernel_kit::arguments::pack(name, extra)?;
        self.scheduler.collect();
        if !self.scheduler.has_slot() { return Err(()); }
        let pid = self.next_pid;
        let next = pid.checked_add(1).ok_or(())?;
        let context = crate::process::create(pid, parent, name, arguments, self.kernel_root).map_err(|_| ())?;
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
        if let Some(pid) = self.scheduler.current_task().map(|t| t.id) { let _ = self.terminate(pid, code); }
    }
    pub fn terminate(&mut self, pid: usize, code: u64) -> Result<(), ()> {
        let current = self.scheduler.task_mut(pid).ok_or(())?;
        if current.state == TaskState::Terminated { return Err(()); }
        current.exit_code = code;
        current.state = TaskState::Terminated;
        current.wait_for = None; current.sleep_until = 0;
        let mut waited = false;
        for task in self.scheduler.tasks.iter_mut().flatten() {
            if task.id != pid && task.state == TaskState::Blocked && task.wait_for == Some(pid) {
                unsafe { (*(task.rsp as *mut TrapFrame)).rax = code; }
                task.wait_for = None;
                task.state = TaskState::Ready;
                waited = true;
            }
            if task.parent == pid { task.parent = 0; }
        }
        if waited { self.scheduler.task_mut(pid).unwrap().waited = true; }
        Ok(())
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
