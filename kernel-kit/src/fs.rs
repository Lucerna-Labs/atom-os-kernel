//! Mathematical Virtual File System (RamFS)
use alloc::vec::Vec;
use alloc::string::String;
use crate::memory::Spinlock;

/// An abstract mathematical node representing data or a branch in the file system tree.
pub enum AtomNode {
    File(Vec<u8>),
    Directory(Vec<(String, AtomNode)>),
}

impl AtomNode {
    pub const fn new_dir() -> Self {
        AtomNode::Directory(Vec::new())
    }

    /// Searches for a file and returns its data buffer as a mutable reference.
    /// In a real OS, paths would be split by '/'. For this proof of concept, we support flat filenames in the root.
    pub fn get_or_create_file(&mut self, filename: &str) -> Option<*mut Vec<u8>> {
        match self {
            AtomNode::Directory(children) => {
                // Find existing
                for (name, node) in children.iter_mut() {
                    if name == filename {
                        if let AtomNode::File(data) = node {
                            return Some(data as *mut Vec<u8>);
                        } else {
                            return None; // Exists but is a directory
                        }
                    }
                }
                
                // If not found, create a new file
                children.push((String::from(filename), AtomNode::File(Vec::new())));
                if let AtomNode::File(data) = &mut children.last_mut().unwrap().1 {
                    Some(data as *mut Vec<u8>)
                } else {
                    None
                }
            },
            AtomNode::File(_) => None, // Cannot search inside a file
        }
    }
}

/// A global shared instance of the root file system protected by our Spinlock.
pub static ROOT_FS: Spinlock<AtomNode> = Spinlock::new(AtomNode::new_dir());
