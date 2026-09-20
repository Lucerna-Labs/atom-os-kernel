//! Shared syscall numbers. The original 1..16 ABI remains available.
pub const SYS_YIELD: u64 = 1;
pub const SYS_ALLOC: u64 = 2;
pub const SYS_EXIT: u64 = 3;
pub const SYS_READ: u64 = 4;
pub const SYS_WRITE: u64 = 5;
pub const SYS_OPEN: u64 = 6;
pub const SYS_READ_FILE: u64 = 7;
pub const SYS_WRITE_FILE: u64 = 8;
pub const SYS_CLOSE: u64 = 9;
pub const SYS_LIST_DIR: u64 = 10;
pub const SYS_CLEAR: u64 = 11;
pub const SYS_TRUNCATE: u64 = 12;
pub const SYS_EXEC: u64 = 13;
pub const SYS_IPC_SEND: u64 = 15;
pub const SYS_IPC_RECV: u64 = 16;
pub const SYS_SPAWN: u64 = 21;
pub const SYS_FREE: u64 = 22;
pub const SYS_WAIT: u64 = 23;
pub const SYS_SLEEP: u64 = 24;
pub const SYS_GETPID: u64 = 25;
pub const SYS_SYNC: u64 = 26;
pub const SYS_FREE_FRAMES: u64 = 27;
pub const SYS_TASK_COUNT: u64 = 28;
pub const SYS_WRITE_BUFFER: u64 = 29;
pub const SYS_REBOOT: u64 = 30;
pub const SYS_TICKS: u64 = 31;
pub const ERROR: u64 = u64::MAX;

// Filesystem operations retain ERROR == u64::MAX; SYS_FS_ERROR explains it.
pub const SYS_REMOVE: u64 = 32;
pub const SYS_RENAME: u64 = 33;
pub const SYS_WRITE_FILE_BUFFER: u64 = 34;
pub const SYS_REPLACE_FILE: u64 = 35;
pub const SYS_FS_STAT: u64 = 36;
pub const SYS_FS_ERROR: u64 = 37;
pub const SYS_OPEN_EXISTING: u64 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum FsError {
    NotFound = 1, Exists = 2, InvalidName = 3, ReadOnly = 4, NoSpace = 5,
    FileTooLarge = 6, Memory = 7, BadHandle = 8, Io = 9, BadBuffer = 10,
    NotDir = 11, IsDir = 12, NotEmpty = 13,
}
pub fn fs_error_message(code: u64) -> &'static str {
    match code {
        0 => "no error", 1 => "file not found", 2 => "destination already exists",
        3 => "invalid filename", 4 => "read-only built-in file",
        5 => "persistent filesystem is full", 6 => "file exceeds 64 KiB",
        7 => "file memory limit reached", 8 => "invalid file descriptor",
        9 => "disk unavailable or I/O failed", 10 => "invalid user buffer",
        11 => "path component is not a directory", 12 => "path is a directory",
        13 => "directory is not empty", _ => "filesystem error",
    }
}

// Process-control ABI. Arguments are UTF-8 strings terminated by NUL; argv[0]
// is the executable name. Spawn/exec inputs contain only the extra arguments.
pub const SYS_SPAWN_ARGS: u64 = 39;
pub const SYS_EXEC_ARGS: u64 = 40;
pub const SYS_ARGS: u64 = 41;
pub const SYS_PROCESSES: u64 = 42;
pub const SYS_KILL: u64 = 43;
/// E21 shadow web: status / freeze the normality cone / query foreign budget.
pub const SYS_SENSE: u64 = 44;
/// E22 fail-dead key: sub=arg, see syscall handler (init/maintain/read/status).
pub const SYS_KEY: u64 = 45;
/// E23 instant-key: init/maintain/transform/status (reconstruction-at-use).
pub const SYS_INSTANT: u64 = 46;
/// E25 key lane: claim/deposit/handoff/log/status — the one owned way out.
pub const SYS_KEYLANE: u64 = 47;
/// E34 taint layer: sub=arg — 0=mark_tainted(pid=arg1), 1=gate(pid=arg1),
/// 2=promote(target=arg1), 3=status(pid=arg1), 4=forget(pid=arg1),
/// 5=propagate(from=arg1,to=arg2), 6=set_input_focus(self).
pub const SYS_TAINT: u64 = 48;
/// E35 crypt layer: encryption at rest with temporal scramble.
/// sub=arg — 0=encrypt_word(word=arg1) -> ct (nonce via sub 2),
/// 1=decrypt_word(ct=arg1, nonce=arg2) -> word, 2=last_nonce,
/// 3=status ((ready<<63)|nonce_counter), 4=destroy.
pub const SYS_CRYPT: u64 = 49;
/// E40 the VGA cursor: sub=arg — 0=move cursor by arg cells
/// (negative = left, clamped line-local), 1=read (col | row<<8).
pub const SYS_VGA: u64 = 54;
/// E37 the network ingress: sub=arg — 0=status, 1=recv (to RECV_BASE,
/// marks the caller tainted), 2=send(bytes=arg,len=arg1) through the
/// egress cone as an ICMP echo, 3=heartbeat now, 4=ARP bootstrap.
pub const SYS_NET: u64 = 50;
/// Directory tree navigation. MKDIR (arg = path pointer) is strict: every
/// parent must already exist (no mkdir -p semantics). CHDIR (arg = path
/// pointer) must land on a directory and updates the caller's working
/// directory, returning the new cwd length. PWD (arg = buffer pointer,
/// arg1 = capacity) copies the caller's cwd plus a trailing NUL, returning
/// the byte count written.
pub const SYS_MKDIR: u64 = 51;
pub const SYS_CHDIR: u64 = 52;
pub const SYS_PWD: u64 = 53;
pub const MAX_ARGS: usize = 16;
pub const MAX_ARG_BYTES: usize = 1024;
pub const MAX_PROCESSES: usize = 16;
pub const KILLED_STATUS: u64 = 137;
pub const PROCESS_READY: u64 = 0;
pub const PROCESS_RUNNING: u64 = 1;
pub const PROCESS_SLEEPING: u64 = 2;
pub const PROCESS_WAITING: u64 = 3;
pub const PROCESS_EXITED: u64 = 4;
pub const PROCESS_TRAPPED: u64 = 5;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ProcessInfo {
    pub pid: u64,
    pub parent: u64,
    pub state: u64,
    pub exit_code: u64,
    pub name: [u8; 64],
}
impl ProcessInfo {
    pub const EMPTY: Self = Self { pid: 0, parent: 0, state: 0, exit_code: 0, name: [0; 64] };
}

/// Read-only network Lightcone: sub 0=status(arg1); sub 1=record(index=arg1,field=arg2).
pub const SYS_LIGHTCONE: u64 = 55;
