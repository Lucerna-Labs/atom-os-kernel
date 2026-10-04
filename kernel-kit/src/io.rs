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

    pub fn read16(&self) -> u16 {
        let value: u16;
        unsafe { asm!("in ax, dx", out("ax") value, in("dx") self.port, options(nomem, nostack, preserves_flags)); }
        value
    }
    pub fn read32(&self) -> u32 {
        let value: u32;
        unsafe { asm!("in eax, dx", out("eax") value, in("dx") self.port, options(nomem, nostack, preserves_flags)); }
        value
    }
    pub fn write16(&mut self, value: u16) {
        unsafe { asm!("out dx, ax", in("ax") value, in("dx") self.port, options(nomem, nostack, preserves_flags)); }
    }
    pub fn write32(&mut self, value: u32) {
        unsafe { asm!("out dx, eax", in("eax") value, in("dx") self.port, options(nomem, nostack, preserves_flags)); }
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

use crate::memory::IrqSpinlock;

/// Raw input bytes held between polls. Generous, because the desktop drains input only
/// between frames: a burst of mouse packets during a slow frame must not overflow.
const BUFFER_SIZE: usize = 8192;

/// Timer ticks (10 ms) as seen by interrupt handlers, for timestamping input.
pub static INPUT_CLOCK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn input_clock() -> u32 { INPUT_CLOCK.load(core::sync::atomic::Ordering::Relaxed) }

/// A bounded ring of input bytes, each stamped with the tick it arrived on.
pub struct RingBuffer {
    buffer: [(u8, u32); BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl RingBuffer {
    pub const fn new() -> Self {
        Self {
            buffer: [(0, 0); BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    /// Pushes a byte and its arrival tick. If full, the oldest byte is overwritten.
    pub fn push(&mut self, data: u8, time: u32) {
        self.buffer[self.head] = (data, time);
        self.head = (self.head + 1) % BUFFER_SIZE;
        if self.head == self.tail {
            // Buffer overflow, drop the oldest data
            self.tail = (self.tail + 1) % BUFFER_SIZE;
        }
    }

    /// Pops the oldest byte and its arrival tick. Returns None if empty.
    pub fn pop(&mut self) -> Option<(u8, u32)> {
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
pub static KEYBOARD_BUFFER: IrqSpinlock<RingBuffer> = IrqSpinlock::new(RingBuffer::new());
/// Raw PS/2 mouse bytes queued by the IRQ12 handler.
pub static MOUSE_BUFFER: IrqSpinlock<RingBuffer> = IrqSpinlock::new(RingBuffer::new());
