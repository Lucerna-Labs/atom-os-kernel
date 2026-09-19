//! Hierarchical AtomNode namespace with owned handles and pre-mutation durable quotas.
//! The root is a directory; paths are '/'-separated and resolve against a
//! base (per-process working) directory with a hard MAX_DEPTH segment cap.
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::memory::Spinlock;
pub use crate::abi::FsError;

pub const MAX_FILES: usize = 128;
pub const MAX_FILE_BYTES: usize = 65536;
pub const MAX_NAME_BYTES: usize = 63;
pub const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
pub const MAX_LIVE_BYTES: usize = 2 * MAX_SNAPSHOT_BYTES;
/// Hard cap on the number of segments in any canonical path.
pub const MAX_DEPTH: usize = 16;

pub fn builtin(name: &str) -> bool { matches!(name, "shell.elf" | "daemon.elf" | "worker.elf" | "fault.elf" | "fs-probe.elf" | "spider.elf" | "rogue.elf" | "weave.elf" | "keykeep.elf" | "instant.elf" | "smuggler.elf" | "lane.elf" | "metro.elf" | "taint.elf" | "crypt.elf" | "seam.elf" | "net.elf" | "sock.elf" | "ping.elf" | "hello.elf" | "sysinfo.elf" | "netstat.elf" | "calc.elf" | "udpsend.elf") }
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.bytes().any(|b| b < 32 || b == b'/')
}

pub struct File {
    bytes: Spinlock<Vec<u8>>,
    readonly: bool,
    budget: Arc<AtomicUsize>,
}
impl core::fmt::Debug for File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("File").field("readonly", &self.readonly).finish_non_exhaustive()
    }
}
impl File {
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let data = self.bytes.lock();
        let result = f(data);
        self.bytes.unlock();
        result
    }
    pub fn len(&self) -> usize { self.with_bytes(|data| data.len()) }
    pub fn byte(&self, offset: usize) -> Option<u8> { self.with_bytes(|data| data.get(offset).copied()) }
}
impl Drop for File {
    fn drop(&mut self) {
        if !self.readonly { self.budget.fetch_sub(self.bytes.lock().capacity(), Ordering::Relaxed); self.bytes.unlock(); }
    }
}

#[derive(Debug)]
pub struct OpenFile { pub file: Arc<File>, pub cursor: usize }
pub enum AtomNode { File(Arc<File>), Directory(Vec<(String, AtomNode)>) }

fn directory(node: &AtomNode) -> Option<&Vec<(String, AtomNode)>> {
    match node { AtomNode::Directory(children) => Some(children), AtomNode::File(_) => None }
}

/// Byte length of the root-relative snapshot name of a canonical path
/// (["a","b"] -> "a/b"): segments plus separators, no leading '/'.
fn joined_len(segments: &[String]) -> usize {
    segments.iter().map(|segment| segment.len() + 1).sum::<usize>().saturating_sub(1)
}

/// Canonical display form of a resolved path (["a","b"] -> "/a/b", root -> "/").
pub fn canonical(segments: &[String]) -> String {
    let mut path = String::from("/");
    for (index, segment) in segments.iter().enumerate() {
        if index != 0 { path.push('/'); }
        path.push_str(segment);
    }
    path
}

/// Resolve `path` against `base` (the caller's canonical working directory;
/// an empty base is the root). Absolute paths start with '/'; empty segments
/// collapse; '.' stays; '..' pops, staying at the root when already there.
/// Hard caps: MAX_DEPTH segments, MAX_NAME_BYTES per segment.
pub fn resolve(base: &str, path: &str) -> Result<Vec<String>, FsError> {
    let mut segments: Vec<String> = Vec::new();
    if !path.starts_with('/') {
        // Bases are kernel-produced canonical paths; validate defensively.
        for segment in base.split('/') {
            match segment {
                "" => {}
                "." | ".." => return Err(FsError::InvalidName),
                name => {
                    if !valid_name(name) || segments.len() >= MAX_DEPTH { return Err(FsError::InvalidName); }
                    segments.try_reserve(1).map_err(|_| FsError::Memory)?;
                    segments.push(String::from(name));
                }
            }
        }
    }
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => { segments.pop(); }
            name => {
                if !valid_name(name) || segments.len() >= MAX_DEPTH { return Err(FsError::InvalidName); }
                segments.try_reserve(1).map_err(|_| FsError::Memory)?;
                segments.push(String::from(name));
            }
        }
    }
    Ok(segments)
}

/// Validate a snapshot-layer entry name: a root-relative path whose segments
/// are legal names, optionally ending in '/' to mark a directory (file names
/// can never contain '/', so the marker is unambiguous).
pub fn snapshot_segments(name: &str) -> Result<(Vec<String>, bool), FsError> {
    let directory = name.ends_with('/');
    let body = if directory { &name[..name.len() - 1] } else { &name[..] };
    let body = body.strip_prefix('/').unwrap_or(body);
    if body.is_empty() { return Err(FsError::InvalidName); }
    let mut segments = Vec::new();
    for segment in body.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || !valid_name(segment)
            || segments.len() >= MAX_DEPTH {
            return Err(FsError::InvalidName);
        }
        segments.try_reserve(1).map_err(|_| FsError::Memory)?;
        segments.push(String::from(segment));
    }
    Ok((segments, directory))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage { pub files: usize, pub serialized_bytes: usize, pub live_bytes: usize,
    pub dirty: bool, pub generation: u64, pub revision: u64, pub saved_revision: u64 }

/// What sits at a resolved path: a directory, a file, or nothing (the parent
/// chain must still exist; a missing or non-directory component is an error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe { Directory, File, Missing }

pub struct FileSystem {
    root: AtomNode,
    budget: Option<Arc<AtomicUsize>>,
    revision: u64,
    saved_revision: u64,
    generation: u64,
}
impl FileSystem {
    pub const fn new() -> Self { Self { root: AtomNode::Directory(Vec::new()), budget: None,
        revision: 0, saved_revision: 0, generation: 0 } }
    pub fn entries(&self) -> &[(String, AtomNode)] {
        match &self.root { AtomNode::Directory(entries) => entries, _ => unreachable!() }
    }
    fn entries_mut(&mut self) -> &mut Vec<(String, AtomNode)> {
        match &mut self.root { AtomNode::Directory(entries) => entries, _ => unreachable!() }
    }
    fn changed(&mut self) { self.revision = self.revision.wrapping_add(1); }
    fn make_file(&mut self, bytes: Vec<u8>, readonly: bool) -> Arc<File> {
        let budget = self.budget.get_or_insert_with(|| Arc::new(AtomicUsize::new(0))).clone();
        if !readonly { budget.fetch_add(bytes.capacity(), Ordering::Relaxed); }
        Arc::new(File { bytes: Spinlock::new(bytes), readonly, budget })
    }
    pub fn file(&self, name: &str) -> Option<Arc<File>> {
        self.entries().iter().find_map(|(key, node)| {
            if key == name { if let AtomNode::File(file) = node { return Some(file.clone()); } }
            None
        })
    }

    // ---- tree navigation -------------------------------------------------

    /// Walk to the directory at `segments` (empty = root). Missing
    /// components are NotFound; a file component is NotDir.
    fn walk(&self, segments: &[String]) -> Result<&Vec<(String, AtomNode)>, FsError> {
        let mut node = &self.root;
        for segment in segments {
            let children = directory(node).ok_or(FsError::NotDir)?;
            node = &children.iter().find(|(key, _)| key == segment).ok_or(FsError::NotFound)?.1;
        }
        directory(node).ok_or(FsError::NotDir)
    }
    /// Mutable twin of `walk`; pre-validated paths only (None = impossible).
    fn walk_dir_mut(&mut self, segments: &[String]) -> Option<&mut Vec<(String, AtomNode)>> {
        let mut node = &mut self.root;
        for segment in segments {
            let children = match node { AtomNode::Directory(children) => children, _ => return None };
            let index = children.iter().position(|(key, _)| key == segment)?;
            node = &mut children[index].1;
        }
        match node { AtomNode::Directory(children) => Some(children), _ => None }
    }
    fn lookup(&self, segments: &[String]) -> Option<&AtomNode> {
        let mut node = &self.root;
        for segment in segments {
            node = &directory(node)?.iter().find(|(key, _)| key == segment)?.1;
        }
        Some(node)
    }
    fn probe(&self, segments: &[String]) -> Result<Probe, FsError> {
        if segments.is_empty() { return Ok(Probe::Directory); }
        let parent = self.walk(&segments[..segments.len() - 1])?;
        let leaf = segments.last().unwrap();
        Ok(match parent.iter().find(|(key, _)| key == leaf) {
            Some((_, AtomNode::Directory(_))) => Probe::Directory,
            Some((_, AtomNode::File(_))) => Probe::File,
            None => Probe::Missing,
        })
    }

    fn linked(&self, file: &Arc<File>) -> bool {
        fn in_entries(entries: &[(String, AtomNode)], file: &Arc<File>) -> bool {
            entries.iter().any(|(_, node)| match node {
                AtomNode::File(found) => Arc::ptr_eq(found, file),
                AtomNode::Directory(children) => in_entries(children, file),
            })
        }
        in_entries(self.entries(), file)
    }
    fn owns(&self, file: &Arc<File>) -> bool {
        self.budget.as_ref().is_some_and(|b| Arc::ptr_eq(b, &file.budget))
    }
    pub fn usage(&self) -> Usage {
        // Mirrors storage::encode's versioned payload byte-for-byte: a 12-byte
        // magic+count header, 7 + path + data per writable file, 3 + path for
        // each directory whose subtree holds no snapshot entries.
        fn measure(entries: &[(String, AtomNode)], prefix: usize, files: &mut usize, bytes: &mut usize) -> bool {
            let mut emitted = false;
            for (name, node) in entries {
                let path_len = prefix + name.len();
                match node {
                    AtomNode::File(file) if !file.readonly => { *files += 1; *bytes += 7 + path_len + file.len(); emitted = true; }
                    AtomNode::File(_) => {}
                    AtomNode::Directory(children) => {
                        if measure(children, path_len + 1, files, bytes) { emitted = true; }
                        else { *bytes += 3 + path_len; emitted = true; }
                    }
                }
            }
            emitted
        }
        let mut files = 0; let mut bytes = 12;
        let _ = measure(self.entries(), 0, &mut files, &mut bytes);
        Usage { files, serialized_bytes: bytes, live_bytes: self.budget.as_ref().map_or(0, |b| b.load(Ordering::Relaxed)),
            dirty: self.revision != self.saved_revision, generation: self.generation,
            revision: self.revision, saved_revision: self.saved_revision }
    }
    pub fn mark_saved(&mut self, revision: u64, generation: u64) {
        self.saved_revision = revision; self.generation = generation;
    }

    // ---- path-aware operations (used by the syscall layer) ---------------

    /// Open (creating if absent) the file at `path`. Opening a directory, or
    /// a path whose parent does not exist, fails.
    pub fn open_at(&mut self, base: &str, path: &str) -> Result<Arc<File>, FsError> {
        let segments = resolve(base, path)?;
        if segments.is_empty() { return Err(FsError::IsDir); }
        let leaf = segments.last().unwrap().as_str();
        match self.probe(&segments)? {
            Probe::File => {
                let parent = self.walk(&segments[..segments.len() - 1]).map_err(|_| FsError::NotFound)?;
                if let Some((_, AtomNode::File(file))) = parent.iter().find(|(key, _)| key == leaf) {
                    return Ok(file.clone());
                }
                return Err(FsError::NotFound);
            }
            Probe::Directory => return Err(FsError::IsDir),
            Probe::Missing => {}
        }
        if segments.len() == 1 && builtin(leaf) { return Err(FsError::ReadOnly); }
        let usage = self.usage();
        if usage.files >= MAX_FILES || usage.serialized_bytes + 7 + joined_len(&segments) > MAX_SNAPSHOT_BYTES {
            return Err(FsError::NoSpace);
        }
        let file = self.make_file(Vec::new(), false);
        let parent = self.walk_dir_mut(&segments[..segments.len() - 1]).ok_or(FsError::NotFound)?;
        parent.try_reserve(1).map_err(|_| FsError::Memory)?;
        parent.push((String::from(leaf), AtomNode::File(file.clone())));
        self.changed(); Ok(file)
    }
    pub fn open_existing_at(&self, base: &str, path: &str) -> Result<Arc<File>, FsError> {
        let segments = resolve(base, path)?;
        if segments.is_empty() { return Err(FsError::IsDir); }
        let parent = self.walk(&segments[..segments.len() - 1])?;
        let leaf = segments.last().unwrap();
        match parent.iter().find(|(key, _)| key == leaf) {
            Some((_, AtomNode::File(file))) => Ok(file.clone()),
            Some((_, AtomNode::Directory(_))) => Err(FsError::IsDir),
            None => Err(FsError::NotFound),
        }
    }
    /// Strict mkdir: every parent must already exist (no mkdir -p).
    pub fn mkdir_at(&mut self, base: &str, path: &str) -> Result<(), FsError> {
        let segments = resolve(base, path)?;
        if !matches!(self.probe(&segments)?, Probe::Missing) { return Err(FsError::Exists); }
        let leaf = segments.last().ok_or(FsError::Exists)?.as_str(); // the root itself always exists
        if segments.len() == 1 && builtin(leaf) { return Err(FsError::ReadOnly); }
        let usage = self.usage();
        if usage.serialized_bytes + 3 + joined_len(&segments) > MAX_SNAPSHOT_BYTES { return Err(FsError::NoSpace); }
        let parent = self.walk_dir_mut(&segments[..segments.len() - 1]).ok_or(FsError::NotFound)?;
        parent.try_reserve(1).map_err(|_| FsError::Memory)?;
        parent.push((String::from(leaf), AtomNode::Directory(Vec::new())));
        self.changed(); Ok(())
    }
    /// Resolve `path` to a directory and return its canonical absolute form
    /// (the caller stores it as the new working directory).
    pub fn directory_at(&self, base: &str, path: &str) -> Result<String, FsError> {
        let segments = resolve(base, path)?;
        match self.probe(&segments)? {
            Probe::Directory => Ok(canonical(&segments)),
            _ => Err(FsError::NotDir),
        }
    }
    /// Entry names of the resolved directory; directories carry a trailing
    /// '/' so they are visually distinct.
    pub fn list_dir_at(&self, base: &str, path: &str) -> Result<Vec<String>, FsError> {
        let segments = resolve(base, path)?;
        let entries = self.walk(&segments)?;
        let mut names = Vec::new();
        for (name, node) in entries {
            let mut label = name.clone();
            if matches!(node, AtomNode::Directory(_)) { label.push('/'); }
            names.try_reserve(1).map_err(|_| FsError::Memory)?;
            names.push(label);
        }
        Ok(names)
    }
    /// Remove a file or an EMPTY directory; non-empty directories are
    /// refused and open handles keep their Arc.
    pub fn remove_at(&mut self, base: &str, path: &str) -> Result<(), FsError> {
        let segments = resolve(base, path)?;
        if segments.is_empty() { return Err(FsError::InvalidName); } // the root is not removable
        let leaf = segments.last().unwrap().as_str();
        {
            let parent = self.walk(&segments[..segments.len() - 1])?;
            match parent.iter().find(|(key, _)| key == leaf) {
                None => return Err(FsError::NotFound),
                Some((_, AtomNode::File(file))) if file.readonly => return Err(FsError::ReadOnly),
                Some((_, AtomNode::Directory(children))) if !children.is_empty() => return Err(FsError::NotEmpty),
                _ => {}
            }
        }
        let parent = self.walk_dir_mut(&segments[..segments.len() - 1]).ok_or(FsError::NotFound)?;
        let index = parent.iter().position(|(key, _)| key == leaf).ok_or(FsError::NotFound)?;
        parent.remove(index); // open handles retain their Arc
        self.changed(); Ok(())
    }
    /// Rename/move within the tree. A destination that is an existing
    /// directory absorbs the source (mv-into-directory); an existing file
    /// collides; moving a directory into its own subtree is refused.
    pub fn rename_at(&mut self, base: &str, old: &str, new: &str) -> Result<(), FsError> {
        let old_segments = resolve(base, old)?;
        let mut new_segments = resolve(base, new)?;
        if old_segments == new_segments { return Ok(()); }
        if old_segments.is_empty() || new_segments.is_empty() { return Err(FsError::InvalidName); }
        let old_leaf = old_segments.last().unwrap().as_str();
        if old_segments.len() == 1 && builtin(old_leaf) { return Err(FsError::ReadOnly); }
        if new_segments.len() == 1 && builtin(&new_segments[0]) { return Err(FsError::ReadOnly); }
        if matches!(self.probe(&old_segments)?, Probe::Missing) { return Err(FsError::NotFound); }
        match self.probe(&new_segments)? {
            Probe::Directory => {
                if new_segments.len() >= MAX_DEPTH { return Err(FsError::InvalidName); }
                new_segments.try_reserve(1).map_err(|_| FsError::Memory)?;
                new_segments.push(String::from(old_leaf));
            }
            Probe::File => return Err(FsError::Exists),
            Probe::Missing => {}
        }
        let new_leaf = new_segments.last().unwrap().as_str();
        {
            let parent = self.walk(&new_segments[..new_segments.len() - 1])?;
            if parent.iter().any(|(key, _)| key == new_leaf) { return Err(FsError::Exists); }
        }
        // A directory cannot move into its own subtree.
        if new_segments.len() >= old_segments.len()
            && new_segments[..old_segments.len()] == old_segments[..] {
            return Err(FsError::InvalidName);
        }
        // Durable-quota check: the moved subtree re-serializes under the new path.
        fn entry_cost(node: &AtomNode, path_len: usize) -> usize {
            match node {
                AtomNode::File(file) if !file.readonly => 7 + path_len + file.len(),
                AtomNode::File(_) => 0,
                AtomNode::Directory(children) => {
                    let (mut total, mut emitted) = (0, false);
                    for (name, child) in children {
                        let cost = entry_cost(child, path_len + name.len() + 1);
                        if cost > 0 { total += cost; emitted = true; }
                    }
                    if emitted { total } else { 3 + path_len }
                }
            }
        }
        let usage = self.usage();
        {
            let parent = self.walk(&old_segments[..old_segments.len() - 1])?;
            let (_, node) = parent.iter().find(|(key, _)| key == old_leaf).ok_or(FsError::NotFound)?;
            if usage.serialized_bytes.saturating_sub(entry_cost(node, joined_len(&old_segments)))
                + entry_cost(node, joined_len(&new_segments)) > MAX_SNAPSHOT_BYTES {
                return Err(FsError::NoSpace);
            }
        }
        // Reserve destination capacity before detaching anything.
        {
            let parent = self.walk_dir_mut(&new_segments[..new_segments.len() - 1]).ok_or(FsError::NotFound)?;
            parent.try_reserve(1).map_err(|_| FsError::Memory)?;
        }
        let node = {
            let parent = self.walk_dir_mut(&old_segments[..old_segments.len() - 1]).ok_or(FsError::NotFound)?;
            let index = parent.iter().position(|(key, _)| key == old_leaf).ok_or(FsError::NotFound)?;
            let (_, node) = parent.remove(index);
            node
        };
        let parent = self.walk_dir_mut(&new_segments[..new_segments.len() - 1]).ok_or(FsError::NotFound)?;
        parent.push((String::from(new_leaf), node));
        self.changed(); Ok(())
    }

    // ---- flat root-level API (legacy callers, exec image lookup) ---------

    pub fn open(&mut self, name: &str) -> Result<Arc<File>, FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        self.open_at("", name)
    }
    pub fn open_existing(&self, name: &str) -> Result<Arc<File>, FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        self.open_existing_at("", name)
    }
    pub fn remove(&mut self, name: &str) -> Result<(), FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        self.remove_at("", name)
    }
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        if !valid_name(old) || !valid_name(new) { return Err(FsError::InvalidName); }
        self.rename_at("", old, new)
    }
    pub fn insert_builtin(&mut self, name: &str, bytes: &[u8]) -> Result<(), FsError> {
        if !builtin(name) { return Err(FsError::InvalidName); }
        if self.entries().iter().any(|(key, _)| key == name) { return Err(FsError::Exists); }
        let file = self.make_file(bytes.to_vec(), true);
        self.entries_mut().push((String::from(name), AtomNode::File(file)));
        Ok(())
    }
    pub fn append(&mut self, file: &Arc<File>, bytes: &[u8]) -> Result<(), FsError> {
        self.modify(file, bytes, false)
    }
    pub fn replace(&mut self, file: &Arc<File>, bytes: &[u8]) -> Result<(), FsError> {
        self.modify(file, bytes, true)
    }
    fn modify(&mut self, file: &Arc<File>, input: &[u8], replace: bool) -> Result<(), FsError> {
        if !self.owns(file) { return Err(FsError::BadHandle); }
        if file.readonly { return Err(FsError::ReadOnly); }
        let linked = self.linked(file);
        let usage = self.usage(); // namespace -> file lock order, before holding target lock
        let data = file.bytes.lock();
        let result = (|| {
            let old = data.len();
            let new = if replace { input.len() } else { old.checked_add(input.len()).ok_or(FsError::FileTooLarge)? };
            if new > MAX_FILE_BYTES { return Err(FsError::FileTooLarge); }
            if linked && usage.serialized_bytes - old + new > MAX_SNAPSHOT_BYTES { return Err(FsError::NoSpace); }
            let old_capacity = data.capacity();
            let capacity = if replace { new } else if new > old_capacity { new.next_power_of_two().min(MAX_FILE_BYTES) } else { old_capacity };
            if usage.live_bytes - old_capacity + capacity > MAX_LIVE_BYTES { return Err(FsError::Memory); }
            if replace || new > old_capacity {
                // Build the replacement before swapping. Shrinking/truncating
                // must release capacity, including for unlinked open files.
                let mut replacement = Vec::new();
                replacement.try_reserve_exact(capacity).map_err(|_| FsError::Memory)?;
                if usage.live_bytes - old_capacity + replacement.capacity() > MAX_LIVE_BYTES { return Err(FsError::Memory); }
                if !replace { replacement.extend_from_slice(data); }
                replacement.extend_from_slice(input);
                *data = replacement;
            } else { data.extend_from_slice(input); }
            let new_capacity = data.capacity();
            if new_capacity >= old_capacity { file.budget.fetch_add(new_capacity - old_capacity, Ordering::Relaxed); }
            else { file.budget.fetch_sub(old_capacity - new_capacity, Ordering::Relaxed); }
            Ok(())
        })();
        file.bytes.unlock();
        if result.is_ok() && linked && (replace || !input.is_empty()) { self.changed(); }
        result
    }

    // ---- persistence ------------------------------------------------------

    pub fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        // Root-relative paths; a trailing '/' marks an empty directory (file
        // names can never contain '/'). Only directories whose subtree holds
        // no snapshot entries need an explicit marker — parents of real
        // entries are implied and recreated on restore.
        fn collect(entries: &[(String, AtomNode)], prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> bool {
            let mut emitted = false;
            for (name, node) in entries {
                match node {
                    AtomNode::File(file) if !file.readonly => {
                        let mut path = String::from(prefix);
                        path.push_str(name);
                        out.push((path, file.with_bytes(|data| data.to_vec())));
                        emitted = true;
                    }
                    AtomNode::File(_) => {}
                    AtomNode::Directory(children) => {
                        let mut child_prefix = String::from(prefix);
                        child_prefix.push_str(name);
                        child_prefix.push('/');
                        if collect(children, &child_prefix, out) { emitted = true; }
                        else { out.push((child_prefix, Vec::new())); emitted = true; }
                    }
                }
            }
            emitted
        }
        let mut files = Vec::new();
        let _ = collect(self.entries(), "", &mut files);
        files
    }
    /// Create the directory at `segments` and any missing parents (restore
    /// path only — the syscall layer enforces strict parents).
    fn make_dir_with_parents(&mut self, segments: &[String]) -> Result<(), FsError> {
        for depth in 1..=segments.len() {
            match self.lookup(&segments[..depth]) {
                Some(AtomNode::Directory(_)) => continue,
                Some(AtomNode::File(_)) => return Err(FsError::NotDir),
                None => {}
            }
            let usage = self.usage();
            if usage.serialized_bytes + 3 + joined_len(&segments[..depth]) > MAX_SNAPSHOT_BYTES {
                return Err(FsError::NoSpace);
            }
            let parent = self.walk_dir_mut(&segments[..depth - 1]).ok_or(FsError::NotFound)?;
            parent.try_reserve(1).map_err(|_| FsError::Memory)?;
            parent.push((segments[depth - 1].clone(), AtomNode::Directory(Vec::new())));
            self.changed();
        }
        Ok(())
    }
    pub fn restored(files: Vec<(String, Vec<u8>)>, generation: u64) -> Result<Self, FsError> {
        let mut fs = Self::new();
        for (name, bytes) in files {
            let (segments, directory) = snapshot_segments(&name)?;
            if segments.len() == 1 && builtin(&segments[0]) { return Err(FsError::ReadOnly); }
            if fs.lookup(&segments).is_some() { return Err(FsError::Exists); }
            if directory {
                if !bytes.is_empty() { return Err(FsError::InvalidName); }
                fs.make_dir_with_parents(&segments)?;
            } else {
                fs.make_dir_with_parents(&segments[..segments.len() - 1])?;
                let file = fs.open_at("", &name)?;
                fs.replace(&file, &bytes)?;
            }
        }
        fs.mark_saved(fs.revision, generation);
        Ok(fs)
    }
}

pub static ROOT_FS: Spinlock<FileSystem> = Spinlock::new(FileSystem::new());
