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
    fn spawn_inner(&mut self, parent: usize, name: &str, arguments: alloc::vec::Vec<u8>,
                   stdin: Option<PipeEnd>, stdout: Option<PipeEnd>) -> Result<usize, ()> {
        self.scheduler.collect();
        // No has_slot() pre-gate: spawn() itself pressure-reaps
        // zombie children under a full table (the interactive OS
        // must outlive its demo fleet).
        let pid = self.next_pid;
        let next = pid.checked_add(1).ok_or(())?;
        let mut context = crate::process::create(pid, parent, name, arguments, self.kernel_root).map_err(|_| ())?;
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
        Ok(())
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
