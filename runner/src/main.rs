use kernel_kit::context::Context;
use kernel_kit::memory::MemoryPool;
use kernel_orchestrator::system::System;
use kernel_orchestrator::syscall::SYS_ALLOC;

fn main() {
    println!("Booting Agnostic OS Kernel (Phase 2: Syscalls & Virtual Memory)...");

    let mut _mem = MemoryPool::new();
    let mut sys = System::new();
    
    // Spawn Task 1
    let mut task1 = Context::new(100, 0);
    
    // Simulate a program issuing an allocation syscall for virtual address 0xDEADBEEF
    task1.r1 = SYS_ALLOC; 
    task1.r0 = 0xDEADBEEF;

    sys.scheduler.spawn(task1).expect("Failed to spawn Task 1");

    println!("Task 1 spawned and requesting virtual memory mapping for 0xDEADBEEF.");
    println!("Beginning execution fold...");

    // Run 10 cycles
    sys.run(10);

    println!("Execution fold complete.");

    // Verify the mapping
    // We can peek into the scheduler to see if the page table mapped it correctly.
    let ctx = sys.scheduler.next_task().unwrap();
    if let Some(phys) = ctx.page_table.translate(0xDEADBEEF) {
        println!("SUCCESS: Virtual address 0xDEADBEEF mapped to physical block {}", phys);
        println!("Task registers after syscall: r0={}, r1={}, pc={}", ctx.r0, ctx.r1, ctx.pc);
    } else {
        println!("FAILED: Memory was not mapped.");
    }
}
