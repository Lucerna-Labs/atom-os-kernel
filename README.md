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
  argument passing, process inspection, explicit termination, orphan cleanup,
  and resource reclamation after a process stops using its
  address space and kernel stack.
- **Memory allocation:** a kernel slab allocator with reusable large/aligned
  allocations, plus a Rust userspace allocator backed by mapped private pages.
- **Scheduling and syscalls:** round-robin preemption, `int 0x80` and fast
  `syscall/sysret` paths, and saved integer/x87/SSE/MXCSR context.
- **Files and IPC:** a flat filesystem with owned open handles, safe removal and
  rename, pre-write durable-capacity checks, saved-state reporting, read-only
  embedded executables, and bounded per-process message queues.
- **Persistent storage:** a legacy/transitional virtio-blk PCI driver with
  FLUSH support and two checksummed snapshot slots.
- **Diagnostics:** serial crash reports and containment of user faults, with a
  dedicated fault probe that verifies the shell survives.

## Verified results

The latest VM verification was completed on **September 17, 2026**. These are
recorded results from the development VM, not a claim that every later CI run
has passed. The earlier September 5 baseline remains preserved in test history.

| Verification | Recorded result |
|---|---|
| Native regression suite | **32 passed, 0 failed** |
| QEMU with KVM | **21 acceptance checks passed**; acceleration confirmed by QMP |
| QEMU with TCG software emulation | **21 acceptance checks passed** on the same boot image |
| Process creation, wait, and reaping | **48 measured cycles** per backend, following warmup |
| Forced termination and reaping | **48 additional cycles** per backend with open unlinked files and live heap allocations |
| Free frames after process churn | **16,321 before → 16,321 after** on both backends |
| Persistent files | Exact generated text survived a warm OS reboot and a fresh QEMU cold boot |
| Executable replacement | Worker replaced the shell, exited, and left the daemon running |
| Interrupted storage / I/O failures | **7 cases passed per backend**, at a full 512 KiB serialized snapshot |
| Interactive console | **2 sessions per backend**, exact retained contents and exclusive-image locking |

The acceptance suite also exercises keyboard input, file reads/writes, exact IPC
delivery, concurrent processes, invalid-pointer/ELF rejection, fast syscalls,
SIMD state across timer preemption, and a contained user page fault.
The recovery suite additionally suspends actual virtio requests at QEMU block
breakpoints, terminates the OS guest, and cold-boots the same test disk. It
checks payload writes, payload flushes, header writes and header flushes, plus
retry after injected I/O and host-space errors. This exercises abrupt guest
termination; it does not model a physical host losing its disk cache.

Evidence:

- [Combined verification record](test-results/run-20260917T212634980705680Z/verification.json)
- [Native test output](test-results/run-20260917T212634980705680Z/native.log)
- [KVM acceptance result](test-results/run-20260917T212634980705680Z/acceptance/result.json)
- [TCG acceptance result](test-results/run-20260917T212634980705680Z/tcg-acceptance/result.json)
- [Cold-boot serial transcript](test-results/run-20260917T212634980705680Z/acceptance/cold/serial.log)
- [Program arguments and process-control report](test-results/process-controls-20260917/REPORT.md)
- [Filesystem and session implementation report](test-results/durable-file-session-20260917/REPORT.md)
- [KVM recovery cases](test-results/run-20260917T212634980705680Z/recovery/result.json)
- [TCG recovery cases](test-results/run-20260917T212634980705680Z/tcg-recovery/result.json)
- [Interactive console results](test-results/run-20260917T212634980705680Z/console-process/result.json)

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
python3 scripts/test-storage-recovery.py --accel tcg --output test-results/local-recovery
python3 scripts/test-console.py --accel tcg --output test-results/local-console
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
original VM workspace, and runs native, boot, recovery and console checks.

### Interactive session with a continuing disk

From the configured host:

```sh
bash scripts/vm-run.sh
```

This builds the current tree in `atom-os-dev` and opens the OS serial console.
Type commands normally, including `>` for redirection. The default data disk is
`~/.local/share/atom-os-kernel/data.img` **inside the VM**, reused across launches.
Use `sync` to save; press **Ctrl-a, then x** to leave QEMU. An existing image is
never truncated, and a second simultaneous session using that image is refused.
The default session starts with an empty 8 MiB image when none exists.

Inside a prepared build environment, the equivalent launcher is:

```sh
python3 scripts/run.py --accel kvm
```

Both launchers accept `--disk /path/to/data.img` and `--accel tcg`. Disk paths
passed to `vm-run.sh` refer to the VM. Changes made after the last successful
`sync` remain in RAM and are lost on exit. The daemon keeps an active shell's
prompt quiet; its standalone heartbeat remains available after the shell exits.

### Buffered IPC and executable preflight

The build now validates every embedded userspace executable with the kernel's
own ELF parser before producing the boot image. `user_rt::receive_into` provides
allocation-free, caller-owned IPC messages; the daemon uses it, and short
buffers are rejected without consuming a queued message.

Run `bash scripts/vm-test-userspace.sh` on the configured host for focused native,
KVM and TCG verification. See [userspace reuse](docs/USERSPACE-REUSE.md) for the API,
source provenance, acceptance cases, and compatibility decisions.

## Shell commands

| Command | Behavior |
|---|---|
| `help`, `ls`, `clear` | List commands/files or clear the screen |
| `cat file` | Read a file |
| `echo text > file` | Append text and a newline |
| `edit file` | Edit up to 64 KiB; Esc saves the RAM copy |
| `rm file` | Remove a writable name; already-open handles retain the file until closed |
| `mv old new` | Rename two whitespace-separated writable names; an existing destination is rejected |
| `df`, `status` | Show file/snapshot/buffer capacity, saved state, generation and disk availability |
| `msg text` | Send a message to the daemon; a full mailbox returns an error |
| `bench`, `heaptest`, `stats` | Exercise yields/heap or show free frames and live tasks |
| `spawn worker.elf [arguments...]` | Start a child with arguments while the shell continues; print its PID |
| `wait PID` | Wait for a child of this shell and collect its exit status |
| `run worker.elf [arguments...]` | Replace the shell with the executable and arguments, retaining its PID |
| `ps` | Snapshot PID, parent PID, state and program name; uncollected exits remain visible |
| `kill PID` | Immediately terminate a live process; its parent can collect status **137** with `wait` |
| `proctest` | Exercise argument limits, bad pointers, exec, process inspection, kill/wait and cleanup |
| `selftest`, `pairtest` | Run one worker or two concurrent workers and check their exits |
| `churn 48` | Exercise repeated process creation/reaping and compare free-frame counts |
| `fstest` | Exercise file lifecycle and full-capacity checks on an empty filesystem with a disk |
| `faulttest` | Confirm a user page fault terminates that worker while the shell survives |
| `sync` | Commit writable files to the data disk |
| `reboot` | Sync successfully, then reboot the OS |

Program launches accept single/double quotes, empty quoted arguments, and
backslash escaping outside single quotes. For example:

```text
spawn worker.elf --args "two words" ''
spawn worker.elf --sleep
ps
kill 4
wait 4
```

Use the actual PID printed by `spawn` in place of `4`. `worker.elf --args` prints
its arguments and exits with status 41; `--sleep` is a long-lived diagnostic.
`run` replaces the shell, so use `spawn` to keep the prompt. Invalid quoting or
arguments are rejected. The ABI accepts at most 16 UTF-8 arguments including
the executable name (`argv[0]`), and 1024 bytes including NUL separators.
The shell's existing line editor accepts ASCII input. Userspace obtains owned
argument strings through `user_rt::args()`; arguments are copied into the kernel
before launch, and rejected exec requests leave the old image and arguments intact.

`ps` uses one validated, fixed-capacity snapshot; states are ready, running,
sleeping, waiting, exited, or trapped. `kill` is immediate termination, not a
catchable signal. It shares exit/wait cleanup, including orphan handling and
deferred reclamation of an active kernel stack/address space. There are no user
identities or permissions yet: **any process can kill any live user process,
including the shell or daemon**. PID 0, unknown PIDs and already-exited PIDs are
rejected. Process inspection and control are not security authorization boundaries.

The serial console accepts normal terminal input. The optional VGA/PS/2 path
uses the [basic keyboard mapping](kernel-kit/src/keyboard.rs), where numpad `+`
enters `>` for redirection.

The kernel embeds five read-only programs: `shell.elf`, `daemon.elf`,
`worker.elf`, `fault.elf`, and `fs-probe.elf`. The diagnostic worker checks heap contents,
untrusted pointers, file handles, fast syscalls, SIMD context and IPC, then exits
with status **37**. The fault probe touches a stack guard page and produces
status **142**, which its parent checks. `fs-probe.elf` exercises deletion with
open handles, name reuse, protected files, count/byte limits and saved-state
tracking. The `storageprobe` diagnostic command supports `seed`, `mutate` and
`verify` with a 16-hex-digit nonce for isolated recovery-test disks; `seed`
requires an empty writable filesystem and fills one complete snapshot.

The shared syscall definitions are in [`abi.rs`](abi.rs). Existing byte-I/O and
IPC calls remain available alongside the newer process and memory operations.
`SYS_ALLOC` returns mapped process-private memory; `SYS_FREE` releases it.

## Architecture

| Location | Responsibility |
|---|---|
| [`x86_64-kernel/`](x86_64-kernel) | Boot entry, CPU/interrupt setup, context activation, program embedding and crash diagnostics |
| [`kernel-kit/`](kernel-kit) | Root atoms, allocation, address spaces, ELF validation, filesystem, hardware I/O and storage |
| [`kernel-orchestrator/`](kernel-orchestrator) | Scheduling, process loading/lifecycle and syscall dispatch |
| [`user-rt/`](user-rt) | Userspace entry, syscall wrappers, console helpers and allocator |
| [`payload/`](payload) | Command shell |
| [`daemon/`](daemon) | IPC receiver and heartbeat process |
| [`worker/`](worker) | Diagnostic worker and fault-probe executables |
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

### Atom architecture: intent and current enforcement

The eight root atom definitions remain unchanged by the process-control work.
The intended architecture uses immutable atoms and synthetic emergence through
composition. The current implementation does **not** establish that all behavior
is confined to such compositions: several atom helpers take caller-supplied Rust
callbacks, and the kernel implements scheduling, syscalls, device access and
loading through direct Rust/assembly control flow. These callbacks are supplied
by compiled code; this is not evidence of remote callback injection.

The ELF loader validates executable structure, mappings and permissions, but
it does not verify an atom-only composition language. Existing VM tests establish
specific runtime behaviors, not atom immutability or a complete security proof.
This distinction must be resolved before networking can rely on the proposed
immutable-composition security model.

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
| Program arguments | 16 including executable name; 1024 serialized bytes |
| User stack | 32 KiB per process, with a guard page |
| Heap address window | 16 MiB per process |
| Individual user allocation | Up to 1 MiB |
| Physical frame pool | Capped at 64 MiB |
| Open file descriptors | 16 per process |
| IPC queue | 4 messages per recipient; up to 255 message bytes |
| Writable names in RAM and on disk | 128 |
| Embedded read-only programs | 5 |
| Live writable-file buffer capacity | 1 MiB, including unlinked-but-open files |
| Filename | 63 bytes; flat names |
| File contents | Up to 64 KiB per writable file |
| Serialized snapshot | 512 KiB total, including filenames and metadata |

**RAM edits are durable only after `sync` succeeds.** Creation, append, replacement
and rename enforce the serialized snapshot budget before mutation. Rejected
writes preserve existing contents; the editor uses atomic replacement rather
than truncating before a save. `cat` opens existing files without creating them.

Unlinking releases a file's name and snapshot capacity immediately. Open handles
still reference its original data; recreating the same name makes a separate
file. Its memory is released on the last close. Live buffer capacity is bounded,
and truncation returns unused buffer capacity. Built-in files cannot be removed,
renamed or written.

`status` reports `saved` or `unsaved` for namespace changes, the last committed
generation and disk availability. A failed sync keeps the RAM state unsaved.
Completed virtio I/O errors can be retried; timed-out or invalid queue operations
leave the device offline. Without a compatible disk, the kernel reports
`STORAGE_UNAVAILABLE` and continues with RAM files. The disk format remains
`ATOMFS01`; existing valid snapshots are compatible.

## Next milestones

These are proposed work, not currently implemented features:

- Define and enforce the immutable-atom composition boundary before relying on
  it for the proposed network security architecture.
- Further storage development beyond the current flat, bounded snapshot format.

## CI and preserved history

The [Test OS workflow](.github/workflows/test.yml) builds all bundled programs
and the boot image, runs native regressions, TCG acceptance, interrupted-storage
recovery and interactive-console checks, and retains failure artifacts. Check [GitHub Actions](https://github.com/Rekonquest/atom-os-kernel/actions)
for the status of a particular commit.

Selected reports and raw logs are committed under [`test-results/`](test-results),
including the [original failed baseline](test-results/20260905T202917Z/REPORT.md).
Generated images, test data disks, caches and large trace archives remain local.
[BACKUP.md](BACKUP.md) explains the backup scope and the inherited, unused
`atom-3d-engine` Git link.
