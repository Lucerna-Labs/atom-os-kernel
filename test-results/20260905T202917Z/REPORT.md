**Atom OS kernel baseline — September 5, 2026**

The current kernel builds, but it does not yet pass a usable OS boot test. KVM stops on a confirmed startup register error. Software-emulated QEMU runs the daemon but never produces the shell's startup benchmark or responds to `ls`. Native tests against the unchanged source modules pass 9 checks and fail 5.

The tested project is `/home/jesse/Desktop/Projects/atom-os-kernel`, Git HEAD `3fe46c838af28c4e35a1045360e2417b7e10f6bf`. A fresh copy of all 51 tracked or initially untracked files was transferred into the existing `atom-os-dev` VM. SHA-256 checks passed before and after building; the original source files remain unchanged. The pre-existing tracked differences are line-ending differences. No alternate kernel, downloaded repository, old boot image, or patched kernel was substituted.

The VM ran Ubuntu 24.04.4 with Rust `1.100.0-nightly (8fa1c96cf 2026-08-17)`, bootimage 0.10.5, and QEMU 8.2.2. Builds used the installed toolchain and cached dependencies with `--locked --offline`. Each OS boot used one virtual CPU, 128 MiB RAM, no network device, and a snapshot of the newly built image. KVM acceleration was confirmed through QMP as `enabled: true`.

| Check | Result | Evidence |
|---|---|---|
| Release payload build | Pass | [Build log](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/build-payload.log) |
| Release daemon build | Pass | [Build log](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/build-daemon.log) |
| Kernel and boot-image build | Pass, with warnings | [Build log](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/build-kernel.log) |
| Basic kernel heap allocation | Pass under KVM and TCG | Both boot logs report `Heap Allocation Test: PASS!` |
| Small-allocation churn | Pass for 100,000 rounds | Slab reached the test cap; bump exhausted after 4,096 allocations. This is a bounded test, despite the kernel log saying “indefinitely.” |
| KVM boot to shell | Fail | [Serial log](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/boot-kvm-run2/serial.log): `CPU EXCEPTION 0D (#GP)` before user processes launch |
| TCG boot to shell | Fail | [Serial log](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/boot-tcg/serial.log): daemon heartbeats, no completed 10,000-yield benchmark or prompt |
| Keyboard → `ls` → directory output | Fail under both backends | QMP successfully injected the keys; the expected file listing never appeared |
| End-to-end file write/read and IPC delivery | Blocked | The interactive shell was unavailable; these checks were not counted as passing |
| Native module diagnostics | 9 pass / 5 fail | [Full test output](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/core-tests.log) |

The package build summaries reported 104 compiler warnings in total. A successful compilation does not establish that the warning sites are safe.

**Fix priorities**

1. **P0 — Correct startup MSR configuration.** The saved KVM exception frame records `RIP=0x20a34f`, `RCX=0xc0000080`, `RDX=0`, `RAX=0x1d00`, and error code 0. The disassembly at that RIP is `wrmsr`, immediately after `or $0x1000,%eax`. This maps directly to [setup_syscall_msr](/home/jesse/Desktop/Projects/atom-os-kernel/x86_64-kernel/src/main.rs:313). The code sets EFER bit 12; Intel reserves that bit, while syscall enable is bit 0. AMD defines bit 12 as SVM enable. The crash is therefore located at the incorrect register write, rather than merely inferred from the last serial message. See the [decoded frame](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/kvm-fault-decoded.json), [raw saved stack](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/results/boot-kvm-fault-frame2/exception-stack.txt), [Intel EFER definition](https://cdrdv2-public.intel.com/874248/325384-090-sdm-vol-3abcd.pdf), and [AMD EFER definition](https://docs.amd.com/api/khub/documents/sD1_QL~h4Afq2_tvzxqqSQ/content).

2. **P0 — Repair syscall/context-switch bookkeeping and prove shell/daemon coexistence.** [The int-0x80 handler](/home/jesse/Desktop/Projects/atom-os-kernel/x86_64-kernel/src/main.rs:399) obtains the current frame but dispatches through the task's previously saved `rsp`. On yield/exit it calls `switch_context` and discards the returned stack pointer. `new_rsp` remains the incoming pointer, so the CR3/TSS update condition never becomes true on this path. TCG's daemon-only behavior is measured; the effect of a proposed correction is not yet tested. The ordinary round-robin scheduler module test passes, which narrows attention toward the interrupt/syscall integration rather than proving that whole path works. The wrappers also need an explicit contract for preserving SIMD state and entering Rust with the correct stack alignment. Rust requires registers not declared as outputs to be preserved across inline assembly. [Rust inline-assembly rules](https://doc.rust-lang.org/reference/inline-assembly.html#rules-for-inline-assembly)

3. **P1 — Honor allocation alignment.** `slab_honors_requested_alignment` failed for a 32-byte alignment request: the returned pointer was only 16-byte aligned. [Slab allocation](/home/jesse/Desktop/Projects/atom-os-kernel/kernel-kit/src/slab.rs:249) always carves nodes at alignment 16 and selects its reuse bucket by size. This violates the requested `Layout` and can break code that relies on stronger alignment. The 100,000-round reuse test and disjoint live-allocation test pass, so retain the working slab mechanism while fixing routing/alignment. Also review fallback deallocation: it currently attempts to read a slab header even for headerless bump allocations; that additional concern is from source inspection, not a completed runtime probe.

4. **P1 — Make open file handles stable.** `ramfs_file_handle_survives_directory_growth` failed on the first added file. [RamFS](/home/jesse/Desktop/Projects/atom-os-kernel/kernel-kit/src/fs.rs:19) returns a pointer to a `Vec<u8>` stored inside the directory's growable vector; adding children can relocate that object. Existing FDs retain the old address. The native test demonstrates relocation without dereferencing the stale pointer. Use stable ownership or identifiers, then verify an FD held across many file creations still reads/writes the same file in the running OS.

5. **P1 — Validate executable images and syscall pointers before dereferencing them.** Two tests show [the ELF gate](/home/jesse/Desktop/Projects/atom-os-kernel/kernel-kit/src/elf.rs:34) accepts AArch64 headers and big-endian headers. It checks only magic and the 64-bit class. [SYS_EXEC](/home/jesse/Desktop/Projects/atom-os-kernel/kernel-orchestrator/src/syscall.rs:205) additionally needs bounds/overflow checks for program headers and segment spans, plus appropriate mapping permissions. Pointer-taking syscalls currently dereference caller addresses directly; malformed-pointer rejection was not exercised in a running user process because boot is blocked.

6. **P1 before runtime process creation — Reclaim terminated tasks.** `scheduler_reuses_terminated_slots` failed after filling the 16 slots and terminating a task. [spawn](/home/jesse/Desktop/Projects/atom-os-kernel/kernel-orchestrator/src/scheduler.rs:29) accepts only `None` slots and there is no reaping path. Implement resource accounting and reclamation together: slot, address space, user/kernel stack, and open handles. Then measure repeated process creation/exit for at least several multiples of the task limit, with memory returning to its baseline.

The other native checks passed: frame exhaustion/reuse, contiguous allocation under fragmentation, rejection of invalid frame-free addresses, basic keyboard decoding, ELF magic rejection, scheduler capacity enforcement, round-robin saved-stack behavior, disjoint live slab allocations, and repeated slab reuse. These execute the real source modules in a native Linux test binary inside the VM. They do not exercise privileged IRQ operations, the MMU, the syscall assembly, or hardware I/O, and do not replace the failed OS boot tests.

**What to add next**

The immediate milestone should be a reliable KVM boot with a usable shell and a live daemon. Include a repeatable VM regression command and allocation-free serial crash reports containing RIP, RSP, error code, and CR2. The existing exception handler receives the frame but discards it, which required recovering this crash through QMP.

After that baseline is reliable, the most useful feature increment is **runtime program launching with `spawn`, `wait`, and exit/reaping**, backed by a real per-process heap. The current payload/daemon allocators return null, and `SYS_ALLOC` uses a newly constructed legacy `MemoryPool` whose values are block indices, not mapped user allocations. Add a small workload that starts a program, exchanges a message, reads/writes a file, exits, and repeats without consuming another permanent task slot or leaking frames.

The following milestone should be **persistent storage through a VM block device**, with a reboot test that verifies exact file contents. The current filesystem is an in-memory, flat RamFS. Stable file identities and validated buffers are prerequisites for making persistent storage dependable.

A useful acceptance sequence for these milestones is:

- KVM reaches the shell prompt and completes the numeric 10,000-yield benchmark while the daemon continues running.
- `ls`, file write/read, and exact IPC delivery pass through injected keyboard input; failures and CPU exceptions fail the automated run.
- Allocation alignment, stable-FD, and malformed-ELF tests all pass.
- Repeated spawn/exit/reap cycles exceed 16 creations and return resource counts to their initial values.
- Persistent-file contents survive an OS reboot using an isolated test disk.

No performance comparison with Linux or bare-metal hardware was performed. The current tests do not establish scheduler fairness, SMP behavior, long-term memory stability, or working fast `syscall/sysret`. The latter path also overwrites the incoming syscall number, loses the user stack pointer, and does not construct the hardware-frame tail expected by the dispatcher; it needs its own validation before enabling it for applications.

**Reproduction and retained evidence**

The VM run directory is `/home/jesse/atom-os-kernel-tests/20260905T202917Z`. The original kernel image SHA-256 is `2deec49605eb14431d790c1c98c2f35827585f32a0e7e6aa9afb30571c4a4769`. All application boot runs used that same image.

Build commands were run from the corresponding directories in the copied `source` tree:

```sh
# In payload/, then daemon/:
cargo +nightly build -Zjson-target-spec --release --locked --offline
# In x86_64-kernel/:
cargo +nightly bootimage -Zjson-target-spec \
  -Zbuild-std=core,alloc,compiler_builtins \
  -Zbuild-std-features=compiler-builtins-mem \
  --target ../x86_64-os.json --release --locked --offline
```

[boot_probe.py](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/boot_probe.py) records each full QEMU command, serial transcript, QMP acceleration state, CPU samples, and test result. Reusing it requires a new `--label` so earlier evidence is not overwritten. [core_probe.rs](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/core_probe.rs) imports the untouched modules by path and was compiled in the VM run directory with `rustc +nightly --edition=2021 --test core_probe.rs -o core-probe`, then run with `./core-probe --test-threads=1 --nocapture`.

Historical attempts from this run are retained: the first KVM launch failed because `sudo -u jesse -g kvm` required a password, then the launch was corrected to use the existing passwordless root privilege solely to drop into user `jesse` with the `kvm` group. No account membership was changed. The first stack-inspection attempt used an unsupported monitor register expression; a subsequent run used the numeric RSP from QMP and recovered the fault frame. The TCG `-svm` diagnostic still launched the daemon with EFER `0x0d00`; its behavior did not reproduce Intel KVM's rejection of the write. The first TCG interrupt trace was retained losslessly as gzip; subsequent trace capture was bounded.

[Source-integrity result](/home/jesse/Desktop/Projects/atom-os-kernel/test-results/20260905T202917Z/source-integrity-after.json): all 51 original files are unchanged. Only this test-results directory was added to the host project. The VM remains running for further work; the nested OS test instances were shut down after capture.
