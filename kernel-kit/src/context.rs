
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated,
    Trapped, // Task is blocked in a trap handler
}

#[derive(Debug, Clone)]
pub struct Context {
    pub rsp: u64, // Physical stack pointer to the full TrapFrame
    pub kernel_stack: u64, // The top of the kernel stack assigned to this process
    pub state: TaskState,
    pub id: usize,
    pub page_table_root: u64, // Physical address of the PageTable root (PML4)
    pub open_files: [(u64, usize); 16], // (Pointer to Vec<u8>, Cursor)
    // GAP-5 fix: Store the full saved TrapFrame, not just a pointer.
    // This ensures the complete task state (rip, rflags, all GPRs) is preserved
    // across context switches, even if the stack is modified or overwritten.
    pub saved_state: Option<super::trap::TrapFrame>,
}

impl Context {
    pub const fn new(id: usize, rsp: u64, kernel_stack: u64, page_table_root: u64) -> Self {
        Self {
            rsp,
            kernel_stack,
            state: TaskState::Ready,
            id,
            page_table_root,
            open_files: [(0, 0); 16],
            saved_state: None,
        }
    }

    pub fn set_state(&mut self, state: TaskState) {
        self.state = state;
    }
}


