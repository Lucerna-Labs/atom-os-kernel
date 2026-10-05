// TODO: unfinished draft, handed to another coder. See docs/parked/linux-programs.md.
//! kernel-linux — Linux functions composed over sealed Linux programs.
//!
//! A static Linux x86-64 executable is a sealed app: Atom OS never changes its bytes.
//! What it lacks on Atom is not code but the functions it expects from its
//! surroundings. Each one is composed here, outside both the program and Atom's core,
//! the way the shadow web and the egress cone are composed over the dispatcher:
//!
//! 1. **Image** — `parse`: where the program's segments go (its own addresses, in the
//!    lower half, clear of Atom's reserved regions) and with what permissions.
//! 2. **Startup** — `startup_stack`: the System V initial stack a Linux program reads
//!    before `main` (argc, argv, envp, the auxiliary vector).
//! 3. **Calls** — `Call::decode`: names each Linux system call (for the trace) and
//!    maps the ones composed so far onto Atom operations. The rest are `Missing`:
//!    the kernel answers ENOSYS and logs them, so running a program lists exactly the
//!    functions still to compose.
//!
//! Pure mechanism, no kernel state: the orchestrator executes the operations.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

use alloc::vec::Vec;

/// Lowest address a segment may use (the zero page stays unmapped).
pub const IMAGE_FLOOR: u64 = 0x1_0000;
/// Segments, stack and heap stay below Atom's reserved regions (0x7f00_0000_0000+).
pub const IMAGE_CEILING: u64 = 0x7e00_0000_0000;
/// Largest program image (the span of its loadable segments).
pub const IMAGE_MAX: u64 = 256 << 20;
/// The initial stack: its top, and how much is mapped below it.
pub const STACK_TOP: u64 = 0x7dff_fff0_0000;
pub const STACK_BYTES: u64 = 1 << 20;

/// Linux errno values the composed calls return (negated in rax).
pub const ENOSYS: u64 = 38;
pub const EBADF: u64 = 9;
pub const EFAULT: u64 = 14;
pub const EINVAL: u64 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// Not a 64-bit little-endian x86-64 ELF executable.
    NotElf,
    /// A position-independent or dynamically linked program (needs a loader).
    Dynamic,
    /// A segment outside the image window, overlapping another, or past the file.
    Bounds,
    /// A segment both writable and executable, or with no read permission.
    Permissions,
    /// The entry point is not inside an executable segment.
    Entry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment { pub vaddr: u64, pub offset: u64, pub filesz: u64, pub memsz: u64, pub flags: u32 }

/// A Linux executable's layout: loadable segments, entry, program headers.
#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub entry: u64,
    /// Page-aligned span of the loadable segments.
    pub start: u64,
    pub end: u64,
    pub segments: Vec<Segment>,
    /// Where the program headers sit in memory (AT_PHDR), and how many.
    pub phdr: u64,
    pub phnum: u64,
    /// The program has a thread-local storage segment (it will set the FS base).
    pub tls: bool,
}

fn u16_at(b: &[u8], at: usize) -> u16 { u16::from_le_bytes([b[at], b[at + 1]]) }
fn u32_at(b: &[u8], at: usize) -> u32 { u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) }
fn u64_at(b: &[u8], at: usize) -> u64 { u64::from_le_bytes(b[at..at + 8].try_into().unwrap()) }

const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const PT_TLS: u32 = 7;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// Validates a static Linux executable and lays out its image. Fails closed.
pub fn parse(bytes: &[u8]) -> Result<Image, ImageError> {
    if bytes.len() < 64 || &bytes[..4] != b"\x7fELF" || bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
        return Err(ImageError::NotElf);
    }
    let (kind, machine) = (u16_at(bytes, 16), u16_at(bytes, 18));
    if machine != 62 || u16_at(bytes, 54) != 56 { return Err(ImageError::NotElf); }
    if kind == 3 { return Err(ImageError::Dynamic); }
    if kind != 2 { return Err(ImageError::NotElf); }
    let (entry, phoff, phnum) = (u64_at(bytes, 24), u64_at(bytes, 32), u16_at(bytes, 56) as u64);
    let table_end = phoff.checked_add(phnum * 56).ok_or(ImageError::Bounds)?;
    if phnum == 0 || phnum > 64 || table_end > bytes.len() as u64 { return Err(ImageError::Bounds); }
    let mut segments: Vec<Segment> = Vec::new();
    let mut tls = false;
    for i in 0..phnum as usize {
        let at = phoff as usize + i * 56;
        let kind = u32_at(bytes, at);
        if kind == PT_INTERP { return Err(ImageError::Dynamic); }
        if kind == PT_TLS { tls = true; }
        if kind != PT_LOAD { continue; }
        let s = Segment { flags: u32_at(bytes, at + 4), offset: u64_at(bytes, at + 8), vaddr: u64_at(bytes, at + 16),
                          filesz: u64_at(bytes, at + 32), memsz: u64_at(bytes, at + 40) };
        let align = u64_at(bytes, at + 48);
        if s.memsz == 0 { continue; }
        let vend = s.vaddr.checked_add(s.memsz).ok_or(ImageError::Bounds)?;
        let fend = s.offset.checked_add(s.filesz).ok_or(ImageError::Bounds)?;
        if s.vaddr < IMAGE_FLOOR || vend > IMAGE_CEILING || s.filesz > s.memsz || fend > bytes.len() as u64 {
            return Err(ImageError::Bounds);
        }
        if align > 1 && (!align.is_power_of_two() || s.vaddr % align != s.offset % align) { return Err(ImageError::Bounds); }
        if s.flags & PF_R == 0 || s.flags & (PF_W | PF_X) == PF_W | PF_X { return Err(ImageError::Permissions); }
        if segments.iter().any(|o| s.vaddr < o.vaddr + o.memsz && o.vaddr < vend) { return Err(ImageError::Bounds); }
        segments.push(s);
    }
    if segments.is_empty() { return Err(ImageError::Bounds); }
    let start = segments.iter().map(|s| s.vaddr & !4095).min().unwrap();
    let end = segments.iter().map(|s| (s.vaddr + s.memsz + 4095) & !4095).max().unwrap();
    if end - start > IMAGE_MAX { return Err(ImageError::Bounds); }
    if !segments.iter().any(|s| s.flags & PF_X != 0 && entry >= s.vaddr && entry < s.vaddr + s.memsz) {
        return Err(ImageError::Entry);
    }
    let image = Image { entry, start, end, phdr: 0, phnum, tls, segments: segments.clone() };
    // A page both writable and executable is refused (segments sharing a page).
    for page in (start..end).step_by(4096) {
        if image.page_flags(page) & (PF_W | PF_X) == PF_W | PF_X { return Err(ImageError::Permissions); }
    }
    // AT_PHDR: the program headers' address, via the segment that loads them.
    let phdr = segments.iter().find(|s| phoff >= s.offset && table_end <= s.offset + s.filesz)
        .map_or(0, |s| s.vaddr + (phoff - s.offset));
    Ok(Image { phdr, ..image })
}

impl Image {
    /// Union of the permissions of the segments covering `page`.
    pub fn page_flags(&self, page: u64) -> u32 {
        self.segments.iter().filter(|s| s.vaddr < page + 4096 && s.vaddr + s.memsz > page).fold(0, |f, s| f | s.flags)
    }
    pub fn writable(flags: u32) -> bool { flags & PF_W != 0 }
    pub fn executable(flags: u32) -> bool { flags & PF_X != 0 }
}

// ---- startup -------------------------------------------------------------------

pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_ENTRY: u64 = 9;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_SECURE: u64 = 23;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;

/// The initial stack's contents, to be copied so its last byte sits just below
/// `top`, and the starting rsp (pointing at argc, 16-byte aligned).
pub struct Startup { pub bytes: Vec<u8>, pub rsp: u64 }

/// Builds the System V x86-64 initial process stack:
/// `[argc][argv...][0][envp...][0][auxv pairs...][AT_NULL 0]` then the strings
/// and 16 random bytes (AT_RANDOM). `seed` fills the random bytes.
pub fn startup_stack(image: &Image, top: u64, argv: &[&[u8]], envp: &[&[u8]], seed: [u8; 16]) -> Startup {
    // Strings and the random block go at the top; the vectors below them.
    let mut strings: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();
    for s in argv.iter().chain(envp.iter()) { off