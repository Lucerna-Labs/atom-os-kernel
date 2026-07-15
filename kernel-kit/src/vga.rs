use crate::atoms::project;

const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;

pub struct VgaWriter {
    column_position: usize,
    row_position: usize,
}

impl VgaWriter {
    pub const fn new() -> Self {
        Self { column_position: 0, row_position: 0 }
    }

    pub fn write_byte(&mut self, byte: u8) {
        crate::serial::SERIAL1.lock().send(byte);
        crate::serial::SERIAL1.unlock();
        
        if byte == b'\n' {
            self.new_line();
            return;
        }

        if byte == 0x08 { // Backspace
            if self.column_position > 0 {
                self.column_position -= 1;
            } else if self.row_position > 0 {
                self.row_position -= 1;
                self.column_position = 79;
            }
            // Blank out the character
            unsafe {
                let offset = (self.row_position * 80 + self.column_position) * 2;
                VGA_BUFFER.add(offset).write_volatile(b' ');
                VGA_BUFFER.add(offset + 1).write_volatile(0x0f);
            }
            return;
        }

        if self.column_position >= 80 {
            self.new_line();
        }

        // Project logical character to physical VGA format (Color 0x0f is White on Black)
        let (ascii, color) = project(byte, |b| (b, 0x0f));
        
        unsafe {
            let offset = (self.row_position * 80 + self.column_position) * 2;
            // Write directly to physical hardware memory
            VGA_BUFFER.add(offset).write_volatile(ascii);
            VGA_BUFFER.add(offset + 1).write_volatile(color);
        }
        
        self.column_position += 1;
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }

    fn new_line(&mut self) {
        self.column_position = 0;
        self.row_position += 1;
        if self.row_position >= 25 {
            // Very simple clear-screen for demonstration
            self.clear_screen();
        }
    }

    pub fn clear_screen(&mut self) {
        self.column_position = 0;
        self.row_position = 0;
        for row in 0..25 {
            self.clear_row(row);
        }
    }

    fn clear_row(&self, row: usize) {
        let blank = b' ';
        let color = 0x0f;
        unsafe {
            for col in 0..80 {
                let offset = (row * 80 + col) * 2;
                VGA_BUFFER.add(offset).write_volatile(blank);
                VGA_BUFFER.add(offset + 1).write_volatile(color);
            }
        }
    }
}
