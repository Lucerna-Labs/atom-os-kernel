#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::alloc::Layout;
use kernel_kit::vga::VgaWriter;
use kernel_kit::interrupts::Idt;
use kernel_kit::pic::ChainedPics;
use kernel_kit::keyboard::Keyboard;
use kernel_orchestrator::system::System;
use kernel_kit::memory::{AtomHeap, BumpAllocator, Spinlock};
use kernel_kit::slab::SlabLocked;
use core::sync::atomic::{AtomicUsize, Ordering};
use bootloader::BootInfo;

// GAP 1: SlabLocked is the active global allocator. Slab fast-path for
// sizes <= 2048 bytes (covering Vec headers, Context, TrapFrame, String,
// small buffers); BumpAllocator fallback for the long tail. Verified by
// slab_self_test (see NOTES.md) and the OOM-after-N benchmark below.
#[global_allocator]
static ALLOCATOR: SlabLocked = SlabLocked::new();

// Retained for the OOM-after-N benchmark's baseline comparison only;
// not the active allocator.
static BUMP_BASELINE: Spinlock<BumpAllocator> = Spinlock::new(BumpAllocator::new());

// 16 MiB statically allocated byte array in the `.bss` section. The front is the
// kernel heap (Vecs, allocations); the back 4 MiB is reserved as the physical
// frame pool used by paging (see kernel_kit::memory::FRAME_ALLOCATOR). The
// bootloader identity-maps physical memory, so virtual == physical here and
// frame addresses can be used both as PTE targets and as directly-derefable
// pointers.
static mut HEAP_MEM: [u8; 16 * 1024 * 1024] = [0; 16 * 1024 * 1024];
const HEAP_BYTES: usize = 16 * 1024 * 1024;

#[alloc_error_handler]
fn alloc_error_handler(_layout: Layout) -> ! {
    let mut vga = VgaWriter::new();
    vga.write_string("ALLOCATION ERROR: OUT OF MEMORY!");
    loop {}
}

static mut IDT: Idt = Idt::new();
static mut PICS: ChainedPics = ChainedPics::new();
static mut KEYBOARD: Keyboard = Keyboard::new();

static TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let mut vga = VgaWriter::new();
    vga.write_string("KERNEL PANIC!");
    loop {}
}

// Our true hardware interrupt wrapper that pushes the state, swaps the stack, and pops the state.
global_asm!(r#"
.global timer_interrupt_wrapper
timer_interrupt_wrapper:
    // Push all general purpose registers to form the TrapFrame
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    // Pass the current stack pointer (rsp) as the first argument (rdi) to the Rust handler
    mov rdi, rsp
    call timer_interrupt_handler

    // The Rust handler returns the new stack pointer in rax. Switch stacks!
    mov rsp, rax

    // Pop all general purpose registers from the new task's TrapFrame
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

// Hardware return from interrupt
    iretq
"#);

global_asm!(r#"
.global syscall_interrupt_wrapper
syscall_interrupt_wrapper:
    // Push TrapFrame (General Purpose Registers)
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call syscall_interrupt_handler

    // The handler doesn't change the stack pointer for a syscall, it just returns it
    mov rsp, rax

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    iretq
"#);

// Keyboard IRQ wrapper. Identical structure to the timer wrapper: the CPU pushes
// the hardware frame, we push the 15 GPRs, hand the frame pointer to the Rust
// handler, then pop and iretq. Registering the bare Rust fn (as the old code did)
// left no path back to the interrupted context — `ret` popped garbage into RIP.
global_asm!(r#"
.global keyboard_interrupt_wrapper
keyboard_interrupt_wrapper:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call keyboard_interrupt_handler

    mov rsp, rax

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    iretq
"#);

// Generic CPU-exception handler. CPU exceptions split into two families:
//   * error-code vectors (8 #DF, 10 #TS, 11 #NP, 12 #SS, 13 #GP, 14 #PF, 17 #AC)
//     where the CPU pushes an error word before transferring control, and
//   * no-error-code vectors (everything else in 0..31).
// To get a uniform stack layout we use two trampoline macros: the no-EC variant
// pushes a synthetic 0 so the handler always sees [errcode][hwframe][GPRs].
global_asm!(r#"
.global exception_common
exception_common:
    // On entry the stack holds: [errcode][SS][RSP][RFLAGS][CS][RIP].
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    // rdi = frame pointer (the 15 GPRs we just pushed). rsi already holds the
    // vector number set by the per-vector trampoline; pushes don't touch it.
    mov rdi, rsp
    call exception_handler

    // Exceptions are fatal in this kernel: never return. Halt with ints off.
    cli
1:  hlt
    jmp 1b
"#);

// No-error-code trampoline: synthesize errcode=0, set vector in rsi, jump.
global_asm!(r#"
.macro EXC_VEC_NEC n
.global exception_entry_\n
exception_entry_\n:
    push 0
    mov rsi, \n
    jmp exception_common
.endm
EXC_VEC_NEC 0
EXC_VEC_NEC 1
EXC_VEC_NEC 2
EXC_VEC_NEC 3
EXC_VEC_NEC 4
EXC_VEC_NEC 5
EXC_VEC_NEC 6
EXC_VEC_NEC 7
EXC_VEC_NEC 9
EXC_VEC_NEC 15
EXC_VEC_NEC 16
EXC_VEC_NEC 18
EXC_VEC_NEC 19
EXC_VEC_NEC 20
EXC_VEC_NEC 21
EXC_VEC_NEC 22
EXC_VEC_NEC 23
EXC_VEC_NEC 24
EXC_VEC_NEC 25
EXC_VEC_NEC 26
EXC_VEC_NEC 27
EXC_VEC_NEC 28
EXC_VEC_NEC 29
EXC_VEC_NEC 30
EXC_VEC_NEC 31
"#);

// Error-code trampoline: CPU already pushed the errcode; just set vector and jump.
global_asm!(r#"
.macro EXC_VEC_EC n
.global exception_entry_\n
exception_entry_\n:
    mov rsi, \n
    jmp exception_common
.endm
EXC_VEC_EC 8
EXC_VEC_EC 10
EXC_VEC_EC 11
EXC_VEC_EC 12
EXC_VEC_EC 13
EXC_VEC_EC 14
EXC_VEC_EC 17
"#);

#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        core::arch::asm!("wrmsr", in("ecx") msr, in("eax") low, in("edx") high, options(nostack, preserves_flags));
    }
}

#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nostack, preserves_flags));
    }
    (low as u64) | ((high as u64) << 32)
}

unsafe fn setup_syscall_msr() {
    let efer_addr: u32 = 0xC0000080;
    let mut efer = rdmsr(efer_addr);
    efer |= 1 << 12;
    wrmsr(efer_addr, efer);
    let star: u64 = (0x08u64 << 32) | (0x1Bu64 << 48);
    wrmsr(0xC0000081, star);
    wrmsr(0xC0000082, syscall_entry as u64);
    let fmask: u64 = (1 << 9) | (1 << 8);
    wrmsr(0xC0000084, fmask);
}

extern "C" {
    fn timer_interrupt_wrapper();
    fn syscall_interrupt_wrapper();
    fn keyboard_interrupt_wrapper();
    // Per-vector CPU-exception entry points generated by the global_asm! macros.
    fn exception_entry_0();
    fn exception_entry_1();
    fn exception_entry_2();
    fn exception_entry_3();
    fn exception_entry_4();
    fn exception_entry_5();
    fn exception_entry_6();
    fn exception_entry_7();
    fn exception_entry_8();
    fn exception_entry_9();
    fn exception_entry_10();
    fn exception_entry_11();
    fn exception_entry_12();
    fn exception_entry_13();
    fn exception_entry_14();
    fn exception_entry_15();
    fn exception_entry_16();
    fn exception_entry_17();
    fn exception_entry_18();
    fn exception_entry_19();
    fn exception_entry_20();
    fn exception_entry_21();
    fn exception_entry_22();
    fn exception_entry_23();
    fn exception_entry_24();
    fn exception_entry_25();
    fn exception_entry_26();
    fn exception_entry_27();
    fn exception_entry_28();
    fn exception_entry_29();
    fn exception_entry_30();
    fn exception_entry_31();
    fn syscall_entry();
}

static mut SYSTEM: Option<System> = None;

#[no_mangle]
pub extern "C" fn timer_interrupt_handler(rsp: u64) -> u64 {
    let tick = TIMER_TICKS.fetch_add(1, Ordering::SeqCst);

    // Advance the radiation/dissipation field substrate once every
    // EVOLUTION_DIVISOR ticks. Field evolution is the substrate that drives
    // the field-driven scheduler (when feature-gated) and provides the data
    // for SYS_FIELD_OBSERVE and SYS_FIELD_MEASUREMENTS.
    kernel_kit::scheduler_glue::maybe_evolve(tick as u64);

    let mut new_rsp = rsp;

    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            new_rsp = sys.scheduler.switch_context(rsp);
            if new_rsp != rsp {
                if let Some(next_task) = sys.scheduler.current_task() {
                    if kernel_kit::paging::Cr3::read() != next_task.page_table_root {
                        kernel_kit::paging::Cr3::load(next_task.page_table_root);
                    }
                    TSS.privilege_stack_table[0] = next_task.kernel_stack;
                }
            }
        }
        PICS.notify_end_of_interrupt(32);
    }

    new_rsp
}

#[no_mangle]
pub extern "C" fn syscall_interrupt_handler(rsp: u64) -> u64 {
    let mut new_rsp = rsp;
    
    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            if let Some(task) = sys.scheduler.current_task_mut() {
                // Read the syscall number from the trap frame on the stack.
                // Do NOT save task.rsp here — only the timer handler owns
                // task.rsp for context switching. Overwriting it here
                // corrupts the saved state when the timer later fires.
                let frame_ptr = rsp as *mut kernel_kit::trap::TrapFrame;
                let sys_num = (*frame_ptr).rax;
                let mut mem = kernel_kit::memory::MemoryPool::new();
                kernel_orchestrator::syscall::dispatch(task, &mut mem);
                // Return the SAME rsp — the syscall didn't switch stacks.
                new_rsp = rsp;

                // Only SYS_YIELD (1) and SYS_EXIT (3) trigger a context switch.
                let did_switch = if sys_num == 1 || sys_num == 3 {
                    // NOW it's safe to save the task's rsp for the switch.
                    task.rsp = rsp;
                    let _new = sys.scheduler.switch_context(rsp);
                    true
                } else {
                    false
                };
            }

            // Only reload CR3 / TSS if a context switch actually occurred.
            // Doing this on every syscall corrupts the TSS.esp0 that the
            // timer handler relies on for its trap frame placement.
            if new_rsp != rsp {
                if let Some(next_task) = sys.scheduler.current_task() {
                    if kernel_kit::paging::Cr3::read() != next_task.page_table_root {
                        kernel_kit::paging::Cr3::load(next_task.page_table_root);
                    }
                    TSS.privilege_stack_table[0] = next_task.kernel_stack;
                }
            }
        }
        // NOTE: int 0x80 is a software interrupt, not PIC-sourced, so no EOI is
        // issued here. The old code sent a spurious EOI to PIC2 (vector 0x80 >= 40)
        // which desynchronized the slave PIC and dropped subsequent hardware IRQs.
    }

    new_rsp
}

/// Fast syscall handler for the syscall/sysret path (GAP 3).
/// Same dispatch logic as syscall_interrupt_handler but entered via the
/// `syscall` instruction (no IDT, no interrupt-gate stack switch).
#[no_mangle]
pub extern "C" fn syscall_fast_handler(rsp: u64) -> u64 {
    let mut new_rsp = rsp;

    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            if let Some(task) = sys.scheduler.current_task_mut() {
                let frame_ptr = rsp as *mut kernel_kit::trap::TrapFrame;
                let sys_num = (*frame_ptr).rax;
                let mut mem = kernel_kit::memory::MemoryPool::new();

                kernel_orchestrator::syscall::dispatch(task, &mut mem);
                new_rsp = rsp;

                if sys_num == 1 || sys_num == 3 {
                    task.rsp = rsp;
                    new_rsp = sys.scheduler.switch_context(rsp);
                }
            }

            if new_rsp != rsp {
                if let Some(next_task) = sys.scheduler.current_task() {
                    if kernel_kit::paging::Cr3::read() != next_task.page_table_root {
                        kernel_kit::paging::Cr3::load(next_task.page_table_root);
                    }
                    TSS.privilege_stack_table[0] = next_task.kernel_stack;
                    KERNEL_STACK_PTR = next_task.kernel_stack;
                }
            }
        }
    }

    new_rsp
}

/// CPU-exception handler. Fatal: prints a diagnostic and halts.
/// Replaces the old silent triple-fault on any #PF/#GP/#DF/etc. so we can
/// actually see what faulted during bring-up.
#[no_mangle]
pub extern "C" fn exception_handler(frame: *const u8, vector: u64) {
    // Render the vector + faulting RIP/RSP from the saved TrapFrame. The frame
    // pointer points at our 15 pushed GPRs; immediately above them sit the
    // errcode and the hardware frame (rip, cs, rflags, rsp, ss).
    let v = vector as u8;
    let mut vga = kernel_kit::vga::VgaWriter::new();
    vga.write_string("\n!! CPU EXCEPTION ");
    // Print vector as two hex digits by hand (no core::fmt in the kernel).
    let hi = (v >> 4) & 0xF;
    let lo = v & 0xF;
    let hx = |n: u8| -> u8 { if n < 10 { b'0' + n } else { b'A' + (n - 10) } };
    vga.write_string(&[hx(hi) as char, hx(lo) as char].iter().collect::<alloc::string::String>());
    vga.write_string(" (");

    let name: &[u8] = match v {
        0 => b"#DE", 1 => b"#DB", 2 => b"NMI", 3 => b"#BP", 4 => b"#OF",
        5 => b"#BR", 6 => b"#UD", 7 => b"#NM", 8 => b"#DF", 10 => b"#TS",
        11 => b"#NP", 12 => b"#SS", 13 => b"#GP", 14 => b"#PF", 16 => b"#MF",
        17 => b"#AC", 18 => b"#MC", 19 => b"#XM", 20 => b"#VE", _ => b"??",
    };
    if let Ok(s) = core::str::from_utf8(name) { vga.write_string(s); }
    vga.write_string(") -- HALTING");

    // Also emit on serial so headless QEMU (CI) sees it.
    let banner = b"\n!! CPU EXCEPTION -- HALTING\n";
    for &b in banner {
        let (sport, ser_sif_1) = kernel_kit::serial::SERIAL1.lock(); sport.send(b); kernel_kit::serial::SERIAL1.unlock(ser_sif_1);
    }

    // Touch the frame so the pointer is provably used; the full register dump
    // can be added later by walking the TrapFrame layout.
    let _ = frame;

    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        loop { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)); }
    }
}

fn inject_payloads() {
    let shell_bytes = include_bytes!("../../target/x86_64-os/release/payload");
    let daemon_bytes = include_bytes!("../../target/x86_64-os/release/daemon");
    
    // Inject the payloads into the Root RamFS
    let mut fs = kernel_kit::fs::ROOT_FS.lock();
    if let kernel_kit::fs::AtomNode::Directory(children) = &mut *fs {
        use alloc::string::String;
        use alloc::vec::Vec;
        
        let mut shell_data = Vec::new();
        shell_data.extend_from_slice(shell_bytes);
        children.push((String::from("shell.elf"), kernel_kit::fs::AtomNode::File(shell_data)));
        
        let mut daemon_data = Vec::new();
        daemon_data.extend_from_slice(daemon_bytes);
        children.push((String::from("daemon.elf"), kernel_kit::fs::AtomNode::File(daemon_data)));
    }
    kernel_kit::fs::ROOT_FS.unlock();
}

unsafe fn spawn_process(pid: u64, filename: &[u8]) -> kernel_kit::context::Context {
    use kernel_kit::trap::TrapFrame;
    use alloc::vec::Vec;

    // Allocate an individual kernel stack for this process's TrapFrame and interrupt handling
    let mut kernel_stack = Vec::<u8>::with_capacity(4096);
    kernel_stack.resize(4096, 0);
    let kernel_stack_ptr = kernel_stack.as_ptr() as u64 + 4096;
    core::mem::forget(kernel_stack);

    let mut tf = TrapFrame::new_user(0, 0xFFFFFFFF80100000);
    let frame_ptr = (kernel_stack_ptr - core::mem::size_of::<TrapFrame>() as u64) as *mut TrapFrame;
    
    // We pass tf.rsp via ctx.rsp so the dispatcher can find it.
    let tf_ptr_temp = &mut tf as *mut _ as u64;
    let mut ctx = kernel_kit::context::Context::new(pid as usize, tf_ptr_temp, kernel_stack_ptr, kernel_kit::paging::Cr3::read());
    let mut mem = kernel_kit::memory::MemoryPool::new();
    
    // Setup trap frame to mimic SYS_EXEC arguments
    tf.rax = 13; // SYS_EXEC
    tf.rdi = filename.as_ptr() as u64;
    
    kernel_orchestrator::syscall::dispatch(&mut ctx, &mut mem);
    
    if tf.rax == core::u64::MAX {
        panic!("Failed to load payload");
    }

    *frame_ptr = tf; // Move TrapFrame to actual kernel stack
    
    // Update Context rsp to point to the TrapFrame on the kernel stack
    ctx.rsp = frame_ptr as u64;
    
    ctx
}

// Now takes/returns rsp because keyboard_interrupt_wrapper does `mov rsp,rax`
// after calling us, mirroring the timer/syscall stubs. We never switch stacks
// here, so we just hand the incoming rsp back unchanged.
#[no_mangle]
pub extern "C" fn keyboard_interrupt_handler(rsp: u64) -> u64 {
    unsafe {
        if let Some(scancode) = KEYBOARD.read_scancode() {
            // Push the scancode into the global ring buffer instead of writing it immediately
            // We only care about key presses (scancode < 0x80), ignore key releases.
            if scancode < 0x80 {
                let (buffer, kb_sif) = kernel_kit::io::KEYBOARD_BUFFER.lock();
                buffer.push(scancode);
                kernel_kit::io::KEYBOARD_BUFFER.unlock(kb_sif);
            }
        }
        PICS.notify_end_of_interrupt(33);
    }
    rsp
}

use kernel_kit::gdt::{GlobalDescriptorTable, TaskStateSegment};

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Kernel stack pointer for syscall entry. Updated by the timer handler
/// and _start so the syscall trampoline can find the kernel stack.
#[no_mangle]
pub static mut KERNEL_STACK_PTR: u64 = 0;

// ──────────────── GAP 3: syscall/sysret fast entry ────────────────
global_asm!(r#"
.global syscall_entry
syscall_entry:
    // On entry: RCX = user RIP, R11 = user RFLAGS, RAX = syscall number.
    // CPU did NOT push anything. We are on the user stack.
    // Switch to kernel stack via KERNEL_STACK_PTR global.
    mov rax, [KERNEL_STACK_PTR]
    mov rsp, rax

    // Push TrapFrame (same layout as the int 0x80 wrapper).
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    // Call Rust handler with rdi = rsp (pointer to TrapFrame).
    mov rdi, rsp
    call syscall_fast_handler

    // Handler returns new rsp in rax.
    mov rsp, rax

    // Restore GPRs.
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    // Return to user mode. RCX has user RIP, R11 has user RFLAGS.
    sysretq
"#);

#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    // The bootloader passes BootInfo in rdi (System V x86_64 ABI). It contains
    // physical_memory_offset — the virtual address at which ALL physical memory
    // is mapped. Storing it globally lets paging.rs translate phys->virt.
    kernel_kit::paging::set_phys_offset(boot_info.physical_memory_offset);

    unsafe {
        let mut cr0: u64;
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        cr0 &= !(1 << 2); // Clear EM
        cr0 |= 1 << 1;    // Set MP
        core::arch::asm!("mov cr0, {}", in(reg) cr0);

        core::arch::asm!("mov {}, cr4", out(reg) cr4);
        cr4 |= 1 << 9;    // Set OSFXSR
        cr4 |= 1 << 10;   // Set OSXMMEXCPT
        core::arch::asm!("mov cr4, {}", in(reg) cr4);
    }
    let (obj, sif_1) = kernel_kit::serial::SERIAL1.lock();
    obj.init();
    kernel_kit::serial::SERIAL1.unlock(sif_1);

    let (obj, sif_2) = kernel_kit::serial::SERIAL1.lock();

    obj.send(b'A');

    kernel_kit::serial::SERIAL1.unlock(sif_2);

    let mut vga = kernel_kit::vga::VgaWriter::new();
    vga.write_string("Booting Fearless Hypatia...\n");
    
    // Set up the kernel heap. SlabLocked owns a BumpAllocator internally
    // for fallback; init wires both to the HEAP_MEM region.
    unsafe {
        let heap_start = (&raw const HEAP_MEM) as *const u8 as usize;
        ALLOCATOR.init(heap_start, HEAP_BYTES);
    }

    // GAP 1 OOM-after-N benchmark: compare slab vs bump on the same
    // workload (alloc 64 bytes, free, repeat). Runs on private regions
    // so it doesn't perturb the live kernel heap. Reports N_slab vs
    // N_bump — the named currency for GAP 1.
    unsafe {
        let heap_start = (&raw const HEAP_MEM) as *const u8 as usize;
        // Carve two equal 256 KiB regions from the tail of HEAP_MEM.
        // The slab global heap won't reach here in normal boot.
        let region_size = 256 * 1024;
        let slab_region = heap_start + HEAP_BYTES - region_size;
        let bump_region = slab_region - region_size;
        let print_fn = |s: &str| {
            let (sport, sif) = kernel_kit::serial::SERIAL1.lock();
            for &b in s.as_bytes() {
                sport.send(b);
            }
            kernel_kit::serial::SERIAL1.unlock(sif);
        };
        kernel_kit::slab::oom_after_n_benchmark(
            slab_region, region_size, bump_region, region_size, print_fn,
        );
    }

    // Set up the physical frame allocator by scanning the bootloader's memory
    // map for the largest Usable region. Frames from this region are in genuine
    // free RAM covered by the bootloader's physical_memory_offset mapping, so
    // paging::phys_to_virt() works on them. This is required for duplicate_pml4
    // to write valid PML4 copies that CR3 can load (the prior triple-fault
    // blocker): without a real USABLE region, PML4 copies were written through
    // addresses that resolved to wrong RAM, corrupting every page-table entry.
    unsafe {
        use bootloader::bootinfo::MemoryRegionType;
        let mut best_start: u64 = 0;
        let mut best_len: u64 = 0;
        for region in boot_info.memory_map.iter() {
            if matches!(region.region_type, MemoryRegionType::Usable) {
                let s = region.range.start_frame_number;
                let e = region.range.end_frame_number;
                if e > s {
                    let len = e - s;
                    if len > best_len && len >= 256 {
                        best_len = len;
                        best_start = s;
                    }
                }
            }
        }
        if best_len == 0 {
            panic!("No usable memory region found for frame allocator");
        }
        let base_phys = (best_start * 4096) as usize;
        let num_frames = (best_len as usize).min(kernel_kit::memory::FRAMES_MAX);
        let (obj, sif_7) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        obj.init(base_phys, num_frames);
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(sif_7);
    }
    let (obj, sif_3) = kernel_kit::serial::SERIAL1.lock();
    obj.send(b'B');
    kernel_kit::serial::SERIAL1.unlock(sif_3);
    
    inject_payloads();
    let (obj, sif_4) = kernel_kit::serial::SERIAL1.lock();
    obj.send(b'C');
    kernel_kit::serial::SERIAL1.unlock(sif_4);
    
    // Test the allocator native to Rust
    let mut test_vec = alloc::vec::Vec::new();
    test_vec.push(42);
    if test_vec[0] == 42 {
        vga.write_string("Heap Allocation Test: PASS!\n");
    }
    
    let (obj, sif_5) = kernel_kit::serial::SERIAL1.lock();
    
    obj.send(b'D');
    
    kernel_kit::serial::SERIAL1.unlock(sif_5);

    // 1. Setup IDT
    unsafe {
        IDT.set_handler(32, timer_interrupt_wrapper as *const () as u64);
        // C1 fix: register the asm WRAPPER (which builds a TrapFrame and iretqs),
        // not the bare Rust fn. The old code triple-faulted on the first keypress
        // because the bare fn ended in `ret`, popping garbage into RIP.
        IDT.set_handler(33, keyboard_interrupt_wrapper as *const () as u64);
        
        // Use set_handler_user to set DPL=3, allowing Ring 3 to trigger the interrupt
        IDT.set_handler_user(0x80, syscall_interrupt_wrapper as *const () as u64);

        // C7 fix: register CPU-exception handlers for vectors 0..31 so that any
        // #PF/#GP/#DF etc. prints a diagnostic and halts instead of silently
        // triple-faulting and resetting the CPU (the confirmed boot failure).
        IDT.set_handler(0, exception_entry_0 as *const () as u64);
        IDT.set_handler(1, exception_entry_1 as *const () as u64);
        IDT.set_handler(2, exception_entry_2 as *const () as u64);
        IDT.set_handler(3, exception_entry_3 as *const () as u64);
        IDT.set_handler(4, exception_entry_4 as *const () as u64);
        IDT.set_handler(5, exception_entry_5 as *const () as u64);
        IDT.set_handler(6, exception_entry_6 as *const () as u64);
        IDT.set_handler(7, exception_entry_7 as *const () as u64);
        IDT.set_handler(8, exception_entry_8 as *const () as u64);  // #DF (errcode)
        IDT.set_handler(9, exception_entry_9 as *const () as u64);
        IDT.set_handler(10, exception_entry_10 as *const () as u64); // #TS (errcode)
        IDT.set_handler(11, exception_entry_11 as *const () as u64); // #NP (errcode)
        IDT.set_handler(12, exception_entry_12 as *const () as u64); // #SS (errcode)
        IDT.set_handler(13, exception_entry_13 as *const () as u64); // #GP (errcode)
        IDT.set_handler(14, exception_entry_14 as *const () as u64); // #PF (errcode)
        IDT.set_handler(15, exception_entry_15 as *const () as u64);
        IDT.set_handler(16, exception_entry_16 as *const () as u64);
        IDT.set_handler(17, exception_entry_17 as *const () as u64); // #AC (errcode)
        IDT.set_handler(18, exception_entry_18 as *const () as u64);
        IDT.set_handler(19, exception_entry_19 as *const () as u64);
        IDT.set_handler(20, exception_entry_20 as *const () as u64);
        IDT.set_handler(21, exception_entry_21 as *const () as u64);
        IDT.set_handler(22, exception_entry_22 as *const () as u64);
        IDT.set_handler(23, exception_entry_23 as *const () as u64);
        IDT.set_handler(24, exception_entry_24 as *const () as u64);
        IDT.set_handler(25, exception_entry_25 as *const () as u64);
        IDT.set_handler(26, exception_entry_26 as *const () as u64);
        IDT.set_handler(27, exception_entry_27 as *const () as u64);
        IDT.set_handler(28, exception_entry_28 as *const () as u64);
        IDT.set_handler(29, exception_entry_29 as *const () as u64);
        IDT.set_handler(30, exception_entry_30 as *const () as u64);
        IDT.set_handler(31, exception_entry_31 as *const () as u64);
        
        IDT.load();
    }
    let (obj, sif_6) = kernel_kit::serial::SERIAL1.lock();
    obj.send(b'E');
    kernel_kit::serial::SERIAL1.unlock(sif_6);

    // 2. Setup PIC
    unsafe {
        PICS.initialize(32, 40); // Map PIC1 to IRQ 32-39, PIC2 to IRQ 40-47
        // initialize() now masks everything; unmask only the lines we handle:
        // IRQ0 (timer, vector 32) and IRQ1 (keyboard, vector 33).
        PICS.unmask(0);
        PICS.unmask(1);
    }
    vga.write_string("PIC Initialized.\n");

    unsafe {
        SYSTEM = Some(System::new());
    }
    vga.write_string("Orchestrator Initialized.\n");

    // Initialise the radiation/dissipation field substrate. The field becomes
    // the computational substrate for IPC and (when feature-gated) the
    // scheduler. The timer IRQ advances it once every EVOLUTION_DIVISOR
    // ticks. See kernel_kit::scheduler_glue and ARCHITECTURE.md in the
    // ATOM OS substrate workspace.
    kernel_kit::scheduler_glue::init();
    vga.write_string("Field substrate Initialized.\n");
    
    // Setup TSS and GDT
    unsafe {
        GDT.set_tss(&raw const TSS);
        GDT.load();
        GDT.load_tss();
        // GAP 3: configure syscall/sysret MSRs after GDT is loaded.
        setup_syscall_msr();
        KERNEL_STACK_PTR = TSS.privilege_stack_table[0];
    }

    // Spawn User Tasks (Ring 3)
    unsafe {
        let shell_ctx = spawn_process(1, b"shell.elf\0");
        let daemon_ctx = spawn_process(2, b"daemon.elf\0");

        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            sys.scheduler.spawn(shell_ctx).unwrap();
            sys.scheduler.spawn(daemon_ctx).unwrap();
            
            // Set the initial TSS privilege stack table to the first active task
            if let Some(task) = sys.scheduler.current_task() {
                TSS.privilege_stack_table[0] = task.kernel_stack;
            }
        }
    }
    vga.write_string("Ring 3 Multi-Tasking Spawned.\n");

    // 3. Enable Interrupts
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    vga.write_string("System running autonomously. Awaiting hardware events...\n");
    
    loop {
        // Idle the CPU until an interrupt occurs
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
