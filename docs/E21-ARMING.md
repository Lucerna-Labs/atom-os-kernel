# E21: can the shadow web arm itself at boot?

Status: PRECOMMITTED (2026-10-06)

## Problem

The shadow web (`kernel-sense`) trains its normality cone automatically after
4096 syscall events, about one second into boot, but it never arms itself: a
cone armed at that point condemned nearly every later program (desktop merge,
2026-10-03). Since then the shell's `demos` command arms it for the demo fleet
and disarms it after. Keying the conversation sites on the program's identity
(commit 0139d12) fixed one cause, a re-run program landing on fresh sites, but
not the arming.

Two gaps remain in what the sensor is handed, before any change to the sensor
itself is justified:

1. **The target.** The dispatcher passes the raw first syscall argument as the
   conversation target. For `SYS_IPC_SEND`, `SYS_KILL` and `SYS_WAIT` that is a
   partner pid, and for the multiplexed calls (`SYS_SENSE`, `SYS_KEY`, ...) it is
   a sub-function. For everything else it is a pointer, a byte or a length, so
   the same conversation lands on a different site every call. With the site
   also keyed on the program's identity, a program first launched after the
   freeze has no learned sites at all.
2. **The learning window.** The 4096 events close inside the shell's startup
   benchmark (10,000 `SYS_YIELD`s), so the map holds the shell's startup and the
   daemon's idle loop. File, process and pipe conversations are never learned.

## Composition (no sensor behaviour changes)

- `kernel-orchestrator/src/conversation.rs`: a pure table from syscall number
  and first argument to the conversation's *partner*: the pid for pid-addressed
  calls, the sub-function for multiplexed calls, 0 otherwise. Policy at the
  edge; the sensor stays inert.
- The dispatcher keys the site either as today (program identity, raw
  argument) or on the conversation (one shared identity, partner), selected at
  run time through `SYS_SENSE` sub 14 so the two keyings can be measured in the
  same boot image. Blame stays on the pid either way.
- `kernel-sense` gains two read-only accessors (the normal map as two 64-bit
  words; the learning horizon) and a `set_horizon` knob so the learning window
  can be chosen at the edge instead of being fixed at 4096. Default unchanged.
- Shell `sense` command: status, armed, keying, the map, and every live
  process's foreign budget and condemnation episodes.

## Experiment

One boot image, KVM, the same command sequence under each keying. Mirror the
real boot: reset the web, run `bench` (the 10,000 yields the boot trains on),
arm, then run shell work (files, bulk I/O, spawn/wait, churn, proctest, a
userspace program), reading `sense` after each command. Then the demo fleet
under the same keying.

## Predictions

- **P1** (keying 0, today's): the shell exceeds the quarantine budget (2.0)
  during bulk file work (`fill`, `cp`) and process churn; armed, it is starved
  at least once (episodes > 0).
- **P2** (keying 1, conversation): still condemned during the same commands,
  because file and process calls are outside the boot-learned map. The target
  fix is necessary but not sufficient; the learning window is the second gap.
- **P3** (both keyings): the demo fleet's rogue is still condemned (its
  partners 90..121 are foreign under either keying) and every E-PASS marker
  still prints.
- **P4** (keying 1, horizon widened to cover the whole shell sequence, then
  armed): a second pass of the same commands runs with no condemnation, and
  the rogue is still condemned. If P4 holds, the next step is a trained map
  carried into the boot image, the way `network-lightcone` carries its world.

Results are recorded below as MEASURED / FAILED / UNRUN; failures are kept.

## Results

UNRUN.
