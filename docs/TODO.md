# Where to pick up

Open work as of 2026-10-06, grouped the way the project is built: everything
else first, kernel security and the network last. Each item says what is known,
where the evidence is, and what the next concrete step would be. Finished items
are removed, not struck through; git history keeps them.

## Kernel security (the kernel phase, in progress)

1. **E21 shadow web: arm at boot.** Measured 2026-10-06 in
   [E21-ARMING.md](E21-ARMING.md). The equilibrium cone keeps only sustained
   conversations readable (erosion relaxes everything else), so the shell's
   low-rate file and process vocabulary is foreign at any learning window, and
   an armed cone condemns the shell on every command. Next: a second admission
   law beside the cone, for vocabulary rather than rate (a conversation is
   admitted because a trusted session used it at all), carried into the boot
   image the way `network-lightcone` carries its world; measure the two laws
   alone and together, chaos tried on the new law first. The `sense` shell
   command and `SYS_SENSE` 4/14/16 already give the readings. Until then the
   cone arms only inside `demos`.
2. **Partner keying (SYS_SENSE 14, default 0).** The right input (pid for
   pid-addressed calls, sub-function for multiplexed ones) but measured equal
   to the raw-argument keying, so it stays off. Turn it on when the admission
   law above depends on it, and re-measure the demo fleet.
3. **Erosion vs. learning window.** The sensor's `EROSION`, `FORMATION` and
   `READABLE` constants decide what a map can hold (about 900 sustained events
   to become readable; everything relaxes to the floor within some 20,000 quiet
   events). They were calibrated for E20 on the substrate, not for an OS
   session. Re-derive them from the chaos step once the admission law exists;
   do not retune by hand.
4. **Field-substrate bridge** (parked, [parked/README.md](parked/README.md)).
   Needs `field-core` and `kernel-glue` from the NAS archive copied into
   `third_party/`, and a wiring that keeps byte IPC beside the energy path.
5. **Kernel hardening**: SMEP/SMAP, user-page permissions, and a decision on
   multiprocessor support. Nothing started.
6. **Judge and keys after a false positive.** Three condemnation episodes
   certify destruction and the keys have one life per boot, so a benign process
   condemned three times (the shell during the E21 measurement) kills the
   keys for the rest of the boot. Revisit once arming is real: the judge's
   evidence rules were designed against the demo rogue only.

7. **Close the kernel trust gap for secrets** (discussed 2026-10-21; the
   possibilities, evidence and code references are in
   [IDEA-KERNEL-TRUST-GAP.md](IDEA-KERNEL-TRUST-GAP.md)). Today every key
   (E22 `kernel-key`, the E23 shares, the E35 master key) is a RAM static, the
   kernel is monolithic with drivers in ring 0, and there is no IOMMU, TPM or
   register-resident key, so a RAM snapshot or a compromised kernel gets the
   keys. Next steps, one at a time, each precommitted:
   - a. **E35 v2, registers-only key** (TRESOR-style: debug or vector
     registers, AES-NI in the CPU, the scheduler leaves those registers alone).
   - b. **TPM root share**: seal one E23 share, or the E35 root, in the
     machine's TPM 2.0 (`/dev/tpm0`).
   - c. **Conserved shares**: E23 rebuilt on Imperium's no-copy algebra
     (`encrypted key protection/`), so shares move instead of sitting copyable.
   - d. **Shares spread across places** (registers, Arc VRAM, TPM, a second
     machine), so no single snapshot holds them all.
   - e. **A ratchet**, so a copied state goes stale, with one verified lineage.
   - f. **A thermal PUF test on the Arc** (is its heat response chip-unique and
     stable?), from the synthetic-brain SENSE-1 and BEAT-1 stacks.
   - g. **A smaller trust base**: the secret lifecycle onto
     redox-kernel-audit's AuthorityWall capabilities, or drivers out of ring 0,
     plus IOMMU confinement.
   - h. **Standard crypto (AES-NI) and a real entropy source** for real
     secrets; the custom cipher stays for experiments.
8. **The front door: an immutable synthetic brain as the boundary organ**
   (Jesse, 2026-10-21: "protect the front door, not RAM"; see the last section
   of [IDEA-KERNEL-TRUST-GAP.md](IDEA-KERNEL-TRUST-GAP.md)). One organ decides
   what may enter and what may leave, for kernel and network: sense, perceive,
   judge every crossing, with immutable laws, scars of normal traffic, and
   temperature as vigilance. It builds on E24, E25 and E37, kernel-sense, and
   atom-coder's HOMEO-2.
   - Open problems: covert channels (timing, heat), encrypted traffic (judge
     behaviour), false positives (the E21 lesson), and which part is immutable.
   - This takes priority over item 7's RAM steps.

## Network and Lightcone (last)

7. **Network Lightcone generalization.** Fresh v3 holdouts pass the frozen
   pairwise gates but rank 0/6 at top-1; the training world is small
   ([network-lightcone/README.md](../network-lightcone/README.md), coverage
   gaps in `coverage.json`: IPv6, full TCP, reassembly, TLS and application
   context, authenticated lineage, copy-provenance enforcement, sustained
   adversarial operation). TPU acceptance is open.
8. **Immutable-atom composition boundary**: define and enforce it before the
   proposed network security architecture relies on it (README, Next
   milestones).
9. **E37/E38 with a NIC.** The console suite runs "wireless" (no NIC); the
   network demos pass by idling. `scripts/test-network-lightcone.sh` and the VM
   suite cover the virtio path; keep both green when the ingress changes.

## Desktop, programs and files

10. **Remaining built-in apps to windowed programs**: Files, Text Editor,
    Terminal, System Monitor; `ui-intent` needs list, scroll and text-area
    elements first.
11. **Program files on disk.** Manifests in `/apps` can name any path, but
    program files still arrive only with the boot image. Needs a safe write
    path plus the ELF check at load.
12. **Real hardware**: UEFI boot with the firmware framebuffer, AHCI/NVMe
    disks, USB keyboard and mouse. Today the desktop, disk and mouse need
    QEMU's devices. The from-scratch `bootloader/` (Lucerna-Labs line) is in
    the tree but not used for the image yet.
13. **Linux programs** (handed to another coder):
    [parked/linux-programs.md](parked/linux-programs.md). The
    `kernel-linux` draft is outside the workspace. Do not pick this up here.

## Housekeeping

14. **`atom-3d-engine` Git link.** An inherited, empty gitlink with no
    `.gitmodules` ([BACKUP.md](../BACKUP.md)); the working tree shows it
    deleted and uncommitted. Decide: commit the removal, or restore it.
15. **Diagnostic logs in the repo root** (`diag-*.log`, `q-*.log`,
    `qemu-*.log`, `run*.log`, `_build_err.txt`) are from past E-series and
    boot debugging. Move the ones worth keeping under `test-results/` and
    drop the rest.
16. **`NOTES.md`** still describes the July context-switch bug as "not yet
    fixed"; the current scheduler restores every task from its full trap
    frame. Retire or rewrite the note.
17. **`scripts/build.sh` has no execute bit**; run it as `bash scripts/build.sh`
    or restore the bit.

## How to resume any item

- Read the item's evidence first (the doc or README section it names), then
  the crate's module docs; the limits are stated there.
- Precommit predictions in a short doc before building (see
  [E21-ARMING.md](E21-ARMING.md) for the shape), measure with and without,
  keep failed predictions.
- `scripts/test-native.sh` for the host gates, `scripts/boot-test.py --accel
  kvm --output <dir>` for the 27 boot checks, `scripts/vm-test.sh` for the
  full suite on the `atom-os-dev` VM.
