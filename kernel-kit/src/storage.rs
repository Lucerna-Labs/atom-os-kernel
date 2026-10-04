//! Copy-on-write file storage over a block device ("ATOMFS02").
//!
//! The disk is an array of 4 KiB blocks. Blocks 0 and 1 hold two superblocks; the
//! newest valid one names the current generation's metadata (the folder tree, each
//! file's size, checksum and extents). A save never overwrites anything the current
//! generation uses:
//!
//! 1. changed files are written to free blocks, then FLUSH;
//! 2. the new metadata is written to free blocks, then FLUSH;
//! 3. the superblock slot the current generation does not use is written, then FLUSH.
//!
//! A crash at any point leaves the previous generation intact. Only files changed
//! since the last save are written, so a save costs what changed, not the whole disk.
//! File contents are read on first open and checked against their saved checksum.
//!
//! A disk in the earlier two-slot snapshot format ("ATOMFS01") is imported on mount;
//! its area is left untouched until the first ATOMFS02 save completes. Both of its
//! payload versions import: v1 (flat root files) and v2 (the "ATOMFST2" folder tree).
use alloc::{string::String, vec::Vec};
use crate::fs::{Content, Extent, File, FileData, Fs, FsError, Ino, Kind, Node, FILE_MAX, PAGE, ROOT};
use crate::memory::Spinlock;
use crate::virtio_blk::{BlockDevice, DiskError, VirtioBlock};

pub const BLOCK: usize = PAGE;
const MAGIC: &[u8; 8] = b"ATOMFS02";
const VERSION: u32 = 2;
/// Smallest usable disk: superblocks plus a little room (256 KiB).
pub const MIN_BLOCKS: u64 = 64;
/// Largest metadata block: 64 MiB.
const META_MAX: usize = 64 << 20;
/// Blocks moved per device request when saving or loading.
const BATCH: usize = 32;

pub fn checksum_update(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
pub const CHECKSUM_START: u64 = 0xcbf29ce484222325;
pub fn checksum(bytes: &[u8]) -> u64 { checksum_update(CHECKSUM_START, bytes) }

fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn u64_at(bytes: &[u8], at: usize) -> u64 { u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) }

/// One bit per disk block: set = in use.
#[derive(Clone)]
pub struct Bitmap { words: Vec<u64>, blocks: u64 }
impl Bitmap {
    pub fn new(blocks: u64) -> Self { Self { words: alloc::vec![0; blocks.div_ceil(64) as usize], blocks } }
    pub fn get(&self, b: u64) -> bool { self.words[(b / 64) as usize] & 1 << (b % 64) != 0 }
    pub fn set(&mut self, b: u64) { self.words[(b / 64) as usize] |= 1 << (b % 64); }
    pub fn count(&self) -> u64 { self.words.iter().map(|w| w.count_ones() as u64).sum() }
    /// Marks a run, failing if any block is outside the disk or already marked.
    fn claim(&mut self, start: u64, count: u64) -> Result<(), DiskError> {
        let end = start.checked_add(count).ok_or(DiskError::Corrupt)?;
        if end > self.blocks { return Err(DiskError::Corrupt); }
        for b in start..end { if self.get(b) { return Err(DiskError::Corrupt); } self.set(b); }
        Ok(())
    }
    /// Takes `count` free blocks as few runs as possible, first fit; or `Full`.
    fn allocate(&mut self, mut count: u64, contiguous: bool) -> Result<Vec<Extent>, DiskError> {
        let mut out = Vec::new();
        let mut b = 0;
        while count > 0 {
            while b < self.blocks && self.get(b) {
                // Skip whole words of used blocks quickly.
                if b % 64 == 0 && self.words[(b / 64) as usize] == u64::MAX { b += 64; } else { b += 1; }
            }
            if b >= self.blocks { return Err(DiskError::Full); }
            let start = b;
            while b < self.blocks && !self.get(b) && b - start < count.min(u32::MAX as u64) { b += 1; }
            let run = b - start;
            if contiguous && run < count { continue; }
            for x in start..b { self.set(x); }
            out.push(Extent { start, count: run as u32 });
            count -= run;
        }
        Ok(out)
    }
}

/// A saved node as the metadata describes it.
struct Entry { id: u32, parent: u32, dir: bool, name: String, modified: u64, size: u64, checksum: u64, extents: Vec<Extent> }

fn encode(fs: &Fs, pending: &[(Ino, Vec<Extent>, u64)]) -> Result<Vec<u8>, DiskError> {
    let order = fs.walk();
    let mut out = Vec::new();
    let mut count = 0u32;
    out.extend_from_slice(&0u32.to_le_bytes());
    for &ino in &order {
        let node = fs.node(ino).ok_or(DiskError::Corrupt)?;
        if node.system && !node.is_dir() { continue; } // Boot-image programs are never saved.
        let (dir, size, checksum, extents) = match &node.kind {
            Kind::Dir(_) => (true, 0, 0, &[][..]),
            Kind::File(file) => match pending.iter().find(|(i, _, _)| *i == ino) {
                Some((_, extents, sum)) => (false, file.size, *sum, extents.as_slice()),
                None => (false, file.size, file.checksum, file.extents.as_slice()),
            },
        };
        out.extend_from_slice(&ino.to_le_bytes());
        out.extend_from_slice(&node.parent.to_le_bytes());
        out.push(dir as u8);
        out.extend_from_slice(&(node.name.len() as u16).to_le_bytes());
        out.extend_from_slice(node.name.as_bytes());
        out.extend_from_slice(&node.modified.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&checksum.to_le_bytes());
        out.extend_from_slice(&(extents.len() as u32).to_le_bytes());
        for e in extents { out.extend_from_slice(&e.start.to_le_bytes()); out.extend_from_slice(&e.count.to_le_bytes()); }
        count += 1;
        if out.len() > META_MAX { return Err(DiskError::Full); }
    }
    out[..4].copy_from_slice(&count.to_le_bytes());
    Ok(out)
}

/// Parses metadata and checks it against the disk: every parent is a folder listed
/// earlier, names are valid and unique per folder, and extents are inside the disk,
/// match the file size and never overlap (each block claimed in `used`).
fn decode(bytes: &[u8], used: &mut Bitmap) -> Result<Vec<Entry>, DiskError> {
    let corrupt = DiskError::Corrupt;
    if bytes.len() < 4 { return Err(corrupt); }
    let count = u32_at(bytes, 0) as usize;
    let mut at = 4;
    let mut entries: Vec<Entry> = Vec::new();
    let take = |at: &mut usize, n: usize| -> Result<usize, DiskError> {
        let start = *at;
        *at = at.checked_add(n).filter(|&e| e <= bytes.len()).ok_or(corrupt)?;
        Ok(start)
    };
    for _ in 0..count {
        let h = take(&mut at, 11)?;
        let (id, parent, dir) = (u32_at(bytes, h), u32_at(bytes, h + 4), bytes[h + 8]);
        let name_len = u16::from_le_bytes([bytes[h + 9], bytes[h + 10]]) as usize;
        let n = take(&mut at, name_len)?;
        let name = core::str::from_utf8(&bytes[n..n + name_len]).map_err(|_| corrupt)?;
        let f = take(&mut at, 28)?;
        let (modified, size, sum) = (u64_at(bytes, f), u64_at(bytes, f + 8), u64_at(bytes, f + 16));
        let extent_count = u32_at(bytes, f + 24) as usize;
        if dir > 1 || id == ROOT || !crate::fs::valid_name(name) || size > FILE_MAX { return Err(corrupt); }
        if entries.iter().any(|e| e.id == id) { return Err(corrupt); }
        if parent != ROOT && !entries.iter().any(|e| e.id == parent && e.dir) { return Err(corrupt); }
        if entries.iter().any(|e| e.parent == parent && e.name == name) { return Err(corrupt); }
        let mut extents = Vec::new();
        let mut blocks = 0u64;
        for _ in 0..extent_count {
            let x = take(&mut at, 12)?;
            let e = Extent { start: u64_at(bytes, x), count: u32_at(bytes, x + 8) };
            if e.count == 0 { return Err(corrupt); }
            used.claim(e.start, e.count as u64)?;
            blocks += e.count as u64;
            extents.push(e);
        }
        if (dir == 1 && (size != 0 || !extents.is_empty())) || blocks != size.div_ceil(BLOCK as u64) { return Err(corrupt); }
        entries.push(Entry { id, parent, dir: dir == 1, name: String::from(name), modified, size, checksum: sum, extents });
    }
    if at != bytes.len() { return Err(corrupt); }
    Ok(entries)
}

struct Super { generation: u64, total: u64, meta_start: u64, meta_len: u64, meta_sum: u64 }

fn read_super(block: &[u8]) -> Option<Super> {
    if &block[..8] != MAGIC || u32_at(block, 8) != VERSION || u32_at(block, 12) != BLOCK as u32 { return None; }
    if checksum(&block[..64]) != u64_at(block, 64) { return None; }
    Some(Super { generation: u64_at(block, 16), total: u64_at(block, 24), meta_start: u64_at(block, 32),
                 meta_len: u64_at(block, 40), meta_sum: u64_at(block, 48) })
}

fn write_super(s: &Super, used: u64) -> Vec<u8> {
    let mut b = alloc::vec![0u8; BLOCK];
    b[..8].copy_from_slice(MAGIC);
    b[8..12].copy_from_slice(&VERSION.to_le_bytes());
    b[12..16].copy_from_slice(&(BLOCK as u32).to_le_bytes());
    b[16..24].copy_from_slice(&s.generation.to_le_bytes());
    b[24..32].copy_from_slice(&s.total.to_le_bytes());
    b[32..40].copy_from_slice(&s.meta_start.to_le_bytes());
    b[40..48].copy_from_slice(&s.meta_len.to_le_bytes());
    b[48..56].copy_from_slice(&s.meta_sum.to_le_bytes());
    b[56..64].copy_from_slice(&used.to_le_bytes());
    let hash = checksum(&b[..64]);
    b[64..72].copy_from_slice(&hash.to_le_bytes());
    b
}

pub struct Store<D: BlockDevice> {
    pub device: D,
    pub generation: u64,
    total: u64,
    /// Blocks the current (saved) generation uses, plus any protected legacy area.
    used: Bitmap,
    /// Metadata of the current generation, which a save must not overwrite.
    meta: Option<Extent>,
}

impl<D: BlockDevice> Store<D> {
    /// Mounts the disk into `fs`. A blank disk mounts empty; an ATOMFS01 disk is
    /// imported (its files are saved in the new format by the next save).
    pub fn open(mut device: D, fs: &mut Fs) -> Result<Self, DiskError> {
        let total = device.sectors() / (BLOCK as u64 / 512);
        if total < MIN_BLOCKS { return Err(DiskError::Bounds); }
        let mut header = alloc::vec![0u8; BLOCK];
        let mut best: Option<(Super, Vec<Entry>, Bitmap)> = None;
        let mut blank = true;
        for slot in 0..2 {
            device.read_blocks(slot, &mut header)?;
            if header.iter().any(|&b| b != 0) { blank = false; }
            let Some(s) = read_super(&header) else { continue };
            if s.total != total || s.generation == 0 || best.as_ref().is_some_and(|(b, _, _)| b.generation >= s.generation) { continue; }
            let blocks = s.meta_len.div_ceil(BLOCK as u64);
            if s.meta_len < 4 || s.meta_len > META_MAX as u64 || s.meta_start < 2 { continue; }
            let mut used = Bitmap::new(total);
            used.set(0); used.set(1);
            if used.claim(s.meta_start, blocks).is_err() { continue; }
            let mut meta = alloc::vec![0u8; (blocks as usize) * BLOCK];
            device.read_blocks(s.meta_start, &mut meta)?;
            meta.truncate(s.meta_len as usize);
            if checksum(&meta) != s.meta_sum { continue; }
            if let Ok(entries) = decode(&meta, &mut used) { best = Some((s, entries, used)); }
        }
        if let Some((s, entries, used)) = best {
            // Map saved ids to new inode numbers as nodes are inserted (parents first).
            let mut ids: Vec<(u32, Ino)> = alloc::vec![(ROOT, ROOT)];
            for e in entries {
                let parent = ids.iter().find(|(id, _)| *id == e.parent).map(|&(_, i)| i).ok_or(DiskError::Corrupt)?;
                let kind = if e.dir { Kind::Dir(Vec::new()) } else {
                    let content = if e.size == 0 { Content::Data(FileData::new()) } else { Content::Unloaded };
                    Kind::File(File { content, size: e.size, extents: e.extents, checksum: e.checksum, dirty: false })
                };
                let ino = fs.insert(parent, &e.name, e.modified, kind).map_err(|_| DiskError::Corrupt)?;
                fs.blocks += e.size.div_ceil(BLOCK as u64);
                ids.push((e.id, ino));
            }
            fs.meta_dirty = false;
            fs.saved_revision = fs.revision;
            fs.capacity = Some(Self::capacity_for(total));
            let meta = Extent { start: s.meta_start, count: s.meta_len.div_ceil(BLOCK as u64) as u32 };
            return Ok(Self { device, generation: s.generation, total, used, meta: Some(meta) });
        }
        let mut used = Bitmap::new(total);
        used.set(0); used.set(1);
        // The earlier format's second slot header lies past the superblocks.
        let legacy_slot = legacy::PAYLOAD_SECTORS + 1;
        if blank && device.sectors() > legacy_slot {
            let mut sector = [0u8; 512];
            device.read_sector(legacy_slot, &mut sector)?;
            if sector.iter().any(|&b| b != 0) { blank = false; }
        }
        if !blank {
            // Not ATOMFS02: an ATOMFS01 disk is imported, anything else is refused.
            let (journal, files) = legacy::Journal::open(device).map_err(|e| match e { DiskError::Bounds => DiskError::Corrupt, e => e })?;
            if journal.generation == 0 { return Err(DiskError::Corrupt); }
            device = journal.device;
            for b in 0..legacy::REQUIRED_SECTORS.div_ceil(BLOCK as u64 / 512).min(total) { used.set(b); }
            for (name, bytes) in files {
                // A v2 path names its folders ("a/b/c.txt"); a trailing '/' is a folder.
                let directory = name.ends_with('/');
                let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
                let Some((&leaf, folders)) = parts.split_last() else { return Err(DiskError::Corrupt) };
                let mut parent = ROOT;
                for folder in folders.iter().chain(if directory { core::slice::from_ref(&leaf) } else { &[] }) {
                    parent = match fs.lookup(parent, folder) {
                        Some(ino) if fs.node(ino).is_some_and(Node::is_dir) => ino,
                        Some(_) => return Err(DiskError::Corrupt),
                        None => fs.insert(parent, folder, 0, Kind::Dir(Vec::new())).map_err(|_| DiskError::Corrupt)?,
                    };
                }
                if directory { continue; }
                let mut data = FileData::new();
                data.write_at(0, &bytes).map_err(|_| DiskError::Memory)?;
                let size = data.len();
                let file = File { content: Content::Data(data), size, extents: Vec::new(), checksum: 0, dirty: true };
                fs.insert(parent, leaf, 0, Kind::File(file)).map_err(|_| DiskError::Corrupt)?;
                fs.blocks += size.div_ceil(BLOCK as u64);
            }
            fs.meta_dirty = true;
        }
        fs.capacity = Some(Self::capacity_for(total));
        Ok(Self { device, generation: 0, total, used, meta: None })
    }

    /// Blocks files may use: the disk minus the superblocks and room for metadata, and
    /// minus a margin so a save that rewrites files can still find free space.
    fn capacity_for(total: u64) -> u64 { (total - 2) * 15 / 16 }

    pub fn total_blocks(&self) -> u64 { self.total }
    pub fn used_blocks(&self) -> u64 { self.used.count() }

    /// Reads a saved file's contents into memory and checks them against the
    /// checksum recorded when they were saved.
    pub fn load(&mut self, file: &mut File) -> Result<(), DiskError> {
        if !matches!(file.content, Content::Unloaded) { return Ok(()); }
        let mut data = FileData::new();
        let mut hash = CHECKSUM_START;
        let mut offset = 0u64;
        let mut buffer = alloc::vec![0u8; BATCH * BLOCK];
        for e in &file.extents {
            let mut done = 0u64;
            while done < e.count as u64 {
                let n = (e.count as u64 - done).min(BATCH as u64) as usize;
                let chunk = &mut buffer[..n * BLOCK];
                self.device.read_blocks(e.start + done, chunk)?;
                let keep = ((file.size - offset) as usize).min(chunk.len());
                hash = checksum_update(hash, &chunk[..keep]);
                data.write_at(offset, &chunk[..keep]).map_err(|_| DiskError::Memory)?;
                offset += keep as u64;
                done += n as u64;
            }
        }
        if offset != file.size || hash != file.checksum { return Err(DiskError::Corrupt); }
        file.content = Content::Data(data);
        Ok(())
    }

    /// Saves every change in `fs` as a new generation (see the module comment). On
    /// failure nothing in `fs` changes and the previous generation stays current.
    pub fn commit(&mut self, fs: &mut Fs) -> Result<(), DiskError> {
        let changed: Vec<Ino> = fs.walk().into_iter()
            .filter(|&i| fs.node(i).and_then(Node::file).is_some_and(|f| f.dirty && matches!(f.content, Content::Data(_))))
            .collect();
        if changed.is_empty() && !fs.meta_dirty && self.generation != 0 { return Ok(()); }
        let mut free = self.used.clone();
        let mut pending: Vec<(Ino, Vec<Extent>, u64)> = Vec::new();
        let mut buffer = alloc::vec![0u8; BATCH * BLOCK];
        for &ino in &changed {
            let Some(Content::Data(data)) = fs.node(ino).and_then(Node::file).map(|f| &f.content) else { continue };
            let blocks = data.len().div_ceil(BLOCK as u64);
            let extents = free.allocate(blocks, false)?;
            // Stream the pages into the new extents, BATCH blocks per request.
            let mut targets = extents.iter().flat_map(|e| e.start..e.start + e.count as u64);
            let mut hash = CHECKSUM_START;
            let mut fill = 0usize;
            let mut first = 0u64;
            let mut result = Ok(());
            data.for_each_page(|page| {
                if result.is_err() { return; }
                hash = checksum_update(hash, page);
                let block = targets.next().unwrap();
                if fill > 0 && block != first + fill as u64 {
                    result = self.device.write_blocks(first, &buffer[..fill * BLOCK]);
                    fill = 0;
                }
                if fill == 0 { first = block; }
                buffer[fill * BLOCK..fill * BLOCK + page.len()].copy_from_slice(page);
                buffer[fill * BLOCK + page.len()..(fill + 1) * BLOCK].fill(0);
                fill += 1;
                if fill == BATCH {
                    result = self.device.write_blocks(first, &buffer[..fill * BLOCK]);
                    fill = 0;
                }
            });
            result?;
            if fill > 0 { self.device.write_blocks(first, &buffer[..fill * BLOCK])?; }
            pending.push((ino, extents, hash));
        }
        self.device.flush()?;
        let meta = encode(fs, &pending)?;
        let meta_blocks = meta.len().div_ceil(BLOCK) as u64;
        let at = free.allocate(meta_blocks, true)?[0].start;
        let mut padded = meta.clone();
        padded.resize(meta_blocks as usize * BLOCK, 0);
        self.device.write_blocks(at, &padded)?;
        self.device.flush()?;
        // The new generation's block usage, recomputed from what it references.
        let mut used = Bitmap::new(self.total);
        used.set(0); used.set(1);
        used.claim(at, meta_blocks)?;
        for ino in fs.walk() {
            let Some(file) = fs.node(ino).and_then(Node::file) else { continue };
            if fs.node(ino).is_some_and(|n| n.system) { continue; }
            let extents = pending.iter().find(|(i, _, _)| *i == ino).map_or(&file.extents, |(_, e, _)| e);
            for e in extents { used.claim(e.start, e.count as u64)?; }
        }
        let generation = self.generation.checked_add(1).ok_or(DiskError::Full)?;
        let s = Super { generation, total: self.total, meta_start: at, meta_len: meta.len() as u64, meta_sum: checksum(&meta) };
        self.device.write_blocks(generation % 2, &write_super(&s, used.count()))?;
        self.device.flush()?;
        // Durable: adopt the new generation.
        for (ino, extents, sum) in pending {
            if let Some(file) = fs.file_mut(ino) { file.extents = extents; file.checksum = sum; file.dirty = false; }
        }
        fs.meta_dirty = false;
        fs.saved_revision = fs.revision;
        self.generation = generation;
        self.used = used;
        self.meta = Some(Extent { start: at, count: meta_blocks as u32 });
        Ok(())
    }
}

static STORE: Spinlock<Option<Store<VirtioBlock>>> = Spinlock::new(None);

/// Mounts the virtio data disk into the root file system; returns its generation.
pub fn mount() -> Result<u64, DiskError> {
    let device = VirtioBlock::discover()?;
    let fs = crate::fs::ROOT_FS.lock();
    let result = Store::open(device, fs);
    crate::fs::ROOT_FS.unlock();
    let store = result?;
    let generation = store.generation;
    *STORE.lock() = Some(store);
    STORE.unlock();
    Ok(generation)
}

/// Saves all changes. Call without holding `ROOT_FS`.
pub fn sync() -> Result<(), DiskError> {
    let fs = crate::fs::ROOT_FS.lock();
    let store = STORE.lock();
    let result = store.as_mut().ok_or(DiskError::Missing).and_then(|s| s.commit(fs));
    STORE.unlock();
    crate::fs::ROOT_FS.unlock();
    result
}

/// Makes a saved file's contents available; call with `ROOT_FS` held (as `fs`).
pub fn ensure_loaded(fs: &mut Fs, ino: Ino) -> Result<(), FsError> {
    let Some(file) = fs.file_mut(ino) else { return Ok(()) };
    if !matches!(file.content, Content::Unloaded) { return Ok(()); }
    let store = STORE.lock();
    let result = store.as_mut().ok_or(FsError::Io).and_then(|s| s.load(file).map_err(|_| FsError::Io));
    STORE.unlock();
    result
}

/// True when a data disk is mounted and its device still answers.
pub fn available() -> bool {
    let store = STORE.lock();
    let result = store.as_ref().is_some_and(|s| s.device.is_online());
    STORE.unlock();
    result
}

/// The saved generation of the data disk (0 before the first save or without one).
pub fn generation() -> u64 {
    let store = STORE.lock();
    let result = store.as_ref().map_or(0, |s| s.generation);
    STORE.unlock();
    result
}

/// (total blocks, blocks used by the saved generation, block size) of the data disk.
pub fn info() -> Option<(u64, u64)> {
    let store = STORE.lock();
    let result = store.as_ref().map(|s| (s.total_blocks(), s.used_blocks()));
    STORE.unlock();
    result
}

/// The earlier format: two checksummed snapshot slots of up to 128 files of at most
/// 64 KiB each, flat (payload v1) or in folders (payload v2, "ATOMFST2"). Kept to
/// import such disks (and to build them in tests).
pub mod legacy {
    use alloc::{string::String, vec::Vec};
    use crate::virtio_blk::{BlockDevice, DiskError};

    const MAGIC: &[u8; 8] = b"ATOMFS01";
    const TREE_MAGIC: &[u8; 8] = b"ATOMFST2";
    pub const PAYLOAD_SECTORS: u64 = 1024;
    pub const REQUIRED_SECTORS: u64 = (PAYLOAD_SECTORS + 1) * 2;
    pub type Files = Vec<(String, Vec<u8>)>;

    fn checksum(bytes: &[u8]) -> u64 { super::checksum(bytes) }
    fn u32_at(bytes: &[u8], at: usize) -> u32 { super::u32_at(bytes, at) }
    fn u64_at(bytes: &[u8], at: usize) -> u64 { super::u64_at(bytes, at) }
    fn valid(name: &str) -> bool { !name.is_empty() && name.len() <= 63 && !name.bytes().any(|b| b < 32 || b == b'/') }

    pub fn encode(files: &Files) -> Result<Vec<u8>, DiskError> {
        if files.len() > 128 { return Err(DiskError::Full); }
        let mut bytes = (files.len() as u32).to_le_bytes().to_vec();
        for (index, (name, data)) in files.iter().enumerate() {
            if !valid(name) || data.len() > 65536 || files[..index].iter().any(|(old, _)| old == name) {
                return Err(DiskError::Corrupt);
            }
            if bytes.len() + 6 + name.len() + data.len() > PAYLOAD_SECTORS as usize * 512 { return Err(DiskError::Full); }
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes()); bytes.extend_from_slice(data);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Files, DiskError> {
        if bytes.len() < 4 || bytes.len() > PAYLOAD_SECTORS as usize * 512 { return Err(DiskError::Corrupt); }
        let count = u32_at(bytes, 0) as usize;
        if count > 128 { return Err(DiskError::Corrupt); }
        let mut cursor = 4;
        let mut files: Files = Vec::new();
        for _ in 0..count {
            if cursor + 6 > bytes.len() { return Err(DiskError::Corrupt); }
            let name_len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
            let data_len = u32_at(bytes, cursor + 2) as usize;
            cursor += 6;
            if name_len == 0 || name_len > 63 || data_len > 65536 || name_len + data_len > bytes.len() - cursor { return Err(DiskError::Corrupt); }
            let name = core::str::from_utf8(&bytes[cursor..cursor + name_len]).map_err(|_| DiskError::Corrupt)?;
            if !valid(name) || files.iter().any(|(old, _)| old == name) { return Err(DiskError::Corrupt); }
            cursor += name_len;
            files.push((String::from(name), bytes[cursor..cursor + data_len].to_vec()));
            cursor += data_len;
        }
        if cursor != bytes.len() { return Err(DiskError::Corrupt); }
        Ok(files)
    }

    /// Decodes a v2 payload: TREE_MAGIC, a u32 entry count, then per entry a u8 kind
    /// (1 = folder, 2 = file), a u16 path length, a u32 data length (files only) and
    /// the root-relative path. Folders come back with a trailing '/'.
    pub fn decode_tree(bytes: &[u8]) -> Result<Files, DiskError> {
        if bytes.len() < 12 || &bytes[..8] != TREE_MAGIC { return Err(DiskError::Corrupt); }
        let count = u32_at(bytes, 8) as usize;
        if count > (bytes.len() - 12) / 3 { return Err(DiskError::Corrupt); }
        let mut cursor = 12;
        let mut files: Files = Vec::new();
        for _ in 0..count {
            if cursor + 3 > bytes.len() { return Err(DiskError::Corrupt); }
            let kind = bytes[cursor];
            let name_len = u16::from_le_bytes([bytes[cursor + 1], bytes[cursor + 2]]) as usize;
            cursor += 3;
            let data_len = match kind {
                1 => 0,
                2 => {
                    if cursor + 4 > bytes.len() { return Err(DiskError::Corrupt); }
                    let len = u32_at(bytes, cursor) as usize;
                    cursor += 4;
                    len
                }
                _ => return Err(DiskError::Corrupt),
            };
            if name_len == 0 || data_len > 65536 || name_len + data_len > bytes.len() - cursor { return Err(DiskError::Corrupt); }
            let name = core::str::from_utf8(&bytes[cursor..cursor + name_len]).map_err(|_| DiskError::Corrupt)?;
            let body = name.strip_suffix('/').unwrap_or(name);
            if (kind == 1) != name.ends_with('/') || body.is_empty()
                || !body.split('/').all(|p| valid(p) && p != "." && p != "..")
                || files.iter().any(|(old, _)| old == name) { return Err(DiskError::Corrupt); }
            cursor += name_len;
            files.push((String::from(name), bytes[cursor..cursor + data_len].to_vec()));
            cursor += data_len;
        }
        if cursor != bytes.len() { return Err(DiskError::Corrupt); }
        Ok(files)
    }

    pub struct Journal<D: BlockDevice> { pub device: D, pub generation: u64 }
    impl<D: BlockDevice> Journal<D> {
        pub fn open(mut device: D) -> Result<(Self, Files), DiskError> {
            if device.sectors() < REQUIRED_SECTORS { return Err(DiskError::Bounds); }
            let mut latest: Option<(u64, Files)> = None;
            let mut blank = true;
            for slot in 0..2 {
                let base = slot * (PAYLOAD_SECTORS + 1);
                let mut header = [0; 512]; device.read_sector(base, &mut header)?;
                if header.iter().all(|&b| b == 0) { continue; }
                blank = false;
                let version = u32_at(&header, 8);
                if &header[..8] != MAGIC || (version != 1 && version != 2) || checksum(&header[..40]) != u64_at(&header, 40) { continue; }
                let len = u32_at(&header, 12) as usize;
                let generation = u64_at(&header, 16);
                if len < 4 || len > PAYLOAD_SECTORS as usize * 512 || generation == 0 { continue; }
                let mut payload = alloc::vec![0; len];
                for index in 0..len.div_ceil(512) {
                    let mut sector = [0; 512]; device.read_sector(base + 1 + index as u64, &mut sector)?;
                    let count = (len - index * 512).min(512);
                    payload[index * 512..index * 512 + count].copy_from_slice(&sector[..count]);
                }
                if checksum(&payload) != u64_at(&header, 24) { continue; }
                let decoded = if version == 2 { decode_tree(&payload) } else { decode(&payload) };
                if let Ok(files) = decoded {
                    if files.len() != u32_at(&header, 32) as usize { continue; }
                    if latest.as_ref().is_none_or(|(old, _)| generation > *old) { latest = Some((generation, files)); }
                }
            }
            match latest {
                Some((generation, files)) => Ok((Self { device, generation }, files)),
                None if blank => Ok((Self { device, generation: 0 }, Vec::new())),
                None => Err(DiskError::Corrupt),
            }
        }
        pub fn commit(&mut self, files: &Files) -> Result<(), DiskError> {
            let payload = encode(files)?;
            let generation = self.generation.checked_add(1).ok_or(DiskError::Full)?;
            let base = (generation % 2) * (PAYLOAD_SECTORS + 1);
            for (index, chunk) in payload.chunks(512).enumerate() {
                let mut sector = [0; 512]; sector[..chunk.len()].copy_from_slice(chunk);
                self.device.write_sector(base + 1 + index as u64, &sector)?;
            }
            self.device.flush()?;
            let mut header = [0; 512];
            header[..8].copy_from_slice(MAGIC);
            header[8..12].copy_from_slice(&1u32.to_le_bytes());
            header[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            header[16..24].copy_from_slice(&generation.to_le_bytes());
            header[24..32].copy_from_slice(&checksum(&payload).to_le_bytes());
            header[32..36].copy_from_slice(&(files.len() as u32).to_le_bytes());
            let hash = checksum(&header[..40]); header[40..48].copy_from_slice(&hash.to_le_bytes());
            self.device.write_sector(base, &header)?;
            self.device.flush()?;
            self.generation = generation;
            Ok(())
        }
    }
}
