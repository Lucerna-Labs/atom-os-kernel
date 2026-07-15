use crate::io::Port;

pub struct Keyboard {
    data_port: Port,
    status_port: Port,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self {
            data_port: Port::new(0x60),
            status_port: Port::new(0x64),
        }
    }

    /// Reads a scancode if available. The orchestrator must poll or be driven by an interrupt.
    pub fn read_scancode(&self) -> Option<u8> {
        let status = self.status_port.read();
        if status & 1 != 0 {
            Some(self.data_port.read())
        } else {
            None
        }
    }
}

pub fn scancode_to_ascii(scancode: u8) -> Option<u8> {
    match scancode {
        0x01 => Some(0x1B), // ESC
        0x02 => Some(b'1'), 0x03 => Some(b'2'), 0x04 => Some(b'3'), 0x05 => Some(b'4'),
        0x06 => Some(b'5'), 0x07 => Some(b'6'), 0x08 => Some(b'7'), 0x09 => Some(b'8'),
        0x0A => Some(b'9'), 0x0B => Some(b'0'), 0x0C => Some(b'-'), 0x0D => Some(b'='),
        0x0E => Some(8), // Backspace
        0x0F => Some(b'\t'), 0x10 => Some(b'q'), 0x11 => Some(b'w'), 0x12 => Some(b'e'),
        0x13 => Some(b'r'), 0x14 => Some(b't'), 0x15 => Some(b'y'), 0x16 => Some(b'u'),
        0x17 => Some(b'i'), 0x18 => Some(b'o'), 0x19 => Some(b'p'), 0x1A => Some(b'['),
        0x1B => Some(b']'), 0x1C => Some(b'\n'), // Enter
        0x1E => Some(b'a'), 0x1F => Some(b's'), 0x20 => Some(b'd'), 0x21 => Some(b'f'),
        0x22 => Some(b'g'), 0x23 => Some(b'h'), 0x24 => Some(b'j'), 0x25 => Some(b'k'),
        0x26 => Some(b'l'), 0x27 => Some(b';'), 0x28 => Some(b'\''), 0x29 => Some(b'`'),
        0x2B => Some(b'\\'), 0x2C => Some(b'z'), 0x2D => Some(b'x'), 0x2E => Some(b'c'),
        0x2F => Some(b'v'), 0x30 => Some(b'b'), 0x31 => Some(b'n'), 0x32 => Some(b'm'),
        0x33 => Some(b','), 0x34 => Some(b'.'), 0x35 => Some(b'/'), 0x39 => Some(b' '),
        0x4E => Some(b'>'), // Numpad Plus mapped to > as a hack for echo >
        _ => None,
    }
}

