# Program arguments and process controls — September 17, 2026

Built and tested inside the existing `atom-os-dev` VM. This milestone adds
bounded UTF-8 arguments to spawn/exec, `user_rt::args()`, quoted shell program
launches, a structured process snapshot exposed as `ps`, and immediate `kill`.
No root atom definitions were changed. No networking was implemented.

## Validation

- 32 native regression tests passed.
- 21 acceptance checks passed on each of KVM and TCG, using the same boot image.
- 48 normal spawn/wait/reap cycles: 16,321 free frames before and after.
- 48 forced-termination/reap cycles: 16,283 free frames before and after, while
  children held unlinked files and heap allocations. File-buffer usage recovered.
- Checked exact 16-argument and 1024-byte boundaries, empty/Unicode arguments,
  bad pointers, unmapped page crossings, read-only/short output buffers,
  malformed argument blocks, failed exec preservation, and successful exec
  preserving PID and transferring arguments.
- Checked sleeping, runnable and self termination, zombie inspection, double
  kill/wait rejection, waiting-parent wakeup, and blocked-parent termination
  with orphan cleanup.
- 7 storage interruption/error-recovery cases passed per backend.
- 2 interactive console sessions passed per backend, including quoted/empty/
  escaped arguments, malformed quoting rejection, shell exec, disk persistence
  and exclusive-image locking.

Image SHA-256: `debb366693541bb8df29169f653b14ab43af6d448222183fd3d02abf3af72f90`.

Evidence is in `../run-20260917T212634980705680Z/verification.json` and its
acceptance, tcg-acceptance, recovery, tcg-recovery, console-process and
tcg-console-process directories. The original build-source digest manifest is
preserved. All compiled inputs still match that snapshot. README was updated
after validation; the expanded console test was copied into that build as
`test-console-process.py`, ran against the same image on both backends, and is
archived alongside these results. The original console runs are retained too.
Generated images, archives, test disks and caches remain local.

## ABI and limits

New syscalls 39–43 implement argument-aware spawn, argument-aware exec,
argument retrieval, process snapshots and termination. Existing spawn/exec
syscalls remain compatible and supply the executable name as argv[0]. The
argument API is explicit (`user_rt::args()`), not a C startup-stack ABI.
At most 16 strings and 1024 serialized bytes including the executable name and
NUL separators are accepted. Process snapshots use 16 fixed-capacity records;
all unused records and name bytes are zeroed before userspace copyout.
Kill returns exit status 137 to waiters and is not a catchable signal.

There is no identity/permission system: every process can inspect and kill any
live user process. This includes the shell and daemon. No new authorization
or isolation claim is made for these operations.

## Architecture finding requested before networking

The user describes the intended security architecture as immutable atoms with
behavior arising through synthetic emergence rather than alterable instructions.
The eight existing root atom definitions remain intact. However, the code does
not enforce an exclusively atom-composed execution model: several helpers accept
compiled caller-supplied callbacks, direct Rust/assembly implements kernel
behavior, and the executable loader admits validated x86-64 ELF code without
checking an atom-only composition language. This observation does not establish
that an attacker can replace kernel callbacks remotely.

The earlier design document already identifies one-shot atom helpers rather
than a complete composed-stack architecture. Current paging, input validation,
and fault containment tests establish their specific boundaries, not the full
immutable-composition security premise. The process-control work extends the
existing conventional execution path and does not close that architectural gap.
The network design must first establish the exact atom/composition contracts
and their enforcement boundary; these tests cannot substitute for that proof.
