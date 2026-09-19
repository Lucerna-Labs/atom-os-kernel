//! Two-slot, checksummed snapshots over a real block device. Commit ordering:
//! write inactive payload -> FLUSH -> write header -> FLUSH -> publish generation.
//!
//! Payload format v2 (tree): the 8-byte TREE_MAGIC prefix, a u32 entry count,
//! then per entry a u8 kind (1 = directory, 2 = file), a u16 name length,
//! a u32 data length (files only), and the root-relative path bytes. In the
//! `Files` surface a directory is an entry whose name ends in '/' with empty
//! data (file names can never contain '/', so the marker is unambiguous).
//! A v1 payload began with a u32 file count <= 128 — byte 1 is always zero,
//! so TREE_MAGIC ('T' at byte 1) can never alias one; v1 images load as
//! root-level files and migrate on the next commit.
use alloc::{string::String, vec::Vec};
use crate::virtio_blk::{BlockDevice, DiskError, VirtioBlock};
use crate::memory::Spinlock;

const MAGIC: &[u8; 8] = b"ATOMFS01";
const TREE_MAGIC: &[u8; 8] = b"ATOMFST2";
const FORMAT_VERSION: u32 = 2;
const MAX_PATH_BYTES: usize = crate::fs::MAX_NAME_BYTES * crate::fs::MAX_DEPTH + crate::fs::MAX_DEPTH;
pub const PAYLOAD_SECTORS: u64 = (crate::fs::MAX_SNAPSHOT_BYTES / 512) as u64;
pub const REQUIRED_SECTORS: u64 = (PAYLOAD_SECTORS + 1) * 2;
pub type Files = Vec<(String, Vec<u8>)>;

fn checksum(bytes: &[u8]) -> u64 {
    crate::atoms::fold(bytes.iter(), 0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}
fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn u64_at(bytes: &[u8], at: usize) -> u64 { u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) }
pub use crate::fs::builtin;

fn warn_line(message: &[u8]) {
    let (serial, flags) = crate::serial::SERIAL1.lock();
    for byte in message { serial.send(*byte); }
    crate::serial::SERIAL1.unlock(flags);
}

pub fn encode(files: &Files) -> Result<Vec<u8>, DiskError> {
    let mut writable = 0;
    for (index, (name, data)) in files.iter().enumerate() {
        let (segments, directory) = crate::fs::snapshot_segments(name).map_err(|_| DiskError::Corrupt)?;
        if directory {
            if !data.is_empty() { return Err(DiskError::Corrupt); }
        } else if data.len() > crate::fs::MAX_FILE_BYTES { return Err(DiskError::Corrupt); }
        else { writable += 1; }
        if segments.len() == 1 && builtin(&segments[0]) { return Err(DiskError::Corrupt); }
        if files[..index].iter().any(|(old, _)| old == name) { return Err(DiskError::Corrupt); }
    }
    if writable > crate::fs::MAX_FILES { return Err(DiskError::Full); }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(TREE_MAGIC);
    bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (name, data) in files {
        let directory = name.ends_with('/');
        if bytes.len() + 3 + name.len() + if directory { 0 } else { 4 + data.len() }
            > PAYLOAD_SECTORS as usize * 512 { return Err(DiskError::Full); }
        bytes.push(if directory { 1 } else { 2 });
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        if !directory { bytes.extend_from_slice(&(data.len() as u32).to_le_bytes()); }
        bytes.extend_from_slice(name.as_bytes()); bytes.extend_from_slice(data);
    }
    Ok(bytes)
}

/// Decode a legacy (v1) flat payload: bare root-level names, no directories.
pub fn decode(bytes: &[u8]) -> Result<Files, DiskError> {
    if bytes.len() < 4 || bytes.len() > PAYLOAD_SECTORS as usize * 512 { return Err(DiskError::Corrupt); }
    let count = u32_at(bytes, 0) as usize;
    if count > crate::fs::MAX_FILES { return Err(DiskError::Corrupt); }
    let mut cursor = 4;
    let mut files: Files = Vec::new();
    for _ in 0..count {
        if cursor + 6 > bytes.len() { return Err(DiskError::Corrupt); }
        let name_len = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap()) as usize;
        let data_len = u32_at(bytes, cursor + 2) as usize;
        cursor += 6;
        if name_len == 0 || name_len > crate::fs::MAX_NAME_BYTES || data_len > crate::fs::MAX_FILE_BYTES || name_len + data_len > bytes.len() - cursor { return Err(DiskError::Corrupt); }
        let name = core::str::from_utf8(&bytes[cursor..cursor + name_len]).map_err(|_| DiskError::Corrupt)?;
        if name.bytes().any(|b| b < 32 || b == b'/') || builtin(name) || files.iter().any(|(old, _)| old == name) {
            return Err(DiskError::Corrupt);
        }
        cursor += name_len;
        files.push((String::from(name), bytes[cursor..cursor + data_len].to_vec()));
        cursor += data_len;
    }
    if cursor != bytes.len() { return Err(DiskError::Corrupt); }
    Ok(files)
}

/// Which payload generation an image came from — legacy flat (v1) or tree (v2).
enum DiskImage { Legacy(Files), Tree(Files) }
impl DiskImage {
    fn entry_count(&self) -> usize { match self { DiskImage::Legacy(files) | DiskImage::Tree(files) => files.len() } }
    fn files(self) -> Files { match self { DiskImage::Legacy(files) | DiskImage::Tree(files) => files } }
    fn is_legacy(&self) -> bool { matches!(self, DiskImage::Legacy(_)) }
}

fn decode_image(bytes: &[u8]) -> Result<DiskImage, DiskError> {
    if bytes.len() >= TREE_MAGIC.len() + 4 && &bytes[..TREE_MAGIC.len()] == TREE_MAGIC {
        Ok(DiskImage::Tree(decode_tree(bytes)?))
    } else {
        Ok(DiskImage::Legacy(decode(bytes)?))
    }
}

fn decode_tree(bytes: &[u8]) -> Result<Files, DiskError> {
    let count = u32_at(bytes, 8) as usize;
    // Every entry needs at least its 3 header bytes; count beyond that is corrupt.
    if count > bytes.len().saturating_sub(TREE_MAGIC.len() + 4) / 3 { return Err(DiskError::Corrupt); }
    let mut cursor = TREE_MAGIC.len() + 4;
    let mut files: Files = Vec::new();
    let mut writable = 0;
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
        if name_len == 0 || name_len > MAX_PATH_BYTES || data_len > crate::fs::MAX_FILE_BYTES
            || name_len + data_len > bytes.len() - cursor { return Err(DiskError::Corrupt); }
        let name = core::str::from_utf8(&bytes[cursor..cursor + name_len]).map_err(|_| DiskError::Corrupt)?;
        let (segments, directory) = crate::fs::snapshot_segments(name).map_err(|_| DiskError::Corrupt)?;
        if directory != (kind == 1) { return Err(DiskError::Corrupt); }
        if segments.len() == 1 && builtin(&segments[0]) { return Err(DiskError::Corrupt); }
        if files.iter().any(|(old, _)| old == name) { return Err(DiskError::Corrupt); }
        cursor += name_len;
        files.push((String::from(name), bytes[cursor..cursor + data_len].to_vec()));
        cursor += data_len;
        if kind == 2 { writable += 1; if writable > crate::fs::MAX_FILES { return Err(DiskError::Corrupt); } }
    }
    if cursor != bytes.len() { return Err(DiskError::Corrupt); }
    Ok(files)
}

pub struct Journal<D: BlockDevice> { pub device: D, pub generation: u64 }
impl<D: BlockDevice> Journal<D> {
    pub fn open(mut device: D) -> Result<(Self, Files), DiskError> {
        if device.sectors() < REQUIRED_SECTORS { return Err(DiskError::Bounds); }
        let mut latest: Option<(u64, DiskImage)> = None;
        let mut blank = true;
        for slot in 0..2 {
            let base = slot * (PAYLOAD_SECTORS + 1);
            let mut header = [0; 512]; device.read_sector(base, &mut header)?;
            if header.iter().all(|&b| b == 0) { continue; }
            blank = false;
            let version = u32_at(&header, 8);
            if &header[..8] != MAGIC || (version != 1 && version != FORMAT_VERSION)
                || checksum(&header[..40]) != u64_at(&header, 40) { continue; }
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
            if let Ok(image) = decode_image(&payload) {
                if image.entry_count() != u32_at(&header, 32) as usize { continue; }
                if latest.as_ref().is_none_or(|(old, _)| generation > *old) { latest = Some((generation, image)); }
            }
        }
        match latest {
            Some((generation, image)) => {
                // One-line serial note when the newest readable image is the
                // old flat format: its entries migrate to the tree root and
                // the next commit writes v2.
                if image.is_legacy() {
                    warn_line(b"[fs] v1 flat image loaded; entries migrate to the tree format\n");
                }
                Ok((Self { device, generation }, image.files()))
            }
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
        header[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
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

static STORE: Spinlock<Option<Journal<VirtioBlock>>> = Spinlock::new(None);

pub fn mount() -> Result<u64, DiskError> {
    let (journal, files) = Journal::open(VirtioBlock::discover()?)?;
    let generation = journal.generation;
    let restored = crate::fs::FileSystem::restored(files, generation).map_err(|_| DiskError::Corrupt)?;
    *crate::fs::ROOT_FS.lock() = restored;
    crate::fs::ROOT_FS.unlock();
    *STORE.lock() = Some(journal); STORE.unlock();
    Ok(generation)
}
pub fn available() -> bool {
    let result = STORE.lock().as_ref().is_some_and(|store| store.device.is_online());
    STORE.unlock(); result
}
pub fn sync() -> Result<(), DiskError> {
    let fs = crate::fs::ROOT_FS.lock();
    let revision = fs.usage().revision;
    let files = fs.snapshot();
    crate::fs::ROOT_FS.unlock();
    let store = STORE.lock();
    let result = store.as_mut().ok_or(DiskError::Missing).and_then(|store| {
        store.commit(&files)?;
        Ok(store.generation)
    });
    STORE.unlock();
    match result {
        Ok(generation) => {
            crate::fs::ROOT_FS.lock().mark_saved(revision, generation);
            crate::fs::ROOT_FS.unlock();
            Ok(())
        }
        Err(error) => Err(error),
    }
}
