# Parked: the field-substrate bridge

Parked on 2026-10-03, when the Lucerna-Labs line was merged with the
durable-file and desktop lines. Kernel and network work is scheduled last, so
this waits for that phase. Git history keeps it too: the Lucerna-Labs commits
`b8540ed` (bridge plus the GAP-5 change) and `fc18671` are merge parents.

## What it is

The bridge connects the kernel to the field substrate, the
`universe-substrate` simulation, through two crates: `field-core` and
`kernel-glue`. It consists of:

- `kernel-kit/src/scheduler_glue.rs`, which owns the global `FieldState`. The
  file is kept in the tree but is not compiled: it is not listed in
  `kernel-kit/src/lib.rs`.
- [`field-substrate-bridge.patch`](field-substrate-bridge.patch), Lucerna's
  changes against the July 17 base (`3fe46c8`):
  - path dependencies on `../ATOM OS/substrate/{field-core,kernel-glue}`;
  - field syscalls 17–20 (`SYS_FIELD_STIMULATE`, `SYS_FIELD_EVOLVE`,
    `SYS_FIELD_OBSERVE`, `SYS_FIELD_MEASUREMENTS`). These numbers are still
    free in `abi.rs`;
  - a timer hook that evolves the field every 10 ticks;
  - `SYS_IPC_SEND` and `SYS_IPC_RECV` rewired so that a message becomes
    injected energy rather than delivered bytes;
  - the GAP-5 `saved_state` copy of the trap frame in the scheduler.

## Why it is not wired in

- The bridge needs `field-core` and `kernel-glue`. They are not in this
  repository, and their GitHub repository (Rekonquest/atom-os-field-substrate)
  no longer exists. The only copy found is the NAS archive
  `Inactive Projects/2026-09-27/Desktop/Projects/atom-os-field-substrate-main/
  atom-os-field-substrate-main/substrate/`. To use the crates here, copy them
  into `third_party/`, as was done for the rendering engine.
- Upstream, the kernel failed to compile with the bridge applied
  (`_build_err.txt`, built on Windows).
- The patch targets the July scheduler and syscall code, which both later
  lines replaced. The GAP-5 copy is not needed with the current scheduler,
  which restores every task from its full trap frame on its own kernel stack.
- The IPC rewiring would end byte delivery for the daemon, the IPC acceptance
  tests and the desktop's message users. Any wiring should keep byte IPC and
  add the energy path beside it.
