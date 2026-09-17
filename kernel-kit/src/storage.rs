//! Two-slot, checksummed snapshots over a real block device. Commit ordering:
//! write inactive payload -> FLUSH -> write header -> FLUSH -> publish generation.
use alloc::{string::String, vec::Vec};
use crate::virtio_blk::{BlockDevice, DiskError, VirtioBlock};
use crate::memory::Spinlock;

const MAGIC: &[u8; 8] = b"ATOMFS01";
pub const PAYLOAD_SECTORS: u64 = 1024;
pub const REQUIRED_SECTORS: u64 = (PAYLOAD_SECTORS + 1) * 2;
pub type Files = Vec<(String, Vec<u8>)>;

fn checksum(bytes: &[u8]) -> u64 {
    crate::atoms::fold(bytes.iter(), 0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}
fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn u64_at(bytes: &[u8], at: usize) -> u64 { u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) }
pub fn builtin(name: &str) -> bool { matches!(name, "shell.elf" | "daemon.elf" | "worker.elf" | "fault.elf") }

pub fn encode(files: &Files) -> Result<Vec<u8>, DiskError> {
    if files.len() > 128 { return Err(DiskError::Full); }
    let mut bytes = (files.len() as u32).to_le_bytes().to_vec();
    for (index, (name, data)) in files.iter().enumerate() {
        if name.is_empty() || name.len() > 63 || name.bytes().any(|b| b < 32 || b == b'/')
            || builtin(name) || data.len() > 65536 || files[..index].iter().any(|(old, _)| old == name) {
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
            if &header[..8] != MAGIC || u32_at(&header, 8) != 1 || checksum(&header[..40]) != u64_at(&header, 40) { continue; }
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
            if let Ok(files) = decode(&payload) {
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

static STORE: Spinlock<Option<Journal<VirtioBlock>>> = Spinlock::new(None);

pub fn mount() -> Result<u64, DiskError> {
    let (journal, files) = Journal::open(VirtioBlock::discover()?)?;
    let generation = journal.generation;
    let fs = crate::fs::ROOT_FS.lock();
    let result = (|| {
        for (name, contents) in files {
            let target = fs.get_or_create_file(&name).ok_or(DiskError::Full)?;
            unsafe { *target = contents; }
        }
        Ok(())
    })();
    crate::fs::ROOT_FS.unlock();
    result?;
    *STORE.lock() = Some(journal); STORE.unlock();
    Ok(generation)
}
pub fn sync() -> Result<(), DiskError> {
    let fs = crate::fs::ROOT_FS.lock();
    let files: Files = match fs {
        crate::fs::AtomNode::Directory(children) => children.iter().filter_map(|(name, node)| {
            if builtin(name) { return None; }
            if let crate::fs::AtomNode::File(bytes) = node { Some((name.clone(), (**bytes).clone())) } else { None }
        }).collect(),
        _ => Vec::new(),
    };
    crate::fs::ROOT_FS.unlock();
    let store = STORE.lock();
    let result = store.as_mut().ok_or(DiskError::Missing).and_then(|store| store.commit(&files));
    STORE.unlock();
    result
}
