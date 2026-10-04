//! The file system tree.
//!
//! Folders and names live in the kernel heap as an inode table; file contents live in
//! 4 KiB physical frames, so a file can grow to `FILE_MAX` without touching the
//! kernel's small heap. A file saved on disk is loaded on first open (see `storage`),
//! and only files changed since the last save are written by the next one.
//!
//! Paths are absolute, `/`-separated, and resolved from the root; a missing leading
//! `/` means the same (`notes.txt` is `/notes.txt`). `.` and `..` are resolved before
//! a path reaches the tree: `join` resolves a path against a process's working
//! folder (the syscall layer does this for every path argument).
use alloc::{string::String, vec::Vec};
use crate::address_space::{frames_allocate, frames_free};
use crate::memory::Spinlock;
use crate::paging::phys_to_virt;

pub const NAME_MAX: usize = 255;
pub const PATH_MAX: usize = 1024;
/// Largest file: 1 GiB.
pub const FILE_MAX: u64 = 1 << 30;
pub const PAGE: usize = 4096;
/// Data-page addresses held by one index frame.
const PER_INDEX: usize = PAGE / 8;

pub type Ino = u32;
pub const ROOT: Ino = 0;
/// Folder that holds the programs embedded in the boot image.
pub const BIN: &str = "bin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError { NotFound, Exists, NotDir, IsDir, NotEmpty, Invalid, ReadOnly, Busy, NoSpace, Io }

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= NAME_MAX && name != "." && name != ".."
        && !name.bytes().any(|b| b < 32 || b == b'/' || b == 127)
}

/// The components of an absolute path, or `Invalid`.
pub fn components(path: &str) -> Result<Vec<&str>, FsError> {
    if path.len() > PATH_MAX { return Err(FsError::Invalid); }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().all(|p| valid_name(p)) { Ok(parts) } else { Err(FsError::Invalid) }
}

/// `path` resolved against the absolute folder `base` ("" means the root): an
/// absolute path ignores `base`, `.` stays, `..` stops at the root. The result is
/// absolute and canonical ("/a/b", or "/" for the root).
pub fn join(base: &str, path: &str) -> Result<String, FsError> {
    if path.len() > PATH_MAX || base.len() > PATH_MAX { return Err(FsError::Invalid); }
    let mut parts: Vec<&str> = Vec::new();
    let start = if path.starts_with('/') { "" } else { base };
    for part in start.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." => { parts.pop(); }
            name => { if !valid_name(name) { return Err(FsError::Invalid); } parts.push(name); }
        }
    }
    let mut out = String::new();
    for part in &parts { out.push('/'); out.push_str(part); }
    if out.is_empty() { out.push('/'); }
    if out.len() > PATH_MAX { return Err(FsError::Invalid); }
    Ok(out)
}

fn page_bytes(phys: u64) -> &'static mut [u8; PAGE] { unsafe { &mut *(phys_to_virt(phys) as *mut [u8; PAGE]) } }
fn index_entries(phys: u64) -> &'static mut [u64; PER_INDEX] { unsafe { &mut *(phys_to_virt(phys) as *mut [u64; PER_INDEX]) } }
fn frame() -> Result<u64, FsError> { frames_allocate(1).map_err(|_| FsError::NoSpace) }

/// File contents in physical frames. `index` holds frames of `PER_INDEX` data-page
/// addresses each (0 = a page never written, which reads as zeros). Bytes past `len`
/// in the last page are always zero.
pub struct FileData { index: Vec<u64>, len: u64 }

impl FileData {
    pub const fn new() -> Self { Self { index: Vec::new(), len: 0 } }
    pub fn len(&self) -> u64 { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }

    fn page(&self, n: usize) -> u64 {
        self.index.get(n / PER_INDEX).map_or(0, |&ix| index_entries(ix)[n % PER_INDEX])
    }
    fn page_or_new(&mut self, n: usize) -> Result<u64, FsError> {
        while self.index.len() <= n / PER_INDEX { self.index.push(frame()?); }
        let entries = index_entries(self.index[n / PER_INDEX]);
        if entries[n % PER_INDEX] == 0 { entries[n % PER_INDEX] = frame()?; }
        Ok(entries[n % PER_INDEX])
    }

    /// Copies bytes from `offset` into `out`; returns how many (0 at the end).
    pub fn read_at(&self, offset: u64, out: &mut [u8]) -> usize {
        if offset >= self.len { return 0; }
        let count = ((self.len - offset) as usize).min(out.len());
        let mut done = 0;
        while done < count {
            let at = offset as usize + done;
            let (n, within) = (at / PAGE, at % PAGE);
            let chunk = (PAGE - within).min(count - done);
            match self.page(n) {
                0 => out[done..done + chunk].fill(0),
                phys => out[done..done + chunk].copy_from_slice(&page_bytes(phys)[within..within + chunk]),
            }
            done += chunk;
        }
        count
    }

    /// Writes `data` at `offset` (a gap past the end reads as zeros).
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<usize, FsError> {
        let end = offset.checked_add(data.len() as u64).ok_or(FsError::NoSpace)?;
        if end > FILE_MAX { return Err(FsError::NoSpace); }
        let mut done = 0;
        while done < data.len() {
            let at = offset as usize + done;
            let (n, within) = (at / PAGE, at % PAGE);
            let chunk = (PAGE - within).min(data.len() - done);
            let phys = self.page_or_new(n)?;
            page_bytes(phys)[within..within + chunk].copy_from_slice(&data[done..done + chunk]);
            done += chunk;
        }
        self.len = self.len.max(end);
        Ok(data.len())
    }

    /// Shrinks or extends to `len`. Shrinking frees whole pages past the end and zeroes
    /// the tail of the last one, so a later extension reads zeros.
    pub fn truncate(&mut self, len: u64) -> Result<(), FsError> {
        if len > FILE_MAX { return Err(FsError::NoSpace); }
        if len < self.len {
            let keep = (len as usize).div_ceil(PAGE);
            let pages = (self.len as usize).div_ceil(PAGE);
            for n in keep..pages {
                let Some(&ix) = self.index.get(n / PER_INDEX) else { break };
                let entry = &mut index_entries(ix)[n % PER_INDEX];
                if *entry != 0 { frames_free(*entry, 1); *entry = 0; }
            }
            let indexes = keep.div_ceil(PER_INDEX);
            while self.index.len() > indexes { frames_free(self.index.pop().unwrap(), 1); }
            if len as usize % PAGE != 0 {
                let phys = self.page(len as usize / PAGE);
                if phys != 0 { page_bytes(phys)[len as usize % PAGE..].fill(0); }
            }
        }
        self.len = len;
        Ok(())
    }

    /// Calls `f` with each page of contents in order (zeros for unwritten pages); the
    /// last slice is cut at the end of the file.
    pub fn for_each_page(&self, mut f: impl FnMut(&[u8])) {
        static ZERO: [u8; PAGE] = [0; PAGE];
        let pages = (self.len as usize).div_ceil(PAGE);
        for n in 0..pages {
            let len = (self.len as usize - n * PAGE).min(PAGE);
            match self.page(n) { 0 => f(&ZERO[..len]), phys => f(&page_bytes(phys)[..len]) }
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = alloc::vec![0; self.len as usize];
        self.read_at(0, &mut out);
        out
    }
}

impl Drop for FileData {
    fn drop(&mut self) {
        for &ix in &self.index {
            for &page in index_entries(ix).iter() { if page != 0 { frames_free(page, 1); } }
            frames_free(ix, 1);
        }
    }
}

/// A run of disk blocks holding part of a saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent { pub start: u64, pub count: u32 }

pub enum Content {
    /// A program embedded in the boot image: read-only, never saved.
    Builtin(&'static [u8]),
    Data(FileData),
    /// Saved on disk and not read yet; `storage::ensure_loaded` reads it.
    Unloaded,
}

pub struct File {
    pub content: Content,
    pub size: u64,
    /// Where the last saved version is on disk, and its checksum.
    pub extents: Vec<Extent>,
    pub checksum: u64,
    /// Changed since the last save.
    pub dirty: bool,
}

pub enum Kind { Dir(Vec<Ino>), File(File) }

pub struct Node {
    pub name: String,
    pub parent: Ino,
    /// Seconds since 1970 (UTC).
    pub modified: u64,
    /// The boot-image program folder and its programs: cannot be removed or renamed.
    pub system: bool,
    pub kind: Kind,
}

impl Node {
    pub fn is_dir(&self) -> bool { matches!(self.kind, Kind::Dir(_)) }
    pub fn size(&self) -> u64 { match &self.kind { Kind::File(f) => f.size, Kind::Dir(c) => c.len() as u64 } }
    pub fn file(&self) -> Option<&File> { if let Kind::File(f) = &self.kind { Some(f) } else { None } }
}

pub struct Fs {
    nodes: Vec<Option<Node>>,
    free: Vec<Ino>,
    /// Folders or names changed since the last save.
    pub meta_dirty: bool,
    /// Disk blocks the saved files may use (None without a disk: RAM only).
    pub capacity: Option<u64>,
    /// Blocks the current user files need when saved.
    pub blocks: u64,
    /// Counts every change; `saved_revision` is its value at the last save.
    pub revision: u64,
    pub saved_revision: u64,
}

fn blocks_for(size: u64) -> u64 { size.div_ceil(PAGE as u64) }

impl Fs {
    pub const fn new() -> Self {
        Self { nodes: Vec::new(), free: Vec::new(), meta_dirty: false, capacity: None, blocks: 0, revision: 0, saved_revision: 0 }
    }
    fn changed(&mut self) { self.meta_dirty = true; self.revision = self.revision.wrapping_add(1); }

    fn root_ready(&mut self) {
        if self.nodes.is_empty() {
            self.nodes.push(Some(Node { name: String::new(), parent: ROOT, modified: 0, system: false, kind: Kind::Dir(Vec::new()) }));
        }
    }

    pub fn node(&self, ino: Ino) -> Option<&Node> {
        if ino == ROOT && self.nodes.is_empty() { return Some(&EMPTY_ROOT); }
        self.nodes.get(ino as usize).and_then(|n| n.as_ref())
    }
    pub fn node_mut(&mut self, ino: Ino) -> Option<&mut Node> {
        self.root_ready();
        self.nodes.get_mut(ino as usize).and_then(|n| n.as_mut())
    }
    pub fn file_mut(&mut self, ino: Ino) -> Option<&mut File> {
        match self.node_mut(ino) { Some(Node { kind: Kind::File(f), .. }) => Some(f), _ => None }
    }
    pub fn children(&self, dir: Ino) -> Result<&[Ino], FsError> {
        match self.node(dir).ok_or(FsError::NotFound)?.kind { Kind::Dir(ref c) => Ok(c), _ => Err(FsError::NotDir) }
    }
    /// Every live node except the root, parents before children.
    pub fn walk(&self) -> Vec<Ino> {
        let mut out = Vec::new();
        let mut stack = alloc::vec![ROOT];
        while let Some(dir) = stack.pop() {
            if let Ok(children) = self.children(dir) {
                for &c in children { out.push(c); if self.node(c).is_some_and(Node::is_dir) { stack.push(c); } }
            }
        }
        out
    }

    pub fn lookup(&self, dir: Ino, name: &str) -> Option<Ino> {
        self.children(dir).ok()?.iter().copied().find(|&c| self.node(c).is_some_and(|n| n.name == name))
    }
    pub fn resolve(&self, path: &str) -> Result<Ino, FsError> {
        let mut at = ROOT;
        for part in components(path)? {
            if !self.node(at).ok_or(FsError::NotFound)?.is_dir() { return Err(FsError::NotDir); }
            at = self.lookup(at, part).ok_or(FsError::NotFound)?;
        }
        Ok(at)
    }
    /// The folder a path's last component lives in, and that component.
    pub fn parent_of<'a>(&self, path: &'a str) -> Result<(Ino, &'a str), FsError> {
        let parts = components(path)?;
        let (&name, folders) = parts.split_last().ok_or(FsError::Invalid)?;
        let mut at = ROOT;
        for part in folders {
            at = self.lookup(at, part).ok_or(FsError::NotFound)?;
            if !self.node(at).ok_or(FsError::NotFound)?.is_dir() { return Err(FsError::NotDir); }
        }
        Ok((at, name))
    }
    pub fn path_of(&self, mut ino: Ino) -> String {
        let mut parts = Vec::new();
        while ino != ROOT {
            let Some(node) = self.node(ino) else { break };
            parts.push(node.name.as_str());
            ino = node.parent;
        }
        let mut path = String::new();
        for part in parts.iter().rev() { path.push('/'); path.push_str(part); }
        if path.is_empty() { path.push('/'); }
        path
    }

    /// Adds a node under `parent` (which must be a folder without that name).
    pub fn insert(&mut self, parent: Ino, name: &str, modified: u64, kind: Kind) -> Result<Ino, FsError> {
        self.root_ready();
        if !valid_name(name) { return Err(FsError::Invalid); }
        if !self.node(parent).ok_or(FsError::NotFound)?.is_dir() { return Err(FsError::NotDir); }
        if self.lookup(parent, name).is_some() { return Err(FsError::Exists); }
        let node = Node { name: String::from(name), parent, modified, system: false, kind };
        let ino = match self.free.pop() {
            Some(ino) => { self.nodes[ino as usize] = Some(node); ino }
            None => { self.nodes.push(Some(node)); (self.nodes.len() - 1) as Ino }
        };
        if let Some(Node { kind: Kind::Dir(children), .. }) = self.node_mut(parent) { children.push(ino); }
        Ok(ino)
    }

    pub fn mkdir(&mut self, path: &str, now: u64) -> Result<Ino, FsError> {
        let (parent, name) = self.parent_of(path)?;
        let ino = self.insert(parent, name, now, Kind::Dir(Vec::new()))?;
        self.changed();
        Ok(ino)
    }

    /// The file at `path`, created empty if it does not exist.
    pub fn open_or_create(&mut self, path: &str, now: u64) -> Result<Ino, FsError> {
        match self.resolve(path) {
            Ok(ino) if self.node(ino).is_some_and(Node::is_dir) => Err(FsError::IsDir),
            Ok(ino) => Ok(ino),
            Err(FsError::NotFound) => {
                let (parent, name) = self.parent_of(path)?;
                let file = File { content: Content::Data(FileData::new()), size: 0, extents: Vec::new(), checksum: 0, dirty: true };
                let ino = self.insert(parent, name, now, Kind::File(file))?;
                self.changed();
                Ok(ino)
            }
            Err(e) => Err(e),
        }
    }

    /// Embeds a boot-image program as `/bin/<name>`, unless a saved file already
    /// has that name.
    pub fn install_builtin(&mut self, name: &str, bytes: &'static [u8]) -> Result<Ino, FsError> {
        self.root_ready();
        let bin = match self.lookup(ROOT, BIN) {
            Some(ino) => ino,
            None => self.insert(ROOT, BIN, 0, Kind::Dir(Vec::new()))?,
        };
        if let Some(node) = self.node_mut(bin) { node.system = true; }
        let file = File { content: Content::Builtin(bytes), size: bytes.len() as u64, extents: Vec::new(), checksum: 0, dirty: false };
        let ino = self.insert(bin, name, 0, Kind::File(file))?;
        if let Some(node) = self.node_mut(ino) { node.system = true; }
        Ok(ino)
    }

    pub fn is_builtin(&self, ino: Ino) -> bool {
        matches!(self.node(ino).and_then(Node::file), Some(File { content: Content::Builtin(_), .. }))
    }

    /// Removes a file or an empty folder. `open` reports whether a descriptor holds a file.
    pub fn remove(&mut self, path: &str, open: impl Fn(Ino) -> bool) -> Result<(), FsError> {
        let ino = self.resolve(path)?;
        let node = self.node(ino).ok_or(FsError::NotFound)?;
        if ino == ROOT || node.system { return Err(FsError::ReadOnly); }
        if let Kind::Dir(children) = &node.kind { if !children.is_empty() { return Err(FsError::NotEmpty); } }
        if open(ino) { return Err(FsError::Busy); }
        let parent = node.parent;
        let size = node.file().map_or(0, |f| f.size);
        if let Some(Node { kind: Kind::Dir(children), .. }) = self.node_mut(parent) { children.retain(|&c| c != ino); }
        self.nodes[ino as usize] = None; // Drops the contents and frees their frames.
        self.free.push(ino);
        self.blocks -= blocks_for(size);
        self.changed();
        Ok(())
    }

    /// Renames or moves `from` to `to`; `to` must not exist. A folder cannot move into
    /// itself.
    pub fn rename(&mut self, from: &str, to: &str, now: u64) -> Result<(), FsError> {
        let ino = self.resolve(from)?;
        if ino == ROOT || self.node(ino).ok_or(FsError::NotFound)?.system { return Err(FsError::ReadOnly); }
        let (parent, name) = self.parent_of(to)?;
        if !valid_name(name) { return Err(FsError::Invalid); }
        if self.lookup(parent, name).is_some() { return Err(FsError::Exists); }
        let mut at = parent;
        loop {
            if at == ino { return Err(FsError::Invalid); }
            if at == ROOT { break; }
            at = self.node(at).ok_or(FsError::NotFound)?.parent;
        }
        let old_parent = self.node(ino).ok_or(FsError::NotFound)?.parent;
        if let Some(Node { kind: Kind::Dir(children), .. }) = self.node_mut(old_parent) { children.retain(|&c| c != ino); }
        if let Some(Node { kind: Kind::Dir(children), .. }) = self.node_mut(parent) { children.push(ino); }
        let node = self.node_mut(ino).ok_or(FsError::NotFound)?;
        node.name = String::from(name);
        node.parent = parent;
        node.modified = now;
        self.changed();
        Ok(())
    }

    fn data_mut(&mut self, ino: Ino) -> Result<&mut File, FsError> {
        let file = self.file_mut(ino).ok_or(FsError::IsDir)?;
        match file.content {
            Content::Builtin(_) => Err(FsError::ReadOnly),
            Content::Unloaded => Err(FsError::Io),
            Content::Data(_) => Ok(file),
        }
    }

    /// Checks that growing a file from `old` to `new` bytes keeps every file saveable.
    fn reserve(&mut self, old: u64, new: u64) -> Result<(), FsError> {
        let (old, new) = (blocks_for(old), blocks_for(new));
        let total = self.blocks - old + new;
        if new > old && self.capacity.is_some_and(|cap| total > cap) { return Err(FsError::NoSpace); }
        self.blocks = total;
        Ok(())
    }

    pub fn read(&self, ino: Ino, offset: u64, out: &mut [u8]) -> Result<usize, FsError> {
        match &self.node(ino).ok_or(FsError::NotFound)?.kind {
            Kind::Dir(_) => Err(FsError::IsDir),
            Kind::File(File { content: Content::Builtin(bytes), .. }) => {
                let start = (offset as usize).min(bytes.len());
                let count = (bytes.len() - start).min(out.len());
                out[..count].copy_from_slice(&bytes[start..start + count]);
                Ok(count)
            }
            Kind::File(File { content: Content::Data(data), .. }) => Ok(data.read_at(offset, out)),
            Kind::File(File { content: Content::Unloaded, .. }) => Err(FsError::Io),
        }
    }

    pub fn write(&mut self, ino: Ino, offset: u64, bytes: &[u8], now: u64) -> Result<usize, FsError> {
        let size = self.data_mut(ino)?.size;
        let end = offset.checked_add(bytes.len() as u64).ok_or(FsError::NoSpace)?.max(size);
        if end > FILE_MAX { return Err(FsError::NoSpace); }
        self.reserve(size, end)?;
        let file = self.data_mut(ino)?;
        let Content::Data(data) = &mut file.content else { return Err(FsError::Io) };
        let result = data.write_at(offset, bytes);
        let len = data.len();
        file.size = len;
        file.dirty = true;
        // A failed write (out of memory) may have stored part of the data.
        self.blocks = self.blocks - blocks_for(end) + blocks_for(len);
        if let Some(node) = self.node_mut(ino) { node.modified = now; }
        self.changed();
        result
    }

    pub fn truncate(&mut self, ino: Ino, len: u64, now: u64) -> Result<(), FsError> {
        let size = self.data_mut(ino)?.size;
        self.reserve(size, len)?;
        let file = self.data_mut(ino)?;
        let Content::Data(data) = &mut file.content else { return Err(FsError::Io) };
        data.truncate(len)?;
        file.size = len;
        file.dirty = true;
        if let Some(node) = self.node_mut(ino) { node.modified = now; }
        self.changed();
        Ok(())
    }

    /// Replaces a file's contents with `bytes`. A size that would not fit on the disk
    /// is refused before anything changes.
    pub fn replace(&mut self, ino: Ino, bytes: &[u8], now: u64) -> Result<(), FsError> {
        let size = self.data_mut(ino)?.size;
        let new = bytes.len() as u64;
        if new > FILE_MAX { return Err(FsError::NoSpace); }
        let (old, grown) = (blocks_for(size), blocks_for(new));
        if grown > old && self.capacity.is_some_and(|cap| self.blocks - old + grown > cap) { return Err(FsError::NoSpace); }
        self.truncate(ino, 0, now)?;
        self.write(ino, 0, bytes, now).map(|_| ())
    }

    /// Files outside the boot-image program folder.
    pub fn user_files(&self) -> usize {
        self.walk().iter().filter(|&&i| self.node(i).is_some_and(|n| !n.system && !n.is_dir())).count()
    }
    /// Bytes of file contents held in memory (loaded or written, not built-in).
    pub fn loaded_bytes(&self) -> u64 {
        self.walk().iter().filter_map(|&i| self.node(i).and_then(Node::file))
            .filter(|f| matches!(f.content, Content::Data(_))).map(|f| f.size).sum()
    }

    /// True when anything changed since the last save.
    pub fn unsaved(&self) -> bool {
        self.meta_dirty || self.walk().iter().any(|&i| self.node(i).and_then(Node::file).is_some_and(|f| f.dirty))
    }
}

static EMPTY_ROOT: Node = Node { name: String::new(), parent: ROOT, modified: 0, system: false, kind: Kind::Dir(Vec::new()) };

/// The system's file tree.
pub static ROOT_FS: Spinlock<Fs> = Spinlock::new(Fs::new());
