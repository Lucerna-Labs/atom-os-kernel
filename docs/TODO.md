# Where to pick up

Open work for the standalone OS (security/brain crates removed from this repository). Finished items are removed, not struck through; git history keeps them.

## Kernel hardening & architecture

1. **Kernel hardening**: SMEP/SMAP, user-page permissions, and a decision on
   multiprocessor support. Nothing started.

## Desktop, programs and files

2. **Remaining built-in apps to windowed programs**: Files, Text Editor,
   Terminal, System Monitor; `ui-intent` needs list, scroll and text-area
   elements first.
3. **Program files on disk.** Manifests in `/apps` can name any path, but
   program files still arrive only with the boot image. Needs a safe write
   path plus the ELF check at load.
4. **Real hardware**: UEFI boot with the firmware framebuffer, AHCI/NVMe
   disks, USB keyboard and mouse. Today the desktop, disk and mouse need
   QEMU's devices. The from-scratch `bootloader/` (Lucerna-Labs line) is in
   the tree but not used for the image yet.
5. **Linux programs** (handed to another coder):
   [parked/linux-programs.md](parked/linux-programs.md). The
   `kernel-linux` draft is outside the workspace. Do not pick this up here.

## Housekeeping

6. **Notes & scripts**:
   - **`NOTES.md`** still describes the July context-switch bug as "not yet
     fixed"; the current scheduler restores every task from its full trap
     frame. Retire or rewrite the note.
   - **`scripts/build.sh` has no execute bit**; run it as `bash scripts/build.sh`
     or restore the bit.

## How to resume any item

- `scripts/test-native.sh` for the host gates, `scripts/boot-test.py --accel
  kvm --output <dir>` for the boot checks, `scripts/vm-test.sh` for the
  full suite on the `atom-os-dev` VM.
