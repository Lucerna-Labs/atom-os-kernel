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
fn alloc_error_handler(layout: Layout) -> ! {
    use core::fmt::Write;
    let _ = writeln!(EmergencySerial, "ALLOCATION ERROR size={} align={}", layout.size(), layout.align());
    kernel_kit::io::Port::new(0xf4).write32(0x11);
    unsafe { core::arch::asm!("cli"); }
    loop { unsafe { core::arch::asm!("hlt"); } }
}

static mut IDT: Idt = Idt::new();
static mut PICS: ChainedPics = ChainedPics::new();
static mut KEYBOARD: Keyboard = Keyboard::new();

static TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    let _ = writeln!(EmergencySerial, "KERNEL_PANIC {info}");
    kernel_kit::io::Port::new(0xf4).write32(0x11);
    unsafe { core::arch::asm!("cli"); }
    loop { unsafe { core::arch::asm!("hlt"); } }
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
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call timer_interrupt_handler
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]

    // The Rust handler returns the new stack pointer in rax. Switch stacks!
    mov rsp, rax

.global restore_interrupt_context
restore_interrupt_context:
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]
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
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call syscall_interrupt_handler
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]

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
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call keyboard_interrupt_handler
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]

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

// PS/2 mouse IRQ12 wrapper, the same shape as the keyboard wrapper.
global_asm!(r#"
.global mouse_interrupt_wrapper
mouse_interrupt_wrapper:
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
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call mouse_interrupt_handler
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]

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
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call exception_handler
    jmp restore_interrupt_context

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
    efer |= 1; // EFER.SCE: bit 12 is reserved on Intel, SVME on AMD.
    wrmsr(efer_addr, efer);
    let star: u64 = (0x08u64 << 32) | (0x13u64 << 48);
    wrmsr(0xC0000081, star);
    wrmsr(0xC0000082, syscall_entry as u64);
    let fmask: u64 = (1 << 9) | (1 << 8);
    wrmsr(0xC0000084, fmask);
}

extern "C" {
    fn timer_interrupt_wrapper();
    fn syscall_interrupt_wrapper();
    fn keyboard_interrupt_wrapper();
    fn mouse_interrupt_wrapper();
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

unsafe fn activate(sys: &System) {
    let root = if let Some(task) = sys.scheduler.current_task() {
        TSS.privilege_stack_table[0] = task.kernel_stack;
        KERNEL_STACK_PTR = task.kernel_stack;
        task.page_table_root
    } else { sys.kernel_root };
    if kernel_kit::paging::Cr3::read() != root { kernel_kit::paging::Cr3::load(root); }
}

#[no_mangle]
pub extern "C" fn timer_interrupt_handler(rsp: u64) -> u64 {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    let mut next = rsp;
    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            next = sys.schedule_tick(rsp);
            activate(sys);
        }
        PICS.notify_end_of_interrupt(32);
    }
    next
}

#[no_mangle]
pub extern "C" fn syscall_interrupt_handler(rsp: u64) -> u64 {
    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            let next = kernel_orchestrator::syscall::dispatch(sys, rsp);
            activate(sys);
            return next;
        }
    }
    rsp
}

#[no_mangle]
pub extern "C" fn syscall_fast_handler(rsp: u64) -> u64 { syscall_interrupt_handler(rsp) }

/// CPU-exception handler. Fatal: prints a diagnostic and halts.
/// Replaces the old silent triple-fault on any #PF/#GP/#DF/etc. so we can
/// actually see what faulted during bring-up.
struct EmergencySerial;
impl core::fmt::Write for EmergencySerial {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        for byte in text.bytes() {
            for _ in 0..1000000 { if kernel_kit::io::Port::new(0x3fd).read() & 0x20 != 0 { break; } core::hint::spin_loop(); }
            kernel_kit::io::Port::new(0x3f8).write(byte);
        }
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn exception_handler(frame: *const u64, vector: u64) -> u64 {
    use core::fmt::Write;
    unsafe {
        let error = *frame.add(15);
        let rip = *frame.add(16);
        let cs = *frame.add(17);
        let rsp = *frame.add(19);
        let cr2: u64;
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
        let _ = writeln!(EmergencySerial, "\n{} vector={} error={:#x} rip={:#x} rsp={:#x} cr2={:#x}",
            if cs & 3 == 3 { "USER_FAULT" } else { "KERNEL_EXCEPTION" }, vector, error, rip, rsp, cr2);
        if cs & 3 == 3 {
            if let Some(sys) = &mut *(&raw mut SYSTEM) {
                sys.exit_current(128 + vector);
                let next = sys.scheduler.switch_context(frame as u64);
                activate(sys);
                return next;
            }
        }
        kernel_kit::io::Port::new(0xf4).write32(0x11);
        core::arch::asm!("cli");
        loop { core::arch::asm!("hlt"); }
    }
}

fn inject_payloads() {
    let shell_bytes = include_bytes!("../../target/x86_64-os/release/payload");
    let daemon_bytes = include_bytes!("../../target/x86_64-os/release/daemon");
    
    let worker_bytes = include_bytes!("../../target/x86_64-os/release/worker");
    let fault_bytes = include_bytes!("../../target/x86_64-os/release/fault-probe");
    let sleeper_bytes = include_bytes!("../../target/x86_64-os/release/sleeper");
    let desktop_bytes = include_bytes!("../../target/x86_64-os/release/desktop");
    // The boot image's programs appear read-only in /bin, served from the image itself.
    let fs = kernel_kit::fs::ROOT_FS.lock();
    for (name, bytes) in [("shell.elf", &shell_bytes[..]), ("daemon.elf", &daemon_bytes[..]), ("worker.elf", &worker_bytes[..]),
                          ("fault.elf", &fault_bytes[..]), ("sleeper.elf", &sleeper_bytes[..]), ("desktop.elf", &desktop_bytes[..])] {
        let _ = fs.install_builtin(name, bytes);
    }
    kernel_kit::fs::ROOT_FS.unlock();
}

// Now takes/returns rsp because keyboard_interrupt_wrapper does `mov rsp,rax`
// after calling us, mirroring the timer/syscall stubs. We never switch stacks
// here, so we just hand the incoming rsp back unchanged.
#[no_mangle]
pub extern "C" fn keyboard_interrupt_handler(rsp: u64) -> u64 {
    unsafe {
        // Queue presses and releases alike: releases carry modifier state
        // (Shift, Ctrl, Alt). Bytes from the auxiliary port belong to IRQ12.
        if kernel_kit::io::Port::new(0x64).read() & 0x20 == 0 {
            if let Some(scancode) = KEYBOARD.read_scancode() {
                let (buffer, kb_sif) = kernel_kit::io::KEYBOARD_BUFFER.lock();
                buffer.push(scancode, kernel_kit::io::input_clock());
                kernel_kit::io::KEYBOARD_BUFFER.unlock(kb_sif);
            }
        }
        PICS.notify_end_of_interrupt(33);
    }
    rsp
}

#[no_mangle]
pub extern "C" fn mouse_interrupt_handler(rsp: u64) -> u64 {
    unsafe {
        let status = kernel_kit::io::Port::new(0x64).read();
        if status & 0x21 == 0x21 {
            let byte = kernel_kit::io::Port::new(0x60).read();
            let (buffer, sif) = kernel_kit::io::MOUSE_BUFFER.lock();
            buffer.push(byte, kernel_kit::io::input_clock());
            kernel_kit::io::MOUSE_BUFFER.unlock(sif);
        }
        PICS.notify_end_of_interrupt(44);
    }
    rsp
}

/// i8042 controller helpers for PS/2 mouse setup (polled, before IRQs run).
fn ps2_wait_write() { for _ in 0..100_000 { if kernel_kit::io::Port::new(0x64).read() & 2 == 0 { return; } core::hint::spin_loop(); } }
fn ps2_read() -> Option<u8> {
    for _ in 0..100_000 {
        if kernel_kit::io::Port::new(0x64).read() & 1 != 0 { return Some(kernel_kit::io::Port::new(0x60).read()); }
        core::hint::spin_loop();
    }
    None
}
fn ps2_command(command: u8) { ps2_wait_write(); kernel_kit::io::Port::new(0x64).write(command); }
fn mouse_write(byte: u8) -> bool {
    ps2_command(0xd4);
    ps2_wait_write();
    kernel_kit::io::Port::new(0x60).write(byte);
    ps2_read() == Some(0xfa)
}

/// Enables the auxiliary PS/2 device in streaming mode, with the IntelliMouse
/// scroll wheel when the device supports it. Returns (present, wheel).
fn init_mouse() -> (bool, bool) {
    ps2_command(0xa8); // Enable the auxiliary port.
    ps2_command(0x20);
    let Some(config) = ps2_read() else { return (false, false) };
    ps2_command(0x60);
    ps2_wait_write();
    kernel_kit::io::Port::new(0x60).write((config | 2) & !0x20); // IRQ12 on, aux clock on.
    if !mouse_write(0xf6) { return (false, false); } // Defaults.
    // The IntelliMouse handshake: sample rates 200, 100, 80, then ID 3.
    for rate in [200u8, 100, 80] { mouse_write(0xf3); mouse_write(rate); }
    let wheel = mouse_write(0xf2) && ps2_read() == Some(3);
    let enabled = mouse_write(0xf4);
    (enabled, wheel)
}

use kernel_kit::gdt::{GlobalDescriptorTable, TaskStateSegment};

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Kernel stack pointer for syscall entry. Updated by the timer handler
/// and _start so the syscall trampoline can find the kernel stack.
#[no_mangle]
pub static mut KERNEL_STACK_PTR: u64 = 0;
#[no_mangle]
pub static mut USER_STACK_PTR: u64 = 0;
#[no_mangle]
pub static KERNEL_MXCSR: u32 = 0x1f80;

// ──────────────── GAP 3: syscall/sysret fast entry ────────────────
global_asm!(r#"
.global syscall_entry
syscall_entry:
    mov [rip + USER_STACK_PTR], rsp
    mov rsp, [rip + KERNEL_STACK_PTR]
    push 0x1b
    push qword ptr [rip + USER_STACK_PTR]
    push r11
    push 0x23
    push rcx
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
    mov rbx, rsp
    sub rsp, 512
    and rsp, -16
    fxsave64 [rsp]
    fninit
    ldmxcsr [rip + KERNEL_MXCSR]
    cld
    call syscall_fast_handler
    cmp rax, rbx
    jne restore_interrupt_context
    lea rsp, [rax - 512]
    and rsp, -16
    fxrstor64 [rsp]
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
    mov rcx, [rsp]
    mov r11, [rsp + 16]
    mov rsp, [rsp + 24]
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
        ALLOCATOR.init(heap_start, HEAP_BYTES - 512 * 1024);
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

    // Set up the physical frame allocator from every Usable region in the
    // bootloader's memory map (RAM is split around holes such as QEMU's PCI
    // hole below 4 GiB). These frames are genuine free RAM covered by the
    // bootloader's physical_memory_offset mapping, so
    // paging::phys_to_virt() works on them. The first 1 MiB is left alone:
    // physical address 0 doubles as "none" in several places, and the BIOS
    // area is reserved for firmware and future AP start-up code. This is required for duplicate_pml4
    // to write valid PML4 copies that CR3 can load (the prior triple-fault
    // blocker): without a real USABLE region, PML4 copies were written through
    // addresses that resolved to wrong RAM, corrupting every page-table entry.
    unsafe {
        use bootloader::bootinfo::MemoryRegionType;
        const LOW_MEMORY_FRAMES: u64 = 256;
        let (obj, sif_7) = kernel_kit::memory::FRAME_ALLOCATOR.lock();
        for region in boot_info.memory_map.iter() {
            if matches!(region.region_type, MemoryRegionType::Usable) {
                let s = region.range.start_frame_number.max(LOW_MEMORY_FRAMES);
                let e = region.range.end_frame_number;
                if e > s { obj.add_region((s * 4096) as usize, (e - s) as usize); }
            }
        }
        let (total, regions) = (obj.total_count(), obj.region_count());
        // Prove the physical-memory mapping reaches the top of every region
        // (including RAM above 4 GiB) before any of it is handed out.
        let mut highest = 0;
        for (_, end) in obj.regions() {
            let probe = kernel_kit::paging::phys_to_virt(end - 8) as *mut u64;
            core::ptr::write_volatile(probe, 0xa70a_5eed_0000_0000 | end);
            if core::ptr::read_volatile(probe) != 0xa70a_5eed_0000_0000 | end {
                panic!("physical memory probe failed at {:#x}", end - 8);
            }
            core::ptr::write_volatile(probe, 0);
            highest = highest.max(end);
        }
        kernel_kit::memory::FRAME_ALLOCATOR.unlock(sif_7);
        if total < 256 {
            panic!("No usable memory region found for frame allocator");
        }
        use core::fmt::Write;
        let _ = writeln!(EmergencySerial, "MEMORY_READY frames={} mib={} regions={} top={:#x} probe=ok", total, total / 256, regions, highest);
    }
    let (obj, sif_3) = kernel_kit::serial::SERIAL1.lock();
    obj.send(b'B');
    kernel_kit::serial::SERIAL1.unlock(sif_3);
    
    match kernel_kit::storage::mount() {
        Ok(generation) => { use core::fmt::Write; let _ = writeln!(EmergencySerial, "STORAGE_READY generation={}", generation); }
        Err(error) => { use core::fmt::Write; let _ = writeln!(EmergencySerial, "STORAGE_UNAVAILABLE {:?}", error); }
    }
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
        IDT.set_handler(44, mouse_interrupt_wrapper as *const () as u64);
        
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
        kernel_kit::io::Port::new(0x43).write(0x36);
        kernel_kit::io::Port::new(0x40).write((11932u16 & 255) as u8);
        kernel_kit::io::Port::new(0x40).write((11932u16 >> 8) as u8);
        PICS.unmask(0);
        PICS.unmask(1);
        let (present, wheel) = init_mouse();
        if present {
            let (decoder, sif) = kernel_kit::input::MOUSE.lock();
            decoder.wheel = wheel;
            kernel_kit::input::MOUSE.unlock(sif);
            PICS.unmask(2); // Cascade line for the secondary PIC.
            PICS.unmask(12);
        }
        use core::fmt::Write;
        let _ = writeln!(EmergencySerial, "INPUT_READY mouse={} wheel={} display={}", present, wheel,
            kernel_kit::display::find().is_some());
    }
    vga.write_string("PIC Initialized.\n");

    unsafe {
        SYSTEM = Some(System::new(kernel_kit::paging::Cr3::read()));
    }
    vga.write_string("Orchestrator Initialized.\n");
    
    // Setup TSS and GDT
    unsafe {
        GDT.set_tss(&raw const TSS);
        GDT.load();
        GDT.load_tss();
        // GAP 3: configure syscall/sysret MSRs after GDT is loaded.
        setup_syscall_msr();
        KERNEL_STACK_PTR = TSS.privilege_stack_table[0];
    }

    // Runtime loader and boot loader use the same validated ownership path.
    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            sys.spawn_program(0, "shell.elf").expect("load shell");
            sys.spawn_program(0, "daemon.elf").expect("load daemon");
        }
    }
    vga.write_string("Ring 3 Multi-Tasking Spawned.\n");

    vga.write_string("System running autonomously. Awaiting hardware events...\n");

    // 3. Enable Interrupts
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    
    loop {
        // Idle the CPU until an interrupt occurs
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
