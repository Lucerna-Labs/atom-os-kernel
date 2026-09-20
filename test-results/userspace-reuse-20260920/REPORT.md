# Reused userspace utilities — September 20, 2026

Implemented in atom-os-kernel on top of `e812b97`:

- Added a caller-owned buffered IPC receive API, inspired by
  Rekonquest/atom-os-system at `cd197fec8a2467f33bd3eacf28989e6ba2a2797e`.
  Unlike the older helper, it checks capacity before receiving and checks
  termination within the ABI bound. Short buffers never consume a message.
- Wired the daemon and existing String convenience API through it.
- Added a Linux host preflight that imports the kernel's actual ELF parser;
  the build validates every embedded userspace executable before bootimage.
- Restored the existing native suite's dependency wiring for the newer scheduler.
- Added focused host, KVM, TCG and CI acceptance, with exact received contents.

## Verified

The existing `atom-os-dev` VM built the production image. All 24 embedded
executables passed loader preflight. 32 existing native tests, 4 IPC helper
tests and 10 preflight CLI cases passed. Each of KVM and TCG passed 8 focused
acceptance checks using the same binary. The IPC probe tested empty/255-byte
UTF-8 payloads, short-buffer queue preservation, retained caller copies, FIFO,
full-mailbox rejection, and compatibility with the existing String API.
Thirty-two buffered send/receive cycles left free frames unchanged at 16,281.
The daemon received a generated per-run nonce through the new API. Directory
and file contents survived a cold boot, and a preflight-approved program ran
and exited normally. All compiled source inputs match the build's SHA-256
manifest; only README and the reuse documentation were added/refreshed afterward.

Evidence: `../userspace-20260920T142545108025487Z/verification.json`,
`native.log`, `userspace-host.log`, `elf-preflight.json`, and the `kvm/` and
`tcg/` directories. The build log includes each executable's preflight receipt.

## Preserved first-attempt failure

`../userspace-20260920T142419312675278Z/` preserves the first run. Build, host,
native and the first six KVM IPC/daemon gates passed. The next test command
failed because the inherited QMP keyboard helper did not map `/` to `slash`.
That test-input mapping was added; the complete focused suite was rerun on
both backends. No kernel change was needed for that correction.

## Boundary

These are focused runtime/import acceptance results, not a new certification
of the security experiments, networking stack, desktop rendering, or the full
older recovery/console suite. The isolated test image starts with `boot.done`
to use the shell's existing interactive mode without launching the attack-demo
fleet. The continuing VM data disk was never opened. No GitHub Actions result
is claimed merely because workflow checks were added. Older source-project
heap/startup code and field semantics were not substituted into the kernel.
