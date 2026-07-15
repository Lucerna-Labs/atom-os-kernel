use core::arch::asm;

/// The Port primitive acts as a mathematical projection between CPU registers and the hardware bus.
#[derive(Clone, Copy)]
pub struct Port {
    port: u16,
}

impl Port {
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    /// Projects the hardware state of the port into an 8-bit value.
    #[inline]
    pub fn read(&self) -> u8 {
        let mut value: u8;
        unsafe {
            asm!("in al, dx", out("al") value, in("dx") self.port, options(nomem, nostack, preserves_flags));
        }
        value
    }

    /// Combines an 8-bit value with the hardware state of the port.
    #[inline]
    pub fn write(&mut self, value: u8) {
        unsafe {
            asm!("out dx, al", in("dx") self.port, in("al") value, options(nomem, nostack, preserves_flags));
        }
    }
}

use crate::memory::Spinlock;

const BUFFER_SIZE: usize = 256;

/// A simple, mathematically bounded Ring Buffer for storing keystrokes or bytes.
pub struct RingBuffer {
    buffer: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl RingBuffer {
    pub const fn new() -> Self {
        Self {
            buffer: [0; BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    /// Pushes a byte into the buffer. If full, the oldest byte is overwritten.
    pub fn push(&mut self, data: u8) {
        self.buffer[self.head] = data;
        self.head = (self.head + 1) % BUFFER_SIZE;
        if self.head == self.tail {
            // Buffer overflow, drop the oldest data
            self.tail = (self.tail + 1) % BUFFER_SIZE;
        }
    }

    /// Pops a byte from the buffer. Returns None if empty.
    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            None
        } else {
            let data = self.buffer[self.tail];
            self.tail = (self.tail + 1) % BUFFER_SIZE;
            Some(data)
        }
    }
}

/// A global shared instance of a Ring Buffer protected by our mathematical Spinlock.
pub static KEYBOARD_BUFFER: Spinlock<RingBuffer> = Spinlock::new(RingBuffer::new());
