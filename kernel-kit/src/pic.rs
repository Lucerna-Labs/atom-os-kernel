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
    // Vector offsets captured during initialize(); used by EOI to tell master-only
    // IRQs (offset1..offset1+7) from slave-chained IRQs (offset2..offset2+7).
    offset1: u8,
    offset2: u8,
}

impl ChainedPics {
    pub const fn new() -> Self {
        Self {
            pic1_command: Port::new(PIC1_COMMAND),
            pic1_data: Port::new(PIC1_DATA),
            pic2_command: Port::new(PIC2_COMMAND),
            pic2_data: Port::new(PIC2_DATA),
            offset1: 0,
            offset2: 0,
        }
    }

    pub fn initialize(&mut self, offset1: u8, offset2: u8) {
        // Record the offsets so notify_end_of_interrupt doesn't have to guess.
        self.offset1 = offset1;
        self.offset2 = offset2;

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

        // Mask all IRQs initially; individual drivers unmask the lines they own.
        // Restoring the pre-init mask (as the old code did) can leave IRQs masked
        // on emulators whose reset state is 0xFF, silently dropping the timer/kbd.
        self.pic1_data.write(0xFF);
        self.pic2_data.write(0xFF);
    }

    /// Unmask a single hardware IRQ line (0..15). Used by drivers to enable the
    /// device interrupts they actually handle.
    pub fn unmask(&mut self, irq: u8) {
        if irq < 8 {
            let mask = self.pic1_data.read();
            self.pic1_data.write(mask & !(1 << irq));
        } else if irq < 16 {
            let mask = self.pic2_data.read();
            self.pic2_data.write(mask & !(1 << (irq - 8)));
        }
    }

    /// Send End-Of-Interrupt. For slave-chained IRQs (vector >= offset2) the slave
    /// must be acknowledged before the master, otherwise the slave stops delivering.
    pub fn notify_end_of_interrupt(&mut self, interrupt_id: u8) {
        if interrupt_id >= self.offset2 {
            self.pic2_command.write(0x20);
        }
        self.pic1_command.write(0x20);
    }
}
