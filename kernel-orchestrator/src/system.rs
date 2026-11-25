use crate::scheduler::Scheduler;
use kernel_kit::context::TaskState;
use kernel_kit::trap::TrapFrame;
use kernel_kit::pipe::PipeEnd;

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
    /// Starts `name` with the parent's standard input and output.
    pub fn spawn_program(&mut self, parent: usize, name: &str) -> Result<usize, ()> {
        let (stdin, stdout) = self.parent_stdio(parent);
        self.spawn_inner(parent, name, kernel_kit::arguments::pack(name, &[])?, stdin, stdout)
    }
    /// Starts `name` with packed extra arguments (each NUL-terminated) and the
    /// parent's standard input and output.
    pub fn spawn_with_args(&mut self, parent: usize, name: &str, extra: &[u8]) -> Result<usize, ()> {
        let arguments = kernel_kit::arguments::pack(name, extra)?;
        let (stdin, stdout) = self.parent_stdio(parent);
        self.spawn_inner(parent, name, arguments, stdin, stdout)
    }
    /// Starts `name` with an argument string (split with the shell's quoting
    /// rules; refused when the argv would exceed its limits) and explicit
    /// standard streams.
    pub fn spawn_with(&mut self, parent: usize, name: &str, args: &str,
                      stdin: Option<PipeEnd>, stdout: Option<PipeEnd>) -> Result<usize, ()> {
        let arguments = kernel_kit::arguments::pack(name, &kernel_kit::arguments::extra_from_string(args))?;
        self.spawn_inner(parent, name, arguments, stdin, stdout)
    }
    fn parent_stdio(&self, parent: usize) -> (Option<PipeEnd>, Option<PipeEnd>) {
        self.scheduler.task(parent).map(|p| (p.stdin.clone(), p.stdout.clone())).unwrap_or((None, None))
    }
    pub fn owner_for_spawn(&self, parent: usize, pid: usize) -> usize {
        if parent == 0 || parent == 1 || self.display_owner == Some(parent) {
            pid
        } else {
            self.scheduler.task(parent).map_or(pid, |p| if p.owner != 0 { p.owner } else { parent })
        }
    }
    fn spawn_inner(&mut self, parent: usize, name: &str, arguments: alloc::vec::Vec<u8>,
                   stdin: Option<PipeEnd>, stdout: Option<PipeEnd>) -> Result<usize, ()> {
        self.scheduler.collect();
        // No has_slot() pre-gate: spawn() itself pressure-reaps
        // zombie children under a full table (the interactive OS
        // must outlive its demo fleet).
        let pid = self.next_pid;
        let next = pid.checked_add(1).ok_or(())?;
        let mut context = crate::process::create(pid, parent, name, arguments, self.kernel_root).map_err(|_| ())?;
        context.owner = self.owner_for_spawn(parent, pid);
        context.stdin = stdin;
        context.stdout = stdout;
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
    /// Returns whether `caller` is authorized to terminate `target`.
    pub fn can_kill(&self, caller: usize, target: usize) -> bool {
        let Some(victim) = self.scheduler.task(target) else { return false; };
        if caller == target
            || caller == 1
            || (self.display_owner == Some(caller) && target != 1)
            || (victim.owner == caller && caller != 0)
        {
            return true;
        }
        let mut parent = victim.parent;
        for _ in 0..crate::abi::MAX_PROCESSES {
            if parent == 0 { return false; }
            if parent == caller { return true; }
            let Some(ancestor) = self.scheduler.task(parent) else { return false; };
            parent = ancestor.parent;
        }
        false
    }
    /// Terminates `target` only when `caller` has authority over it.
    pub fn kill_from(&mut self, caller: usize, target: usize) -> Result<(), ()> {
        if !self.can_kill(caller, target) { return Err(()); }
        self.kill(target)
    }
    /// Ends another live process with KILLED_STATUS. Its resources are
    /// reclaimed by `collect` once it is not the running task; its parent can
    /// still `wait` for it.
    pub fn kill(&mut self, pid: usize) -> Result<(), ()> {
        self.terminate(pid, crate::abi::KILLED_STATUS)
    }
    pub fn terminate(&mut self, pid: usize, code: u64) -> Result<(), ()> {
        let current = self.scheduler.task(pid).ok_or(())?;
        if current.state == TaskState::Terminated { return Err(()); }
        if self.display_owner == Some(pid) { self.release_display(); }
        let current = self.scheduler.task_mut(pid).ok_or(())?;
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
        self.scheduler.has_terminated = true;
        self.wake_display_owner();
        Ok(())
    }
    /// Wakes a sleeping task (Blocked with no wait target).
    pub fn wake_task(&mut self, pid: usize) -> bool {
        if let Some(task) = self.scheduler.task_mut(pid) {
            if task.state == TaskState::Blocked && task.wait_for.is_none() {
                task.state = TaskState::Ready;
                task.sleep_until = 0;
                return true;
            }
        }
        false
    }
    /// Wakes the active desktop display owner when new input, pipe output,
    /// or process state changes arrive.
    pub fn wake_display_owner(&mut self) -> bool {
        if let Some(owner) = self.display_owner {
            self.wake_task(owner)
        } else {
            false
        }
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
            let exit_code = child.exit_code;
            self.scheduler.has_terminated = true;
            return Ok(Some(exit_code));
        }
        let caller = self.scheduler.current_task_mut().unwrap();
        caller.wait_for = Some(pid);
        caller.state = TaskState::Blocked;
        Ok(None)
    }
}
