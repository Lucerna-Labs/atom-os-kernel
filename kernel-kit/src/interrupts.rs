//! Bare-metal Interrupt Descriptor Table (IDT) mechanism.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    pub const fn new() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    pub fn set_handler_addr(&mut self, addr: u64, dpl: u8) {
        self.offset_low = addr as u16;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.selector = 0x08; // Code segment selector in GDT
        
        // 0x8E = Present(1) | DPL(0) | 0 | GateType(0xE)
        // DPL goes in bits 5 and 6.
        self.type_attr = 0x8E | (dpl << 5); 
    }
}

#[repr(C, packed)]
pub struct IdtDescriptor {
    size: u16,
    offset: u64,
}

pub struct Idt {
    entries: [IdtEntry; 256],
}

impl Idt {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::new(); 256],
        }
    }

    pub fn set_handler(&mut self, interrupt_id: u8, handler: u64) {
        self.entries[interrupt_id as usize].set_handler_addr(handler, 0); // Ring 0 only
    }

    pub fn set_handler_user(&mut self, interrupt_id: u8, handler: u64) {
        self.entries[interrupt_id as usize].set_handler_addr(handler, 3); // Ring 3 accessible!
    }

    pub fn load(&'static self) {
        let descriptor = IdtDescriptor {
            size: (core::mem::size_of::<Idt>() - 1) as u16,
            offset: self as *const _ as u64,
        };
        unsafe {
            core::arch::asm!("lidt [{}]", in(reg) &descriptor, options(readonly, nostack, preserves_flags));
        }
    }
}
