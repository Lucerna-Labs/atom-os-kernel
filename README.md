# Atom OS Kernel

An experimental x86-64 operating-system kernel written in Rust, with a command
shell, isolated user processes, a real userspace heap, IPC, and persistent files
on a virtual block device.

This repository contains the kernel, userspace runtime, bundled programs, and
build and verification tools. Its core library defines eight root operations:
**scan, hash, fold, project, scale, compare, combine, and order**. Their definitions
live in [`kernel-kit/src/atoms.rs`](kernel-kit/src/atoms.rs); the kernel and
orchestrator compose mechanisms around them.

**Current stage:** a bootable development kernel verified in QEMU with KVM and
software emulation. Hardware installation and multiprocessor operation have not
been validated.

[Verified results](#verified-results) · [Build and test](#build-and-test) ·
[Shell commands](#shell-commands) · [Architecture](#architecture) ·
[Current limits](#persistent-storage-and-current-limits) · [Next milestones](#next-milestones)

## What works

- **Protected processes:** private address spaces, validated ELF loading,
  write/execute permissions, stack guards, and checked syscall buffers.
- **Process lifecycle:** `spawn`, parent-owned `wait`, exit status, sleeping,
  orphan cleanup, and resource reclamation after a process stops using its
  address space and kernel stack.
- **Memory allocation:** a kernel slab allocator with reusable large/aligned
  allocations, plus a Rust userspace allocator backed by mapped private pages.
- **Scheduling and syscalls:** round-robin preemption, `int 0x80` and fast
  `syscall/sysret` paths, and saved integer/x87/SSE/MXCSR context.
- **Files and IPC:** a flat in-memory filesystem with stable open-file objects,
  file removal and rename that refuse to touch open or built-in files,
  read-only embedded executables, and bounded per-process message queues.
- **Process control:** `ps` lists live and exited processes with their parent,
  state and program name; `kill` ends a process, whose parent then collects
  status **137**.
- **Persistent storage:** a legacy/transitional virtio-blk PCI driver with
  FLUSH support and two checksummed snapshot slots.
- **Diagnostics:** serial crash reports and containment of user faults, with a
  dedicated fault probe that verifies the shell survives.

## Verified results

The latest committed acceptance evidence is from **September 5, 2026**. These
are recorded results from the development VM, not a claim that every later CI
run has passed.

| Verification | Recorded result |
|---|---|
| Native regression suite | **21 passed, 0 failed** |
| QEMU with KVM | **16 acceptance checks passed**; acceleration confirmed by QMP |
| QEMU with TCG software emulation | **16 acceptance checks passed** on the same boot image |
| Process creation, wait, and reaping | **48 measured cycles** per backend, following warmup |
| Free frames after those cycles | **16,326 before → 16,326 after** on both backends |
| Persistent files | Exact generated text survived a warm OS reboot and a fresh QEMU cold boot |
| Executable replacement | Worker replaced the shell, exited, and left the daemon running |

**October 3, 2026 update:** after adding `rm`, `mv`, `ps` and `kill`, the
native suite recorded **23 passed, 0 failed** and the TCG acceptance run recorded
**19 checks passed**, including the new `PS_KILL_REAP`, `FILE_REMOVE_RENAME` and
`REMOVE_RENAME_PERSISTENCE` checks. That run used QEMU 8.2.2 (TCG only; KVM was
not available) and the pinned toolchain; its logs were not committed.

The acceptance suite also exercises keyboard input, file reads/writes, exact IPC
delivery, concurrent processes, invalid-pointer/ELF rejection, fast syscalls,
SIMD state across timer preemption, and a contained user page fault.
Interrupted-write/flush recovery is covered by **native fault-injection tests**;
real-VM interruption during a save is a planned extension.

Evidence:

- [Combined verification record](test-results/run-20260905T221621361972313Z/verification.json)
- [Native test output](test-results/run-20260905T221621361972313Z/native.log)
- [KVM acceptance result](test-results/run-20260905T221621361972313Z/acceptance/result.json)
- [TCG acceptance result](test-results/run-20260905T221621361972313Z/tcg/result.json)
- [Cold-boot serial transcript](test-results/run-20260905T221621361972313Z/acceptance/cold/serial.log)
- [Implementation report](test-results/implementation-20260905/REPORT.md)

The reference environment was Ubuntu 24.04.4 on x86-64 with QEMU 8.2.2,
bootimage 0.10.5, and Rust `1.100.0-nightly (8fa1c96cf 2026-08-17)` from the
pinned `nightly-2026-08-18` toolchain. Compiler warnings remain in the low-level
code; the build is not described as warning-free.

## Build and test

### Prepare an x86-64 Linux environment

Install [rustup](https://rust-lang.github.io/rustup/installation/index.html)
first. On Ubuntu/Debian, install the host tools and the pinned Rust components:

```sh
sudo apt-get update
sudo apt-get install -y build-essential git python3 qemu-system-x86

rustup toolchain install nightly-2026-08-18 --profile minimal \
  --component rust-src --component llvm-tools-preview

# Install this host tool from outside a Cargo workspace.
cargo +nightly-2026-08-18 install bootimage --version 0.10.5 --locked
```

The toolchain is pinned in [`rust-toolchain.toml`](rust-toolchain.toml). Use the
pinned version when reproducing the recorded results.

### Clone, build, and run acceptance

Authenticate with an account that has access to this private repository before
cloning:

```sh
git clone https://github.com/Rekonquest/atom-os-kernel.git
cd atom-os-kernel

bash scripts/build.sh
bash scripts/test-native.sh
python3 scripts/boot-test.py --accel tcg --output test-results/local-tcg
```

[`build.sh`](scripts/build.sh) builds the shell, daemon, worker and fault probe
before embedding them in the kernel. The resulting boot image is:

```text
target/x86_64-os/release/bootimage-x86_64-kernel.bin
```

Use [`test-native.sh`](scripts/test-native.sh) for native checks and
[`boot-test.py`](scripts/boot-test.py) for OS acceptance. The acceptance run is
headless, injects keyboard input through QMP, and records serial output and
machine-readable results. Each run requires a **new output directory**, creates
an isolated **8 MiB test disk**, and stops its QEMU instances when finished.

For hardware acceleration on a machine with KVM available:

```sh
python3 scripts/boot-test.py --accel kvm --output test-results/local-kvm
```

The KVM path requires access to `/dev/kvm`. If direct access is unavailable, the
script attempts its existing passwordless `sudo -n setpriv` route using the
`kvm` group. That fallback must already be permitted on the machine. TCG does
not require KVM access.

For builds using cached dependencies only:

```sh
ATOM_OFFLINE=1 bash scripts/build.sh
```

### Existing `atom-os-dev` development VM

The maintainer's dedicated VM is already configured for this project. From its
host machine, run:

```sh
bash scripts/vm-test.sh
```

This wrapper uses the libvirt system connection, the `atom-os-dev` VM and SSH
alias, and `/home/jesse/atom-os-kernel-tests` inside that VM. It starts the VM if
needed, copies the current working tree into a new timestamped directory,
builds and tests it, and retrieves the results under `test-results/run-*`.
`test-results/latest-run.txt` records the latest local result directory.

The wrapper is specific to that existing setup; the direct Linux commands above
are the starting point for another machine. It preserves previous runs and the
original VM workspace. A reusable interactive session with a continuing data
disk is planned separately from this acceptance harness.

## Shell commands

| Command | Behavior |
|---|---|
| `help`, `ls`, `clear` | List commands/files or clear the screen |
| `cat file` | Read a file |
| `rm file` | Remove a writable file that no process has open |
| `mv old new` | Rename a writable file; fails if `new` exists |
| `echo text > file` | Append text and a newline |
| `edit file` | Edit up to 64 KiB; Esc saves the RAM copy |
| `msg text` | Send a message to the daemon; a full mailbox returns an error |
| `bench`, `heaptest`, `stats` | Exercise yields/heap or show free frames and live tasks |
| `spawn worker.elf` | Start a child while the shell continues; print its PID |
| `wait PID` | Wait for a child of this shell and collect its exit status |
| `ps` | List processes: PID, parent, state and program name |
| `kill PID` | End a process; its parent's `wait` returns status 137 |
| `run worker.elf` | Replace the shell process with the executable |
| `selftest`, `pairtest` | Run one worker or two concurrent workers and check their exits |
| `churn 48` | Exercise repeated process creation/reaping and compare free-frame counts |
| `faulttest` | Confirm a user page fault terminates that worker while the shell survives |
| `sync` | Commit writable files to the data disk |
| `reboot` | Sync successfully, then reboot the OS |

The current [PS/2 keyboard mapping](kernel-kit/src/keyboard.rs) uses the numpad
`+` key to enter `>` for redirection.

`rm` and `mv` change RAM files only; use `sync` to make the change durable.
Built-in programs cannot be removed or renamed, and a file held open by any
process cannot be removed.

The kernel embeds five read-only programs: `shell.elf`, `daemon.elf`,
`worker.elf`, `fault.elf` and `sleeper.elf`. The diagnostic worker checks heap contents,
untrusted pointers, file handles, fast syscalls, SIMD context and IPC, then exits
with status **37**. The worker also checks remove/rename rules and its own
process-list entry. `sleeper.elf` sleeps until killed, for `ps`/`kill` tests. The fault probe touches a stack guard page and produces
status **142**, which its parent checks.

The shared syscall definitions are in [`abi.rs`](abi.rs). Existing byte-I/O and
IPC calls remain available alongside the newer process and memory operations.
`SYS_ALLOC` returns mapped process-private memory; `SYS_FREE` releases it.
`SYS_REMOVE` and `SYS_RENAME` take NUL-terminated names. `SYS_PROCESSES` copies
fixed 64-byte records into a checked, writable user buffer, and `SYS_KILL`
terminates a process by PID.

## Architecture

| Location | Responsibility |
|---|---|
| [`x86_64-kernel/`](x86_64-kernel) | Boot entry, CPU/interrupt setup, context activation, program embedding and crash diagnostics |
| [`kernel-kit/`](kernel-kit) | Root atoms, allocation, address spaces, ELF validation, filesystem, hardware I/O and storage |
| [`kernel-orchestrator/`](kernel-orchestrator) | Scheduling, process loading/lifecycle and syscall dispatch |
| [`user-rt/`](user-rt) | Userspace entry, syscall wrappers, console helpers and allocator |
| [`payload/`](payload) | Command shell |
| [`daemon/`](daemon) | IPC receiver and heartbeat process |
| [`worker/`](worker) | Diagnostic worker, fault-probe and sleeper executables |
| [`tests/`](tests) and [`scripts/`](scripts) | Native regression tests, builds and VM acceptance |

Each process owns its image, stacks, heap allocations, receive page and copied
page-table paths. Kernel mappings remain supervisor-only. The loader validates
ELF headers and ranges before mapping; syscall buffers are checked before use.
The scheduler releases exited resources only after their CR3 and kernel stack
are inactive. Exit status remains available to the parent until `wait`, and
orphaned exits are collected automatically.

The [design document](ATOM-STACK-KERNEL-DESIGN.md) and [working notes](NOTES.md)
retain the project's architecture reasoning and historical investigations.
Their dated plans and earlier failure states should be read alongside the
current source and verification evidence.

## Persistent storage and current limits

The filesystem uses stable boxed file objects in RAM. A successful `sync`
serializes writable files into an inactive disk slot, flushes the payload,
writes a checksummed header, and flushes again. Mount selects the newest
complete valid generation. Embedded executables come from the boot image and
are excluded from disk snapshots.

The driver uses the legacy/transitional virtio-blk PCI interface with one polled
queue and negotiated FLUSH support. The test harness attaches the data disk
with `virtio-blk-pci,disable-modern=on`; the storage contracts follow the
[OASIS Virtio specification](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html).

| Resource | Current bound |
|---|---|
| Task slots | 16 |
| User stack | 32 KiB per process, with a guard page |
| Heap address window | 16 MiB per process |
| Individual user allocation | Up to 1 MiB |
| Physical frame pool | Capped at 64 MiB |
| Open file descriptors | 16 per process |
| IPC queue | 4 messages per recipient; up to 255 message bytes |
| RAM directory | 256 entries, including embedded programs |
| Persistent writable files | 128 |
| Filename | 63 bytes; flat names |
| File contents | Up to 64 KiB per writable file |
| Serialized snapshot | 512 KiB total, including filenames and metadata |

**RAM edits are durable only after `sync` succeeds.** The RAM directory and disk
format have different capacity limits, so a RAM write may succeed even when a
later `sync` cannot save everything; `rm` can free space before retrying. Capacity reporting is a
planned improvement. Without a compatible disk, the kernel reports
`STORAGE_UNAVAILABLE` and continues with RAM files. The disk format is specific
to Atom OS.

## Next milestones

These are proposed work, not currently implemented features:

- Consistent durable-capacity enforcement and clear saved/unsaved status.
- Recovery tests that interrupt actual virtio writes and flushes in the VM.
- An interactive launcher that reuses a persistent development disk.
- Program arguments for spawned processes.

## CI and preserved history

The [Test OS workflow](.github/workflows/test.yml) builds all bundled programs
and the boot image, runs native regressions and TCG acceptance, and retains
failure artifacts. Check [GitHub Actions](https://github.com/Rekonquest/atom-os-kernel/actions)
for the status of a particular commit.

Selected reports and raw logs are committed under [`test-results/`](test-results),
including the [original failed baseline](test-results/20260905T202917Z/REPORT.md).
Generated images, test data disks, caches and large trace archives remain local.
[BACKUP.md](BACKUP.md) explains the backup scope and the inherited, unused
`atom-3d-engine` Git link.
