//! Mathematical Virtual File System (RamFS)
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use crate::memory::Spinlock;

/// Read-only programs embedded in the boot image. They are never removed,
/// renamed or written to disk snapshots.
pub const BUILTINS: [&str; 6] = ["shell.elf", "daemon.elf", "worker.elf", "fault.elf", "sleeper.elf", "desktop.elf"];
pub fn builtin(name: &str) -> bool { BUILTINS.contains(&name) }
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 63 && !name.bytes().any(|b| b < 32 || b == b'/')
}

/// An abstract mathematical node representing data or a branch in the file system tree.
pub enum AtomNode {
    File(Box<Vec<u8>>),
    Directory(Vec<(String, AtomNode)>),
}

impl AtomNode {
    pub const fn new_dir() -> Self {
        AtomNode::Directory(Vec::new())
    }

    pub fn file(&self, filename: &str) -> Option<&Vec<u8>> {
        match self {
            Self::Directory(children) => children.iter().find_map(|(name, node)| {
                if name == filename { if let Self::File(data) = node { return Some(&**data); } }
                None
            }),
            _ => None,
        }
    }

    /// Searches for a file and returns its data buffer as a mutable reference.
    /// In a real OS, paths would be split by '/'. For this proof of concept, we support flat filenames in the root.
    pub fn get_or_create_file(&mut self, filename: &str) -> Option<*mut Vec<u8>> {
        if !valid_name(filename) { return None; }
        match self {
            AtomNode::Directory(children) => {
                // Find existing
                for (name, node) in children.iter_mut() {
                    if name == filename {
                        if let AtomNode::File(data) = node {
                            return Some(&mut **data as *mut Vec<u8>);
                        } else {
                            return None; // Exists but is a directory
                        }
                    }
                }
                
                // If not found, create a new file
                if children.len() >= 256 { return None; }
                children.push((String::from(filename), AtomNode::File(Box::new(Vec::new()))));
                if let AtomNode::File(data) = &mut children.last_mut().unwrap().1 {
                    Some(&mut **data as *mut Vec<u8>)
                } else {
                    None
                }
            },
            AtomNode::File(_) => None, // Cannot search inside a file
        }
    }
}

impl AtomNode {
    /// Address of a file's stable data object, as held by open descriptors.
    pub fn file_address(&self, filename: &str) -> Option<u64> {
        self.file(filename).map(|data| data as *const Vec<u8> as u64)
    }

    /// Removes a file. Callers must first confirm no descriptor holds it,
    /// because descriptors reference the boxed data object directly.
    pub fn remove(&mut self, filename: &str) -> bool {
        let Self::Directory(children) = self else { return false; };
        match children.iter().position(|(name, node)| name == filename && matches!(node, Self::File(_))) {
            Some(index) => { children.remove(index); true }
            None => false,
        }
    }

    /// Renames a file in place. The boxed data object does not move, so open
    /// descriptors stay valid. Fails if the target name exists or is invalid.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        if !valid_name(to) { return false; }
        let Self::Directory(children) = self else { return false; };
        if children.iter().any(|(name, _)| name == to) { return false; }
        match children.iter_mut().find(|(name, node)| name == from && matches!(node, Self::File(_))) {
            Some((name, _)) => { *name = String::from(to); true }
            None => false,
        }
    }
}

/// A global shared instance of the root file system protected by our Spinlock.
pub static ROOT_FS: Spinlock<AtomNode> = Spinlock::new(AtomNode::new_dir());
