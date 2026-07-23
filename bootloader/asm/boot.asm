# ATOM Bootloader - 100% Dependency-Free
# Built from math primitives: scan, hash, fold, project, scale, compare, combine, order
# 
# Boot sequence:
# 1. BIOS/UEFI loads this at 0x7C00 (or multiboot loads at 0x100000)
# 2. Assembly stub sets up initial state
# 3. Switch to protected mode (32-bit)
# 4. Set up GDT and paging
# 5. Switch to long mode (64-bit)
# 6. Load kernel from disk
# 7. Jump to kernel entry point

.section .multiboot
.align 4
.long 0xE85250D6                # magic number (multiboot 2)
.long 0                         # architecture 0 (protected mode i386)
.long multiboot_end - multiboot_start  # header length
.long 0x100000000 - (0xE85250D6 + 0 + (multiboot_end - multiboot_start))  # checksum

multiboot_start:
# Optional tags can go here
.long 0                         # end tag type
.long 8                         # end tag size
multiboot_end:

.section .bss
.align 16
stack_bottom:
.skip 16384                     # 16 KB stack
stack_top:

# GDT for protected mode and long mode
.align 16
gdt64:
.null:
    .quad 0
.code: equ $ - gdt64
    .quad (1<<43) | (1<<44) | (1<<47) | (1<<53)  # Code segment
.data: equ $ - gdt64
    .quad (1<<44) | (1<<47) | (1<<41)            # Data segment
.pointer:
    .word $ - gdt64 - 1
    .quad gdt64

.section .text
.code32
.global _start
_start:
    # Disable interrupts
    cli
    
    # Save multiboot info
    mov %eax, multiboot_magic
    mov %ebx, multiboot_info
    
    # Set up stack
    mov $stack_top, %esp
    
    # Check for CPUID
    pushf
    pop %eax
    mov %eax, %ecx
    xor $0x200000, %eax
    push %eax
    popf
    pushf
    pop %eax
    xor %ecx, %eax
    jz no_cpuid
    
    # Check for long mode support
    mov $0x80000000, %eax
    cpuid
    cmp $0x80000001, %eax
    jb no_long_mode
    
    mov $0x80000001, %eax
    cpuid
    test $(1<<29), %edx
    jz no_long_mode
    
    # Enable PAE (Physical Address Extension)
    mov %cr4, %eax
    or $(1<<5), %eax
    mov %eax, %cr4
    
    # Set up page tables for identity mapping (first 2MB)
    # PML4 -> PDPT -> PD -> 2MB pages
    
    # Clear page tables
    mov $page_tables_start, %edi
    xor %eax, %eax
    mov $4096 * 4, %ecx
    rep stosb
    
    # PML4[0] -> PDPT
    mov $page_tables_start, %edi
    mov $(pdpt - page_tables_start + 0x3), (%edi)  # Present + Writable
    
    # PDPT[0] -> PD
    mov $pdpt, %edi
    mov $(pd - page_tables_start + 0x3), (%edi)
    
    # PD[0] -> 2MB page at 0x0
    mov $pd, %edi
    mov $0x83, (%edi)  # Present + Writable + PageSize (2MB)
    
    # Load PML4
    mov $page_tables_start, %eax
    mov %eax, %cr3
    
    # Enable long mode (set LME bit in IA32_EFER MSR)
    mov $0xC0000080, %ecx
    rdmsr
    or $(1<<8), %eax     # LME bit
    wrmsr
    
    # Enable paging
    mov %cr0, %eax
    or $(1<<31), %eax    # PG bit
    mov %eax, %cr0
    
    # Load 64-bit GDT
    lgdt gdt64.pointer
    
    # Jump to 64-bit code
    ljmp $gdt64.code, $long_mode_start

no_cpuid:
    hlt
    jmp no_cpuid

no_long_mode:
    hlt
    jmp no_long_mode

.code64
long_mode_start:
    # Set up data segments
    mov $gdt64.data, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Call Rust bootloader code
    call bootloader_main
    
    # Should not return, but if it does, halt
.halt:
    hlt
    jmp .halt

.section .bss
.align 4096
page_tables_start:
.skip 4096          # PML4
pdpt:
.skip 4096          # PDPT
pd:
.skip 4096          # PD

.section .data
.align 8
multiboot_magic:
    .quad 0
multiboot_info:
    .quad 0
