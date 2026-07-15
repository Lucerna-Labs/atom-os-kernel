use crate::io::Port;

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;

pub struct ChainedPics {
    pic1_command: Port,
    pic1_data: Port,
    pic2_command: Port,
    pic2_data: Port,
}

impl ChainedPics {
    pub const fn new() -> Self {
        Self {
            pic1_command: Port::new(PIC1_COMMAND),
            pic1_data: Port::new(PIC1_DATA),
            pic2_command: Port::new(PIC2_COMMAND),
            pic2_data: Port::new(PIC2_DATA),
        }
    }

    pub fn initialize(&mut self, offset1: u8, offset2: u8) {
        // Save masks
        let a1 = self.pic1_data.read();
        let a2 = self.pic2_data.read();

        // Start init sequence
        self.pic1_command.write(ICW1_INIT | ICW1_ICW4);
        self.pic2_command.write(ICW1_INIT | ICW1_ICW4);
        
        // Setup offsets
        self.pic1_data.write(offset1);
        self.pic2_data.write(offset2);
        
        // Setup cascading
        self.pic1_data.write(4);
        self.pic2_data.write(2);
        
        // 8086/88 (MCS-80/85) mode
        self.pic1_data.write(ICW4_8086);
        self.pic2_data.write(ICW4_8086);
        
        // Restore masks
        self.pic1_data.write(a1);
        self.pic2_data.write(a2);
    }
    
    pub fn notify_end_of_interrupt(&mut self, interrupt_id: u8) {
        let pic2_offset = 40; // Assuming offset2 is 40
        if interrupt_id >= pic2_offset {
            self.pic2_command.write(0x20);
        }
        self.pic1_command.write(0x20);
    }
}
