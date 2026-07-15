
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TrapFrame {
    // Pushed by our assembly stub
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // Pushed by the CPU hardware on interrupt
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TrapFrame {
    /// Constructs a completely empty frame, useful for initialization
    pub const fn empty() -> Self {
        Self {
            r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: 0, rsi: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
            rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        }
    }

    /// Creates a TrapFrame engineered to drop the CPU into Ring 3 User-Space.
    pub const fn new_user(instruction_pointer: u64, stack_pointer: u64) -> Self {
        let mut frame = Self::empty();
        frame.rip = instruction_pointer;
        frame.rsp = stack_pointer;
        // User Code Segment is index 3 (0x18). Add 3 for RPL=3 (Ring 3) -> 0x1B
        frame.cs = 0x1B;
        // User Data Segment is index 4 (0x20). Add 3 for RPL=3 (Ring 3) -> 0x23
        frame.ss = 0x23;
        // Interrupts enabled (0x200), Reserved bit 1 set (0x2) -> 0x202
        frame.rflags = 0x202;
        frame
    }

    /// Creates a TrapFrame engineered to run a kernel thread in Ring 0.
    pub const fn new_kernel(instruction_pointer: u64, stack_pointer: u64) -> Self {
        let mut frame = Self::empty();
        frame.rip = instruction_pointer;
        frame.rsp = stack_pointer;
        // Kernel Code Segment is index 1 (0x8)
        frame.cs = 0x08;
        // Kernel Data Segment is index 2 (0x10)
        frame.ss = 0x10;
        frame.rflags = 0x202;
        frame
    }
}
