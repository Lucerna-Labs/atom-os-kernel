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
pub const SYS_REMOVE: u64 = 32;
pub const SYS_RENAME: u64 = 33;
/// rdi = buffer, rsi = record capacity; returns records written.
pub const SYS_PROCESSES: u64 = 34;
pub const SYS_KILL: u64 = 35;
/// Process record: pid u32, parent u32, state u8, name length u8,
/// 6 reserved bytes, then up to 48 name bytes.
pub const PROCESS_RECORD_BYTES: usize = 64;
pub const PROCESS_NAME_BYTES: usize = 48;
pub const STATE_READY: u8 = 0;
pub const STATE_RUNNING: u8 = 1;
pub const STATE_BLOCKED: u8 = 2;
pub const STATE_EXITED: u8 = 3;
/// Exit status reported to the parent of a killed process.
pub const KILLED_STATUS: u64 = 137;
/// Exit status of a process that wrote to a pipe nobody reads.
pub const BROKEN_PIPE_STATUS: u64 = 141;

/// rdi = SpawnRequest pointer; returns the child's pid.
pub const SYS_SPAWN_WITH: u64 = 36;
/// rdi = buffer, rsi = capacity; copies the caller's arguments and returns
/// their full length (which may exceed the capacity).
pub const SYS_ARGS: u64 = 37;
/// Creates a pipe and returns a handle holding both of its ends.
pub const SYS_PIPE: u64 = 38;
pub const SYS_PIPE_CLOSE: u64 = 39;
/// rdi = buffer, rsi = capacity; returns bytes read, 0 at end of input, or
/// WOULD_BLOCK. On the console, Esc ends input.
pub const SYS_STDIN_READ: u64 = 40;
/// rdi = buffer, rsi = length; always writes to the console (diagnostics).
pub const SYS_CONSOLE_WRITE: u64 = 41;
/// SYS_WRITE_BUFFER writes to stdout. It returns bytes written (possibly
/// fewer than requested), WOULD_BLOCK when a pipe is full, or ERROR when no
/// reader remains. SYS_EXEC takes an optional argument string in rsi.
pub const WOULD_BLOCK: u64 = u64::MAX - 1;

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
pub const SYS_DISPLAY_OPEN: u64 = 42;
pub const SYS_DISPLAY_CLOSE: u64 = 43;
/// rdi = InputEvent buffer, rsi = capacity; returns events written.
pub const SYS_INPUT_POLL: u64 = 44;
/// Returns the real-time clock as seconds since 1970-01-01 (UTC).
pub const SYS_TIME: u64 = 45;
/// Non-zero when a display device is present (the desktop can start).
pub const SYS_DISPLAY_PRESENT: u64 = 46;

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
pub const SYS_LIST_FILES: u64 = 47;
pub const FILE_RECORD_BYTES: usize = 72;
pub const FILE_BUILTIN: u32 = 1;
pub const FILE_DIR: u32 = 2;
/// Total physical frames managed by the kernel (4 KiB each).
pub const SYS_MEMORY_TOTAL: u64 = 48;

/// rdi = pipe handle, rsi = buffer, rdx = length. Read returns bytes read,
/// 0 at end of input or WOULD_BLOCK; write returns bytes written,
/// WOULD_BLOCK when full or ERROR when no reader remains.
pub const SYS_PIPE_READ: u64 = 49;
pub const SYS_PIPE_WRITE: u64 = 50;
/// SYS_PIPE_CLOSE rsi: which end to close.
pub const PIPE_BOTH: u64 = 0;
pub const PIPE_READ_END: u64 = 1;
pub const PIPE_WRITE_END: u64 = 2;

/// Files and folders. Paths are absolute ("/docs/notes.txt"; a missing leading "/"
/// means the same), at most PATH_MAX bytes, names at most 255 bytes.
/// rdi = fd, rsi = buffer, rdx = length: bytes read at the descriptor's position
/// (0 at the end of the file).
pub const SYS_FILE_READ: u64 = 51;
/// rdi = fd, rsi = buffer, rdx = length: bytes written at the descriptor's position.
pub const SYS_FILE_WRITE: u64 = 52;
/// rdi = fd, rsi = position (clamped to the file size): returns the new position.
pub const SYS_SEEK: u64 = 53;
/// rdi = path, rsi = DirEntry to fill.
pub const SYS_STAT: u64 = 54;
/// rdi = path: creates a folder.
pub const SYS_MKDIR: u64 = 55;
/// rdi = folder path, rsi = DirEntry buffer, rdx = capacity in entries: fills up to
/// `capacity` entries and returns how many the folder holds.
pub const SYS_READ_DIR: u64 = 56;
/// rdi = FsInfo to fill.
pub const SYS_FS_INFO: u64 = 57;
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

/// Error results of the file calls above and of SYS_REMOVE, SYS_RENAME and SYS_SYNC.
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

pub const ERROR: u64 = u64::MAX;
