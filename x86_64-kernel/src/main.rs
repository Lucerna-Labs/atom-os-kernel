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
use core::sync::atomic::{AtomicUsize, Ordering};

#[global_allocator]
static ALLOCATOR: AtomHeap = AtomHeap(Spinlock::new(BumpAllocator::new()));

// 1MB statically allocated byte array in the `.bss` section to serve as our physical heap.
static mut HEAP_MEM: [u8; 1024 * 1024] = [0; 1024 * 1024];

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

extern "C" {
    fn timer_interrupt_wrapper();
    fn syscall_interrupt_wrapper();
}

static mut SYSTEM: Option<System> = None;

#[no_mangle]
pub extern "C" fn timer_interrupt_handler(rsp: u64) -> u64 {
    TIMER_TICKS.fetch_add(1, Ordering::SeqCst);
    let mut new_rsp = rsp;
    
    unsafe {
        if let Some(sys) = &mut *(&raw mut SYSTEM) {
            new_rsp = sys.scheduler.switch_context(rsp);
            if let Some(next_task) = sys.scheduler.current_task() {
                if kernel_kit::paging::Cr3::read() != next_task.page_table_root {
                    kernel_kit::paging::Cr3::load(next_task.page_table_root);
                }
                TSS.privilege_stack_table[0] = next_task.kernel_stack;
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
                task.rsp = rsp;
                let mut mem = kernel_kit::memory::MemoryPool::new();
                
                // Read syscall number before dispatch
                let frame_ptr = task.rsp as *mut kernel_kit::trap::TrapFrame;
                let sys_num = (*frame_ptr).rax;
                
                kernel_orchestrator::syscall::dispatch(task, &mut mem);
                new_rsp = task.rsp;
                
                // Check if it was SYS_YIELD (syscall 1) or SYS_EXIT (syscall 3)
                if sys_num == 1 || sys_num == 3 {
                    new_rsp = sys.scheduler.switch_context(new_rsp);
                }
            }

            if let Some(next_task) = sys.scheduler.current_task() {
                if kernel_kit::paging::Cr3::read() != next_task.page_table_root {
                    kernel_kit::paging::Cr3::load(next_task.page_table_root);
                }
                TSS.privilege_stack_table[0] = next_task.kernel_stack;
            }
        }
        PICS.notify_end_of_interrupt(0x80); // Optional if using software interrupt `int 0x80`
    }
    
    new_rsp
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

    let mut tf = TrapFrame::new_user(0, 0x200000);
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

pub extern "C" fn keyboard_interrupt_handler() {
    unsafe {
        if let Some(scancode) = KEYBOARD.read_scancode() {
            // Push the scancode into the global ring buffer instead of writing it immediately
            // We only care about key presses (scancode < 0x80), ignore key releases.
            if scancode < 0x80 {
                let buffer = kernel_kit::io::KEYBOARD_BUFFER.lock();
                buffer.push(scancode);
                kernel_kit::io::KEYBOARD_BUFFER.unlock();
            }
        }
        PICS.notify_end_of_interrupt(33);
    }
}

use kernel_kit::gdt::{GlobalDescriptorTable, TaskStateSegment};

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();

#[no_mangle]
pub extern "C" fn _start() -> ! {
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
    kernel_kit::serial::SERIAL1.lock().init();
    kernel_kit::serial::SERIAL1.unlock();

    kernel_kit::serial::SERIAL1.lock().send(b'A');
    kernel_kit::serial::SERIAL1.unlock();

    let mut vga = kernel_kit::vga::VgaWriter::new();
    vga.write_string("Booting Fearless Hypatia...\n");
    
    // 0.5 Setup Physical Heap Allocator
    unsafe {
        let heap_start = HEAP_MEM.as_ptr() as usize;
        let heap_size = HEAP_MEM.len();
        ALLOCATOR.0.lock().init(heap_start, heap_size);
        ALLOCATOR.0.unlock();
    }
    kernel_kit::serial::SERIAL1.lock().send(b'B');
    kernel_kit::serial::SERIAL1.unlock();
    
    inject_payloads();
    kernel_kit::serial::SERIAL1.lock().send(b'C');
    kernel_kit::serial::SERIAL1.unlock();
    
    // Test the allocator native to Rust
    let mut test_vec = alloc::vec::Vec::new();
    test_vec.push(42);
    if test_vec[0] == 42 {
        vga.write_string("Heap Allocation Test: PASS!\n");
    }
    
    kernel_kit::serial::SERIAL1.lock().send(b'D');
    kernel_kit::serial::SERIAL1.unlock();

    // 1. Setup IDT
    unsafe {
        IDT.set_handler(32, timer_interrupt_wrapper as *const () as u64);
        IDT.set_handler(33, keyboard_interrupt_handler as *const () as u64);
        
        // Use set_handler_user to set DPL=3, allowing Ring 3 to trigger the interrupt
        IDT.set_handler_user(0x80, syscall_interrupt_wrapper as *const () as u64);
        
        IDT.load();
    }
    kernel_kit::serial::SERIAL1.lock().send(b'E');
    kernel_kit::serial::SERIAL1.unlock();

    // 2. Setup PIC
    unsafe {
        PICS.initialize(32, 40); // Map PIC1 to IRQ 32-39, PIC2 to IRQ 40-47
    }
    vga.write_string("PIC Initialized.\n");

    unsafe {
        SYSTEM = Some(System::new());
    }
    vga.write_string("Orchestrator Initialized.\n");
    
    // Setup TSS and GDT
    unsafe {
        GDT.set_tss(&raw const TSS);
        GDT.load();
        GDT.load_tss();
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
