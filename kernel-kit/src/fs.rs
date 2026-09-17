//! Flat AtomNode namespace with owned handles and pre-mutation durable quotas.
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::memory::Spinlock;
pub use crate::abi::FsError;

pub const MAX_FILES: usize = 128;
pub const MAX_FILE_BYTES: usize = 65536;
pub const MAX_NAME_BYTES: usize = 63;
pub const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
pub const MAX_LIVE_BYTES: usize = 2 * MAX_SNAPSHOT_BYTES;

pub fn builtin(name: &str) -> bool { matches!(name, "shell.elf" | "daemon.elf" | "worker.elf" | "fault.elf" | "fs-probe.elf" | "spider.elf" | "rogue.elf") }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage { pub files: usize, pub serialized_bytes: usize, pub live_bytes: usize,
    pub dirty: bool, pub generation: u64, pub revision: u64, pub saved_revision: u64 }

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
    fn linked(&self, file: &Arc<File>) -> bool {
        self.entries().iter().any(|(_, node)| match node { AtomNode::File(f) => Arc::ptr_eq(f, file), _ => false })
    }
    fn owns(&self, file: &Arc<File>) -> bool {
        self.budget.as_ref().is_some_and(|b| Arc::ptr_eq(b, &file.budget))
    }
    pub fn usage(&self) -> Usage {
        let mut files = 0; let mut bytes = 4; // serialized file-count header
        for (name, node) in self.entries() {
            if let AtomNode::File(file) = node {
                if !file.readonly { files += 1; bytes += 6 + name.len() + file.len(); }
            }
        }
        Usage { files, serialized_bytes: bytes, live_bytes: self.budget.as_ref().map_or(0, |b| b.load(Ordering::Relaxed)),
            dirty: self.revision != self.saved_revision, generation: self.generation,
            revision: self.revision, saved_revision: self.saved_revision }
    }
    pub fn mark_saved(&mut self, revision: u64, generation: u64) {
        self.saved_revision = revision; self.generation = generation;
    }
    pub fn open(&mut self, name: &str) -> Result<Arc<File>, FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        if let Some(file) = self.file(name) { return Ok(file); }
        if builtin(name) { return Err(FsError::ReadOnly); }
        let usage = self.usage();
        if usage.files >= MAX_FILES || usage.serialized_bytes + 6 + name.len() > MAX_SNAPSHOT_BYTES {
            return Err(FsError::NoSpace);
        }
        self.entries_mut().try_reserve(1).map_err(|_| FsError::Memory)?;
        let file = self.make_file(Vec::new(), false);
        self.entries_mut().push((String::from(name), AtomNode::File(file.clone())));
        self.changed(); Ok(file)
    }
    pub fn open_existing(&self, name: &str) -> Result<Arc<File>, FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        self.file(name).ok_or(FsError::NotFound)
    }
    pub fn insert_builtin(&mut self, name: &str, bytes: &[u8]) -> Result<(), FsError> {
        if !builtin(name) { return Err(FsError::InvalidName); }
        if self.file(name).is_some() { return Err(FsError::Exists); }
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
    pub fn remove(&mut self, name: &str) -> Result<(), FsError> {
        if !valid_name(name) { return Err(FsError::InvalidName); }
        if builtin(name) { return Err(FsError::ReadOnly); }
        let index = self.entries().iter().position(|(key, _)| key == name).ok_or(FsError::NotFound)?;
        self.entries_mut().remove(index); // open handles retain their Arc
        self.changed(); Ok(())
    }
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        if !valid_name(old) || !valid_name(new) { return Err(FsError::InvalidName); }
        if builtin(old) || builtin(new) { return Err(FsError::ReadOnly); }
        let index = self.entries().iter().position(|(key, _)| key == old).ok_or(FsError::NotFound)?;
        if old == new { return Ok(()); }
        if self.file(new).is_some() { return Err(FsError::Exists); }
        if self.usage().serialized_bytes - old.len() + new.len() > MAX_SNAPSHOT_BYTES { return Err(FsError::NoSpace); }
        self.entries_mut()[index].0 = String::from(new);
        self.changed(); Ok(())
    }
    pub fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        self.entries().iter().filter_map(|(name, node)| match node {
            AtomNode::File(file) if !file.readonly => Some((name.clone(), file.with_bytes(|data| data.to_vec()))),
            _ => None,
        }).collect()
    }
    pub fn restored(files: Vec<(String, Vec<u8>)>, generation: u64) -> Result<Self, FsError> {
        let mut fs = Self::new();
        for (name, bytes) in files {
            if fs.file(&name).is_some() { return Err(FsError::Exists); }
            let file = fs.open(&name)?; fs.replace(&file, &bytes)?;
        }
        fs.mark_saved(fs.revision, generation);
        Ok(fs)
    }
}

pub static ROOT_FS: Spinlock<FileSystem> = Spinlock::new(FileSystem::new());
