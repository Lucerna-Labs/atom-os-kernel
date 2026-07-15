//! Global Descriptor Table and Task State Segment implementation for x86_64 bare-metal.
use core::arch::asm;
use crate::atoms::{combine, project};

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct GdtEntry(u64);

impl GdtEntry {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Kernel Code Segment (64-bit, Ring 0)
    pub const fn kernel_code() -> Self {
        Self(0x00209A0000000000)
    }

    /// Kernel Data Segment (Ring 0)
    pub const fn kernel_data() -> Self {
        Self(0x0000920000000000)
    }

    /// User Code Segment (64-bit, Ring 3)
    pub const fn user_code() -> Self {
        Self(0x0020FA0000000000)
    }

    /// User Data Segment (Ring 3)
    pub const fn user_data() -> Self {
        Self(0x0000F20000000000)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed(4))]
pub struct TaskStateSegment {
    pub reserved_1: u32,
    pub privilege_stack_table: [u64; 3], // RSP0, RSP1, RSP2
    pub reserved_2: u64,
    pub interrupt_stack_table: [u64; 7], // IST1..IST7
    pub reserved_3: u64,
    pub reserved_4: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved_1: 0,
            privilege_stack_table: [0; 3],
            reserved_2: 0,
            interrupt_stack_table: [0; 7],
            reserved_3: 0,
            reserved_4: 0,
            iomap_base: 0,
        }
    }
}

/// The TSS requires a 16-byte descriptor in 64-bit mode.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TssDescriptor {
    low: u64,
    high: u64,
}

impl TssDescriptor {
    pub fn new(tss_ptr: *const TaskStateSegment) -> Self {
        let ptr = tss_ptr as u64;
        let mut low = 0x0000890000000067; // Base attributes for a 64-bit TSS (present, type 9, limit 103)
        
        low = combine(low, ptr, |l, p| {
            l | ((p & 0xFFFFFF) << 16) | (((p >> 24) & 0xFF) << 56)
        });
        let high = ptr >> 32;

        Self { low, high }
    }
}

#[derive(Debug)]
#[repr(C, align(8))]
pub struct GlobalDescriptorTable {
    pub null: GdtEntry,
    pub kcode: GdtEntry,
    pub kdata: GdtEntry,
    pub ucode: GdtEntry,
    pub udata: GdtEntry,
    pub tss: TssDescriptor,
}

#[repr(C, packed(2))]
struct Gdtr {
    limit: u16,
    base: u64,
}

impl GlobalDescriptorTable {
    pub const fn new() -> Self {
        Self {
            null: GdtEntry::new(0),
            kcode: GdtEntry::kernel_code(),
            kdata: GdtEntry::kernel_data(),
            ucode: GdtEntry::user_code(),
            udata: GdtEntry::user_data(),
            tss: TssDescriptor { low: 0, high: 0 },
        }
    }

    pub fn set_tss(&mut self, tss_ptr: *const TaskStateSegment) {
        self.tss = TssDescriptor::new(tss_ptr);
    }

    pub unsafe fn load(&'static self) {
        let gdtr = Gdtr {
            limit: (core::mem::size_of::<Self>() - 1) as u16,
            base: self as *const _ as u64,
        };

        project(&gdtr, |ptr| {
            unsafe {
                asm!("lgdt [{}]", in(reg) ptr, options(nostack, readonly, preserves_flags));
            }
        });

        // Reload segment registers (CS is tricky, requires a far jump, but we can fake it with a retq or leave it for later wrapper if needed)
        // Here we just reload data segments. CS reload is typically done via pushing to stack and retf.
        unsafe {
            asm!(
                "mov ax, 0x10", // 0x10 is the kdata segment (index 2)
                "mov ds, ax",
                "mov es, ax",
                "mov ss, ax",
                options(nostack, preserves_flags)
            );
        }
    }

    pub unsafe fn load_tss(&self) {
        // The TSS is at index 5 in the GDT, so the selector is 5 * 8 = 40 (0x28).
        unsafe {
            asm!("ltr ax", in("ax") 0x28_u16, options(nostack, preserves_flags));
        }
    }
}
