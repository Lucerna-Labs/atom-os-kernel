# Atom OS: durable file management and interactive sessions

Implemented and verified September 17, 2026 in the existing `atom-os-dev` VM,
from the kernel project at `/home/jesse/Desktop/Projects/atom-os-kernel`.
Work is on `codex/durable-file-session`, starting from the backed-up README
checkpoint `9ad51dd`.

## Implemented behavior

- `rm` unlinks a writable filename without invalidating already-open handles.
  Those handles retain the original file until closed; recreating the same name
  creates a different file. `mv` preserves the file object and refuses an
  existing destination. Embedded programs remain read-only.
- Creation, append, replacement and rename check the persistent file count and
  serialized byte budget before mutation. File writes and editor replacement
  either succeed completely or preserve previous contents. Reading a missing
  file no longer creates it.
- `df` and `status` show file count, snapshot usage, live buffer capacity,
  saved/unsaved state, generation and disk availability. Failed operations do
  not advance the saved revision. A successful sync acknowledges only its
  captured revision.
- File descriptors now own reference-counted file objects. Buffer capacity is
  bounded even for unlinked open files, and truncation releases excess capacity.
- Completed virtio requests reporting transient I/O failure leave their queue
  usable for retry. Timeouts or invalid completion IDs still take it offline.
- `bash scripts/vm-run.sh` builds in the existing VM and attaches an interactive
  serial console. Its persistent disk is
  `/home/jesse/.local/share/atom-os-kernel/data.img` inside the VM. Existing images
  are never truncated, and simultaneous launches against the same disk are
  refused. `sync` saves; Ctrl-a followed by x exits QEMU.
- Serial input accepts ordinary terminal characters, including `>`; periodic
  daemon messages no longer flood an active shell's prompt. The host launcher
  was exercised directly through SSH: the OS booted and accepted `help` and
  `status` against its default continuing disk.

## Final verification

The same boot image was tested under KVM and TCG:

`062e9bcbd6013768567495670609defe393e95199a5fb4ee4f385d10dd09e4f3`

| Suite | KVM | TCG |
|---|---:|---:|
| OS acceptance checks | 18 passed | 18 passed |
| Interrupted-storage / recoverable-I/O cases | 7 passed | 7 passed |
| Interactive sessions on a retained disk | 2 passed | 2 passed |
| Process churn | 48 cycles | 48 cycles |
| Free frames before / after churn | 16,324 / 16,324 | 16,324 / 16,324 |

The native suite passed **29 tests with no failures**. New cases include open
handle survival, recreation without aliasing, protected names, full count/byte
budgets, failure atomicity, detached-buffer limits, truncation reclamation,
read-only opening, and saved-revision tracking.

The console tests typed commands through a real PTY/serial connection, saved a
unique file, quit QEMU, and started the launcher again with the same image. The
file contents and image checksum survived; a second concurrent launcher was
rejected while the first held the session lock.

## Interrupted-save evidence

The recovery workload uses eight independently checked binary files whose
serialized snapshot fills all **524,288 bytes**. QEMU's `blkdebug` backend
suspends an actual virtio request at each selected event. The harness confirms
the suspended tag and completed-operation counters before sending SIGKILL to
that owned OS test process. It then cold-boots the same disk.

| Interruption | Completed writes / flushes at stop | Recovered snapshot, both backends |
|---|---:|---|
| First payload write | 0 / 0 | Previous complete generation |
| Payload flush | 1,024 / 0 | Previous complete generation |
| Header write | 1,024 / 1 | Previous complete generation |
| Header flush | 1,025 / 1 | New complete generation |

An independent host decoder checked the complete file map, and the rebooted OS
read back and verified every byte. No mixed generation was accepted. Additional
one-shot write EIO, flush EIO, and host ENOSPC injections preserved the previous
acknowledged snapshot, kept RAM marked unsaved, left the shell responsive, and
allowed a successful retry followed by verified cold boot.

These are actual guest interruptions through QEMU's block layer, not the native
mock-disk fixture. They do **not** simulate loss of the physical host's disk
cache or certify behavior under electrical power failure. The mechanism is
documented in [QEMU's blkdebug guide](https://www.qemu.org/docs/master/devel/testing/blkdebug.html).

An initial header-phase run timed out while its monitor waited for a breakpoint
that had not yet been observed. The harness was corrected to first wait for
QEMU's suspension notification, then confirm the tag. The failed run and its
partial results are retained under this directory's `results/stage02-recovery`;
the corrected run used the same candidate image and passed all seven cases.

## Evidence and reproduction

Final artifacts are in
[the verified run](../run-20260917T202346184712688Z/):

- [Combined verification](../run-20260917T202346184712688Z/verification.json)
- [Native output](../run-20260917T202346184712688Z/native.log)
- [KVM acceptance](../run-20260917T202346184712688Z/acceptance/result.json)
- [KVM recovery](../run-20260917T202346184712688Z/recovery/result.json)
- [TCG acceptance](../run-20260917T202346184712688Z/tcg-acceptance/result.json)
- [TCG recovery](../run-20260917T202346184712688Z/tcg-recovery/result.json)
- [KVM console](../run-20260917T202346184712688Z/console/result.json)
- [TCG console](../run-20260917T202346184712688Z/tcg-console/result.json)

`scripts/vm-test.sh` now runs the native, OS acceptance, recovery and console
suites and retrieves their artifacts. The GitHub workflow runs their TCG
counterparts; no GitHub Actions success is claimed by this report. Source
snapshots, image hashes, serial output, QMP messages, suspended-request counters,
and failure history are retained. README and backup documentation were refreshed
after testing; executable sources and test scripts match the tested snapshot.

The original compiler warnings remain outside this milestone. The OS still
uses one CPU, flat filenames, 128 writable names, 64 KiB per file, a 512 KiB
serialized snapshot and a 1 MiB live file-buffer budget. `mv` takes two
whitespace-separated names. Saving requires a successful `sync`. Program
arguments and `ps`/`kill` remain future work.
