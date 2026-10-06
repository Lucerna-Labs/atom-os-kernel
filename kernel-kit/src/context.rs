use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use crate::address_space::AddressSpace;
use crate::pipe::PipeEnd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState { Ready, Running, Blocked, Terminated, Trapped }

#[derive(Debug)]
pub struct Context {
    pub rsp: u64,
    pub kernel_stack: u64,
    pub state: TaskState,
    pub id: usize,
    pub page_table_root: u64,
    /// Open descriptors: (inode number + 1, cursor); 0 marks a free slot.
    pub open_files: [(u64, usize); 16],
    pub readonly_files: u16,
    /// Why the last filesystem call returned ERROR (an `abi::FsError` code).
    pub fs_error: u64,
    /// Working directory for FS syscalls: an absolute canonical path. The
    /// default is the empty string, which MEANS "/" (a const constructor
    /// cannot allocate a String). SYS_PWD reports "/" for it.
    pub cwd: String,
    pub space: Option<AddressSpace>,
    pub kernel_stack_phys: u64,
    pub kernel_stack_pages: usize,
    pub parent: usize,
    pub wait_for: Option<usize>,
    pub sleep_until: u64,
    pub exit_code: u64,
    pub waited: bool,
    pub mailbox: VecDeque<Vec<u8>>,
    /// Packed argv (name NUL arg NUL ...), returned by SYS_ARGS.
    pub arguments: Vec<u8>,
    /// Name of the executable image, for process listings.
    pub name: String,
    /// Stable identity of the program (a hash of its name): the same program
    /// gets the same identity every time it runs, unlike its pid. 0 = unnamed.
    pub identity: u64,
    /// Standard input and output: None is the console.
    pub stdin: Option<PipeEnd>,
    pub stdout: Option<PipeEnd>,
    /// Pipes created by this process: (read end, write end), each closable.
    pub pipes: [(Option<PipeEnd>, Option<PipeEnd>); MAX_PIPES],
}

/// Pipe handles one process may hold open at once.
pub const MAX_PIPES: usize = 8;

impl Context {
    pub const fn new(id: usize, rsp: u64, kernel_stack: u64, page_table_root: u64) -> Self {
        Self { id, rsp, kernel_stack, page_table_root, state: TaskState::Ready,
            open_files: [(0, 0); 16], readonly_files: 0, fs_error: 0, cwd: String::new(), space: None,
            kernel_stack_phys: 0, kernel_stack_pages: 0, parent: 0,
            wait_for: None, sleep_until: 0, exit_code: 0, waited: false,
            mailbox: VecDeque::new(), arguments: Vec::new(), name: String::new(), identity: 0,
            stdin: None, stdout: None, pipes: [const { (None, None) }; MAX_PIPES] }
    }
    /// Names the process and derives its identity (FNV-1a of the name).
    pub fn set_name(&mut self, name: String) {
        self.identity = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        self.name = name;
    }
    pub fn set_state(&mut self, state: TaskState) { self.state = state; }

    // Only called after switching off this task's CR3 and kernel stack.
    pub fn release_resources(&mut self) {
        self.space = None;
        if self.kernel_stack_pages != 0 {
            crate::address_space::frames_free(self.kernel_stack_phys, self.kernel_stack_pages);
            self.kernel_stack_pages = 0;
            self.kernel_stack_phys = 0;
        }
        self.open_files = [(0, 0); 16];
        self.mailbox.clear();
        // Dropping the ends lets readers see end-of-input and writers see
        // a broken pipe.
        self.stdin = None;
        self.stdout = None;
        self.pipes = [const { (None, None) }; MAX_PIPES];
    }
}
