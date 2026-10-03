//! Bounded byte pipes connecting one process's stdout to another's stdin.
//!
//! A pipe counts its live read and write ends. Ends are owned by process
//! contexts (and by the creator's pipe handle until it closes), and dropping
//! an end releases it, so a process that exits or is killed closes its ends
//! when the scheduler reclaims its resources.
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use crate::memory::Spinlock;

pub const PIPE_CAPACITY: usize = 4096;

pub struct Pipe { buffer: VecDeque<u8>, readers: usize, writers: usize }

#[derive(Debug, PartialEq, Eq)]
pub enum Read { Data(usize), WouldBlock, End }
#[derive(Debug, PartialEq, Eq)]
pub enum Write { Wrote(usize), WouldBlock, Broken }

/// One counted end of a pipe. Cloning takes another reference of the same kind.
pub struct PipeEnd { pipe: Arc<Spinlock<Pipe>>, writer: bool }

impl PipeEnd {
    /// A new pipe, returned as its (read, write) ends.
    pub fn pair() -> (PipeEnd, PipeEnd) {
        let pipe = Arc::new(Spinlock::new(Pipe { buffer: VecDeque::new(), readers: 0, writers: 0 }));
        (Self::attach(&pipe, false), Self::attach(&pipe, true))
    }
    fn attach(pipe: &Arc<Spinlock<Pipe>>, writer: bool) -> PipeEnd {
        let state = pipe.lock();
        if writer { state.writers += 1; } else { state.readers += 1; }
        pipe.unlock();
        PipeEnd { pipe: pipe.clone(), writer }
    }
    pub fn is_writer(&self) -> bool { self.writer }

    /// Copies buffered bytes out. `End` once the buffer is empty and no
    /// write end remains; `WouldBlock` while a writer may still produce data.
    pub fn read(&self, out: &mut [u8]) -> Read {
        let state = self.pipe.lock();
        let result = if out.is_empty() { Read::Data(0) }
        else if state.buffer.is_empty() { if state.writers == 0 { Read::End } else { Read::WouldBlock } }
        else {
            let count = out.len().min(state.buffer.len());
            for (slot, byte) in out.iter_mut().zip(state.buffer.drain(..count)) { *slot = byte; }
            Read::Data(count)
        };
        self.pipe.unlock();
        result
    }

    /// Buffers as many bytes as fit. `Broken` once no read end remains.
    pub fn write(&self, bytes: &[u8]) -> Write {
        let state = self.pipe.lock();
        let result = if state.readers == 0 { Write::Broken }
        else {
            let count = bytes.len().min(PIPE_CAPACITY - state.buffer.len());
            if count == 0 && !bytes.is_empty() { Write::WouldBlock }
            else { state.buffer.extend(&bytes[..count]); Write::Wrote(count) }
        };
        self.pipe.unlock();
        result
    }
}

impl Clone for PipeEnd {
    fn clone(&self) -> Self { Self::attach(&self.pipe, self.writer) }
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        let state = self.pipe.lock();
        if self.writer { state.writers -= 1; } else { state.readers -= 1; }
        self.pipe.unlock();
    }
}

impl core::fmt::Debug for PipeEnd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(if self.writer { "PipeEnd(write)" } else { "PipeEnd(read)" })
    }
}
