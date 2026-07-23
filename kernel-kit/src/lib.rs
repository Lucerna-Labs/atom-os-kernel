#![no_std]
extern crate alloc;

pub mod atoms;
pub mod context;
pub mod slab;
pub mod fs;
pub mod elf;
pub mod serial;
pub mod gdt;
pub mod interrupts;
pub mod io;
pub mod keyboard;
pub mod memory;
pub mod page_table;
pub mod paging;
pub mod pic;
pub mod trap;
pub mod vga;

// Re-export the field substrate and kernel-glue layer so the rest of the
// kernel can reach them via `kernel_kit::field` / `kernel_kit::glue`.
pub use field_core as field;
pub use kernel_glue as glue;

/// Scheduler / field-substrate glue. Owns the global `FieldState` the kernel
/// timer IRQ advances and the scheduler consults.
pub mod scheduler_glue;
