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
/// SYS_FS_STAT arg selects one figure: 0 user files, 1 file-count limit (0 = none),
/// 2 bytes the files need when saved, 3 bytes they may use on the disk (0 without
/// one), 4 unsaved changes (0/1), 5 saved generation, 6 file bytes held in memory,
/// 7 largest file, 8 change counter, 9 change counter at the last save, 10 disk
/// present and answering (0/1).
pub const SYS_FS_STAT: u64 = 36;
pub const SYS_FS_ERROR: u64 = 37;
pub const SYS_OPEN_EXISTING: u64 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum FsError {
    NotFound = 1, Exists = 2, InvalidName = 3, ReadOnly = 4, NoSpace = 5,
    FileTooLarge = 6, Memory = 7, BadHandle = 8, Io = 9, BadBuffer = 10,
    NotDir = 11, IsDir = 12, NotEmpty = 13, Busy = 14,
}
pub fn fs_error_message(code: u64) -> &'static str {
    match code {
        0 => "no error", 1 => "file not found", 2 => "destination already exists",
        3 => "invalid filename", 4 => "read-only built-in file",
        5 => "persistent filesystem is full", 6 => "file exceeds 1 GiB",
        7 => "file memory limit reached", 8 => "invalid file descriptor",
        9 => "disk unavailable or I/O failed", 10 => "invalid user buffer",
        11 => "path component is not a directory", 12 => "path is a directory",
        13 => "directory is not empty", 14 => "file is open",
        _ => "filesystem error",
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
/// Sub 1 (freeze) also ARMS the cone: condemned pids are starved until sub 11
/// disarms it (sub 12 reports whether it is armed).
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

/// Read-only network causal-world admission records and provenance.
pub const SYS_LIGHTCONE: u64 = 55;

/// Read-only Lightcone response page (same per-process page as network receive).
pub const LIGHTCONE_PAGE: u64 = 0x0000_7f00_0000_0000;

// ---------------------------------------------------------------------------
// Desktop, pipes and the page-backed filesystem (numbers 60 and up). These
// came from the desktop line of work, where they used 34..57; they were moved
// here when the two lines merged so the calls above keep their numbers.
// SYS_PROCESSES / SYS_KILL / SYS_MKDIR / SYS_REMOVE / SYS_RENAME above serve
// both lines.
// ---------------------------------------------------------------------------
/// Coarse process states for listings (user-rt `Process::state`).
pub const STATE_READY: u8 = 0;
pub const STATE_RUNNING: u8 = 1;
pub const STATE_BLOCKED: u8 = 2;
pub const STATE_EXITED: u8 = 3;
/// Exit status of a process that wrote to a pipe nobody reads.
pub const BROKEN_PIPE_STATUS: u64 = 141;

/// rdi = SpawnRequest pointer; returns the child's pid.
pub const SYS_SPAWN_WITH: u64 = 60;
/// rdi = buffer, rsi = capacity; copies the caller's arguments after argv[0]
/// as one space-separated string and returns its full length (which may
/// exceed the capacity). SYS_ARGS returns the packed argv instead.
pub const SYS_ARGS_STRING: u64 = 61;
/// Creates a pipe and returns a handle holding both of its ends.
pub const SYS_PIPE: u64 = 62;
pub const SYS_PIPE_CLOSE: u64 = 63;
/// rdi = buffer, rsi = capacity; returns bytes read, 0 at end of input, or
/// WOULD_BLOCK. On the console, Esc ends input.
pub const SYS_STDIN_READ: u64 = 64;
/// rdi = buffer, rsi = length; always writes to the console (diagnostics).
pub const SYS_CONSOLE_WRITE: u64 = 65;
/// SYS_WRITE_BUFFER writes to stdout. It returns bytes written (possibly
/// fewer than requested), WOULD_BLOCK when a pipe is full, BROKEN_PIPE when no
/// reader remains, or ERROR for a bad buffer or refused output. SYS_EXEC takes
/// an optional argument string in rsi.
pub const WOULD_BLOCK: u64 = u64::MAX - 1;
/// SYS_WRITE_BUFFER's result when standard output is a pipe nobody reads any more
/// (ERROR there means a bad buffer or output the egress cone refused).
pub const BROKEN_PIPE: u64 = u64::MAX - 2;

pub const ARGS_MAX: usize = 255;
/// SpawnRequest stdin/stdout values; anything else is a pipe handle.
pub const STDIO_INHERIT: u64 = u64::MAX;
pub const STDIO_CONSOLE: u64 = u64::MAX - 1;
#[repr(C)]
pub struct SpawnRequest {
    /// NUL-terminated program name.
    pub path: [u8; 64],
    /// NUL-terminated argument string.
    pub args: [u8; ARGS_MAX + 1],
    pub stdin: u64,
    pub stdout: u64,
}
/// Display and input (desktop). SYS_DISPLAY_OPEN switches to the linear
/// framebuffer, maps it into the caller at the returned address and makes the
/// caller the display owner: it alone receives keyboard and mouse input until
/// it exits or calls SYS_DISPLAY_CLOSE. rdi = DisplayInfo pointer to fill.
pub const SYS_DISPLAY_OPEN: u64 = 66;
pub const SYS_DISPLAY_CLOSE: u64 = 67;
/// rdi = InputEvent buffer, rsi = capacity; returns events written.
pub const SYS_INPUT_POLL: u64 = 68;
/// Returns the real-time clock as seconds since 1970-01-01 (UTC).
pub const SYS_TIME: u64 = 69;
/// Non-zero when a display device is present (the desktop can start).
pub const SYS_DISPLAY_PRESENT: u64 = 70;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayInfo { pub width: u32, pub height: u32, pub pitch: u32, pub bpp: u32 }

pub const INPUT_KEY: u8 = 1;
pub const INPUT_MOUSE: u8 = 2;
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_CAPS: u8 = 8;
pub const MOUSE_LEFT: u8 = 1;
pub const MOUSE_RIGHT: u8 = 2;
pub const MOUSE_MIDDLE: u8 = 4;
/// Key codes: printable ASCII and \n, 8 (backspace), 9 (tab), 27 (escape)
/// stand for themselves; other keys use these codes.
pub const KEY_UP: u16 = 0x100;
pub const KEY_DOWN: u16 = 0x101;
pub const KEY_LEFT: u16 = 0x102;
pub const KEY_RIGHT: u16 = 0x103;
pub const KEY_HOME: u16 = 0x104;
pub const KEY_END: u16 = 0x105;
pub const KEY_PAGE_UP: u16 = 0x106;
pub const KEY_PAGE_DOWN: u16 = 0x107;
pub const KEY_DELETE: u16 = 0x108;
pub const KEY_INSERT: u16 = 0x109;
/// F1..F12 are KEY_F1 + 0..11.
pub const KEY_F1: u16 = 0x110;
pub const KEY_SHIFT: u16 = 0x120;
pub const KEY_CTRL: u16 = 0x121;
pub const KEY_ALT: u16 = 0x122;
pub const KEY_CAPS_LOCK: u16 = 0x123;
pub const KEY_SUPER: u16 = 0x124;

/// One keyboard or mouse event. Mouse deltas use screen orientation
/// (positive dy is downward); wheel is positive when scrolled up. `time` is the
/// timer tick (10 ms) on which the hardware delivered the event.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputEvent {
    pub kind: u8, pub modifiers: u8, pub pressed: u8, pub buttons: u8,
    pub key: u16, pub dx: i16, pub dy: i16, pub wheel: i16,
    pub time: u32,
}

/// rdi = buffer, rsi = record capacity; copies FILE_RECORD_BYTES records
/// (name: 64 bytes NUL-padded, size: u32, flags: u32) and returns the count.
pub const SYS_LIST_FILES: u64 = 71;
pub const FILE_RECORD_BYTES: usize = 72;
pub const FILE_BUILTIN: u32 = 1;
pub const FILE_DIR: u32 = 2;
/// Total physical frames managed by the kernel (4 KiB each).
pub const SYS_MEMORY_TOTAL: u64 = 72;

/// rdi = pipe handle, rsi = buffer, rdx = length. Read returns bytes read,
/// 0 at end of input or WOULD_BLOCK; write returns bytes written,
/// WOULD_BLOCK when full or ERROR when no reader remains.
pub const SYS_PIPE_READ: u64 = 73;
pub const SYS_PIPE_WRITE: u64 = 74;
/// SYS_PIPE_CLOSE rsi: which end to close.
pub const PIPE_BOTH: u64 = 0;
pub const PIPE_READ_END: u64 = 1;
pub const PIPE_WRITE_END: u64 = 2;

/// Files and folders. Paths are absolute ("/docs/notes.txt"; a missing leading "/"
/// means the same), at most PATH_MAX bytes, names at most 255 bytes.
/// rdi = fd, rsi = buffer, rdx = length: bytes read at the descriptor's position
/// (0 at the end of the file).
pub const SYS_FILE_READ: u64 = 75;
/// rdi = fd, rsi = buffer, rdx = length: bytes written at the descriptor's position.
pub const SYS_FILE_WRITE: u64 = 76;
/// rdi = fd, rsi = position (clamped to the file size): returns the new position.
pub const SYS_SEEK: u64 = 77;
/// rdi = path, rsi = DirEntry to fill.
pub const SYS_STAT: u64 = 78;
/// rdi = folder path, rsi = DirEntry buffer, rdx = capacity in entries: fills up to
/// `capacity` entries and returns how many the folder holds.
pub const SYS_READ_DIR: u64 = 79;
/// rdi = FsInfo to fill.
pub const SYS_FS_INFO: u64 = 80;
pub const PATH_MAX: usize = 1024;
/// Largest file (1 GiB).
pub const FILE_MAX: u64 = 1 << 30;
/// Most bytes one SYS_FILE_READ / SYS_FILE_WRITE moves.
pub const FILE_IO_MAX: usize = 1 << 20;

pub const ENTRY_BUILTIN: u32 = 1;
pub const ENTRY_DIR: u32 = 2;
/// Changed since the last save to disk.
pub const ENTRY_UNSAVED: u32 = 4;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirEntry {
    pub name: [u8; 256], pub name_len: u32, pub flags: u32,
    /// Bytes for a file; number of entries for a folder.
    pub size: u64,
    /// Seconds since 1970-01-01 (UTC); 0 when unknown.
    pub modified: u64,
}
impl Default for DirEntry { fn default() -> Self { Self { name: [0; 256], name_len: 0, flags: 0, size: 0, modified: 0 } } }

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FsInfo {
    /// Data disk size, and the bytes the last save uses on it (0 without a disk).
    pub disk_bytes: u64, pub saved_bytes: u64,
    /// What all files need when saved, and the most they may need.
    pub needed_bytes: u64, pub capacity_bytes: u64,
    /// Non-zero when there are changes not yet saved; non-zero when a disk is present.
    pub unsaved: u32, pub disk: u32,
}

/// Error results of SYS_FILE_READ, SYS_FILE_WRITE, SYS_STAT and SYS_READ_DIR. The
/// original calls (SYS_OPEN, SYS_REMOVE, SYS_RENAME, SYS_MKDIR, SYS_SYNC, ...)
/// return ERROR and leave the reason in SYS_FS_ERROR; every call sets it.
pub const ERR_NOT_FOUND: u64 = u64::MAX - 16;
pub const ERR_EXISTS: u64 = u64::MAX - 17;
pub const ERR_NOT_DIR: u64 = u64::MAX - 18;
pub const ERR_IS_DIR: u64 = u64::MAX - 19;
pub const ERR_NOT_EMPTY: u64 = u64::MAX - 20;
pub const ERR_INVALID: u64 = u64::MAX - 21;
pub const ERR_READ_ONLY: u64 = u64::MAX - 22;
pub const ERR_BUSY: u64 = u64::MAX - 23;
pub const ERR_NO_SPACE: u64 = u64::MAX - 24;
pub const ERR_IO: u64 = u64::MAX - 25;

