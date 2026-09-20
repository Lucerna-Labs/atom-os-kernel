# Userspace utilities adopted from atom-os-system

Source reviewed: `Rekonquest/atom-os-system`, commit
`cd197fec8a2467f33bd3eacf28989e6ba2a2797e` (GitHub main at review time).
The current kernel's runtime, allocator, process loader, shell and storage
remain the implementation used by these additions.

## Buffered IPC

`user_rt::receive_into(&mut buffer)` adapts the source project's
`atom_rt::sys::ipc_recv_into` API to this kernel's current IPC contract.
The daemon now uses this API instead of borrowing the receive page directly.
The existing `receive() -> Option<String>` convenience API uses it internally.

```rust
let mut buffer = [0u8; user_rt::ipc::MAX_MESSAGE_BYTES];
match user_rt::receive_into(&mut buffer) {
    Ok(Some(length)) => { /* complete payload in buffer[..length] */ }
    Ok(None) => { /* mailbox empty */ }
    Err(error) => { /* explicit failure */ }
}
```

- The caller must provide at least 255 bytes. Capacity is checked **before**
  receiving, so rejection neither dequeues nor truncates a pending message.
- `Ok(Some(0))` distinguishes an empty message from an empty mailbox.
- The NUL terminator is excluded. Bytes after the payload are left untouched.
- The bounded copy reads at most the ABI's 256-byte message/terminator area.
  Missing termination is an explicit error, not a read beyond that area.
- The payload belongs to the caller and survives later receives into the
  kernel's reusable page. The API allocates no heap memory.
- The mailbox remains the existing four-message FIFO with rejection when full;
  no new syscall or queue implementation is introduced.

`spawn worker.elf --ipc-test`, followed by `wait PID`, exercises short-buffer
queue preservation, empty and maximum-length UTF-8 messages, ownership across
receive-page reuse, FIFO/backpressure, convenience-API compatibility and 32
receive cycles with stable free-frame counts on the real kernel.

## Executable preflight

The source project's `scripts/check-elf.ps1` inspired a Linux-native gate:
`bash scripts/check-elf.sh`. It compiles a small host executable that imports
**the actual `kernel-kit/src/elf.rs` parser**, avoiding a second loader policy.
With no arguments, it discovers all executable `include_bytes!` paths from
`x86_64-kernel/src/main.rs` and validates every one. Explicit ELF paths can also
be supplied. Missing files, empty invocation/discovery, and rejected executables
produce a nonzero status. A mixed batch fails if even one member is rejected.

`scripts/build.sh` invokes the gate after building userspace and before building
the boot image. This checks headers, architecture, segments, entry point,
address ranges, and page permissions under the loader's existing rules. It
cannot certify instruction behavior or replace VM acceptance.

## Verification

```sh
bash scripts/build.sh
bash scripts/test-userspace-host.sh
python3 scripts/test-userspace-imports.py --accel tcg --output test-results/imports-tcg
# Existing configured host and VM: both KVM and TCG, one image, fresh disks.
bash scripts/vm-test-userspace.sh
```

The focused VM suite creates its own v2 filesystem image containing `boot.done`,
using the shell's existing interactive mode so the unrelated attack-demo fleet
does not consume task slots. It exercises production runtime/kernel code,
buffered daemon delivery, program execution, and cold-boot directory/file
persistence. It never opens the continuing interactive data image.

The native test script now links the scheduler's actual current security and
network dependencies, restoring that existing suite's ability to compile.
The focused host suite adds four IPC copy-boundary tests and ten executable
preflight acceptance/rejection cases. CI runs these checks and the focused TCG
suite alongside the existing suites; adding the workflow is not a claim that a
remote GitHub Actions run has passed.

## Deliberate selection

The old bump allocator, entrypoint macro, syscall assumptions and kernel
patches target an earlier OS and were not substituted for this kernel's newer
implementations. The numeric formatter duplicates existing formatting support;
the older tokenizer lacks the current launch quoting behavior. `fieldmon` uses
unimplemented field syscalls 17–20 and needs a separately designed integration,
so the current sensor was not substituted for its original substrate semantics.
