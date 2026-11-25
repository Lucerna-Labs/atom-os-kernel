# Where to pick up

Open work for the standalone OS (security/brain crates removed from this repository). Finished items are removed, not struck through; git history keeps them.

## Kernel hardening & architecture

1. **Kernel hardening**:
   - **Done (`9ffbf4f`, `477278e`)**: `CR0.WP`, CPUID-gated `EFER.NXE`, `CR4.SMEP`,
     `CR4.SMAP`, `IA32_FMASK` flag hardening (clears `IF`/`TF`/`DF`/`AC` on
     `SYSCALL`), full `copy_from_user` syscall isolation, streaming disk ELF
     loader (no full-file kernel heap copy), and process-tree kill authorization.
   - **Open**: KASLR, Spectre/BHI/IBPB mitigations, kernel stack guards / red-zone
     checks, fine-grained capabilities/user permissions, and the next-generation
     Atom security sensors and enforcement layer.

## Kernel performance & scalability (deferred — do afterwards)

Baseline measured at `477278e` (`kvm/host` via `scripts/perf-bench.py`): `85 ns`
`SYS_GETPID` (`syscall`), `0.6%` desktop host idle CPU, `12 KiB` kernel heap /
`99%` slab hit rate, `4.87 GiB/s` write & `7.55 GiB/s` read RAMFS sequential I/O,
`4.6 ms` (`4,613–5,255 us`) 1080p desktop frame render. Remaining gaps vs. Linux
to tackle after security hardening:

2. **Soft-float kernel build (`-mmx,-sse,+soft-float`)**: separate kernel vs.
   userspace target specs so `syscall_entry` skips the 512-byte
   `fxsave64`/`fxrstor64` on non-switching syscalls and only swaps FPU/SSE
   state on task context switches.
3. **Non-contiguous paging & process table scaling**: replace `AddressSpace`
   physically contiguous runs (`alloc_contiguous`) with per-page frame lists,
   add demand paging and shared read-only `.text` mappings, and lift the fixed
   `MAX_PROCESSES = 16` table limit.
4. **Interrupt-driven VirtIO block & net I/O**: replace busy-spin polling and the
   single in-flight 128 KiB bounce-buffer transfer in
   `kernel-kit/src/virtio_blk.rs` with IRQ completion and multi-descriptor
   scatter-gather DMA.
5. **PCID / ASID TLB tagging**: enable `CR4.PCIDE` where supported to avoid full
   TLB flushes across `CR3` switches.
6. **Tickless APIC timer**: replace continuous 100 Hz legacy PIT interrupts with
   one-shot LAPIC timer scheduling so idle host CPU drops toward `<0.1%`.
7. **SMP multi-core bring-up**: AP startup, per-CPU runqueues/stacks, and
   finer-grained locking beyond `-smp 1`.
8. **GPU-accelerated desktop compositing** (optional later; the current CPU
   renderer stays frozen as the temporary visual inspection UI).

## Desktop, programs and files

9. **Remaining built-in apps to windowed programs**: Files, Text Editor,
   Terminal, System Monitor; `ui-intent` needs list, scroll and text-area
   elements first (deferred while desktop UI stays frozen for kernel security
   work).
10. **Program files on disk.** Manifests in `/apps` can name any path, but
    program files still arrive only with the boot image. Needs a safe write
    path plus the ELF check at load.
11. **Real hardware**: UEFI boot with the firmware framebuffer, AHCI/NVMe
    disks, USB keyboard and mouse. Today the desktop, disk and mouse need
    QEMU's devices. The from-scratch `bootloader/` (Lucerna-Labs line) is in
    the tree but not used for the image yet.
12. **Linux programs** (handed to another coder):
    [parked/linux-programs.md](parked/linux-programs.md). The
    `kernel-linux` draft is outside the workspace. Do not pick this up here.

## Housekeeping

13. **Notes & scripts**:
    - **`NOTES.md`** still describes the July context-switch bug as "not yet
      fixed"; the current scheduler restores every task from its full trap
      frame. Retire or rewrite the note.
    - **`scripts/build.sh` has no execute bit**; run it as `bash scripts/build.sh`
      or restore the bit.

## How to resume any item

- `scripts/test-native.sh` for the host gates, `scripts/boot-test.py --accel
  kvm --cpu host --output <dir>` for the boot checks,
  `python3 scripts/perf-bench.py --accel kvm --cpu host --baseline target/perf-baseline.json`
  for before/after speed & hardware-usage tracking, and `scripts/vm-test.sh`
  for the full suite on the `atom-os-dev` VM.
