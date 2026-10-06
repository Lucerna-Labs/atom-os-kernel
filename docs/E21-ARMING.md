# E21: can the shadow web arm itself at boot?

Status: MEASURED (2026-10-06); P4 FAILED, see below

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

Three KVM boots of one image (commit after ea20302), 512 MiB, the driver and
raw logs kept outside the repo. The demo fleet ran first in every boot, then
the web was reset and retrained on `bench`, armed, and the shell sequence run
with `sense` after each command. Budgets are the foreign budget at the moment
`sense` ran (they decay 1.3 per second); episodes are the judge's count of
condemnations.

**P1 MEASURED (held).** Keying 0. After `bench` the map admits 6 of 128 sites
and the shell already carries 1.55 of its 2.0 budget. Armed, the shell is
condemned on every command (episodes 2, 4, 6, 8, 11, 13, 17, 19 through
`spawn`), finishing each only through the thermodynamic release. The spawned
worker reaches 9.45 and never finishes: `wait` timed out at 120 s.

**P2 MEASURED (held).** Keying 1. Same picture to the digit: map 6 sites,
shell 1.55 after `bench`, episodes 2, 4, 6, 8, 11, 14, 17, 19, the worker
at 7.47 and `wait` timed out. The target fix changes nothing while the map is
the benchmark's.

**P3 MEASURED (held).** Under both keyings every fleet marker (E22 to E38)
printed and the spider saw the rogue's budget cross the bar. Keying 1 does not
weaken the rogue's condemnation.

**P4 FAILED.** Keying 1, horizon open through `bench` plus the whole shell
sequence (280,000 events, 200,000 of them `churn`), then frozen and armed: the
map admits 47 sites. The second pass ran to completion, but the shell was
condemned 19 times during the first eight commands (budget 0.6, 1.1, 1.5, 1.9,
then 2.2 to 2.5 with episodes 5, 8, 12, 15, 16, 19 through `run hello.elf`)
and was clean for the last five (`churn`, `proctest`, `msg`, `selftest`,
`fstest`: budget 0.2 to 0.6, no new episodes). The worker reached 2.56 and one
episode but finished.

Why: the map is not a union over the session. Erosion relaxes every site by
0.18 % per 16 events, so a site's depth is its recent equilibrium; after the
200,000 `churn` events, everything trained before `churn` had relaxed to the
floor, and only the conversations of the last ten to twenty thousand events
were readable at the freeze. That is the law working as written: the map
admits *sustained* conversations (about 900 events to reach readable depth),
so a conversation a shell uses a few times per command can never be learned
by it, whatever the window. The budget then charges every low-rate legitimate
conversation at 0.02 per event, and anything issuing more than about 100
foreign syscalls faster than the decay (65 per second) is condemned.

## What this means for arming at boot

- Both input gaps are real and are now fixed or selectable at the edge, but
  neither is the blocker. The blocker is that the equilibrium law cannot
  represent low-rate vocabulary, and the shell, file and process calls are
  low-rate vocabulary.
- The partner keying (keying 1) is the right input and costs nothing, but it
  measured equal, so it stays off by default until something depends on it.
- Next step, not built here: a second admission law beside the equilibrium
  cone (a co-engine), for vocabulary rather than rate: a conversation
  admitted because a trusted session used it at all, carried into the boot
  image the way `network-lightcone` carries its world. The equilibrium cone
  keeps its job (sustained foreign conversation, the rogue), and the two are
  measured alone and together, with chaos tried on the admission law first.
  `sense` and `SYS_SENSE` 4/14/16 give the measurements that step needs.
