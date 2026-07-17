use crate::io::Port;
use crate::memory::IrqSpinlock;
use core::arch::asm;

pub struct SerialPort {
    data: Port,
    interrupt_enable: Port,
    fifo_control: Port,
    line_control: Port,
    modem_control: Port,
    line_status: Port,
}

impl SerialPort {
    pub const fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            interrupt_enable: Port::new(base + 1),
            fifo_control: Port::new(base + 2),
            line_control: Port::new(base + 3),
            modem_control: Port::new(base + 4),
            line_status: Port::new(base + 5),
        }
    }

    pub fn init(&mut self) {
        self.interrupt_enable.write(0x00);    // Disable all interrupts
        self.line_control.write(0x80);        // Enable DLAB (set baud rate divisor)
        self.data.write(0x03);                // Set divisor to 3 (lo byte) 38400 baud
        self.interrupt_enable.write(0x00);    //                  (hi byte)
        self.line_control.write(0x03);        // 8 bits, no parity, one stop bit
        self.fifo_control.write(0xC7);        // Enable FIFO, clear them, with 14-byte threshold
        self.modem_control.write(0x0B);       // IRQs enabled, RTS/DSR set
    }

    fn is_transmit_empty(&self) -> bool {
        self.line_status.read() & 0x20 != 0
    }

    pub fn send(&mut self, data: u8) {
        while !self.is_transmit_empty() {
            unsafe { asm!("pause", options(nomem, nostack, preserves_flags)); }
        }
        self.data.write(data);
    }
}

pub static SERIAL1: IrqSpinlock<SerialPort> = IrqSpinlock::new(SerialPort::new(0x3F8));
