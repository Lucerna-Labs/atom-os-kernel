# Corrected passive network Lightcone — September 20, 2026

The preceding traffic-envelope attempt was rejected by the user as a different
mechanism. Its exact source/patch and results were archived locally in
`../rejected-network-envelope-20260920/`, then all of its active wiring was
removed. This implementation follows the D60 correction, E8 containment and
Imperium's passive network Lightcone, not that classifier.

## Implemented and measured

- Independent no_std `kernel-lightcone`: immutable world, hashless reachability,
  strict past ancestry, validated coordinates, full-width arithmetic and facts
  with no confidence/severity/policy. No live model-mutating interface.
- Network adapter records incoming and successfully transmitted frames. Explicit
  ATLC1 claims carry coordinates; ordinary traffic remains unplaced. The bounded
  ledger exposes missing/evicted records rather than treating them as valid.
- Read-only syscall taps, netstat observability, offline geometry calibration
  and reproducible source-linked receipt generation.
- 32 existing native tests, 8 geometry/calibration tests and 3 network-tap/checksum
  tests passed. The exhaustive control compares containment with independently
  expanded ring graph reachability. Poisoned calibration outliers, invalid
  worlds/positions, arithmetic extremes, immutable queries and ledger eviction
  have specific gates.
- KVM and TCG each passed six checks through a real virtio NIC on an isolated
  local QEMU Ethernet link. Both observed four inbound and one outbound claim:
  two within the declared world, two outside, one for another world. The exact
  outbound payload crossed unchanged, read-only behavior was checked, and IPC
  remained operational. Both backends used the same boot image:
  `54c0f02c13cd18eb9db703b75eb0c589762abc653063a4049e5d68c411a0e9f8`.
- All compiled source inputs match the VM snapshot. Subsequent changes are
  documentation and the reproducible calibration-control script/receipts.

Evidence: `../lightcone-20260920T210301337729173Z/verification.json`, its native
log, KVM/TCG results, QMP and serial logs, wire captures and calibration receipt.

## Training boundary

The embedded development ring was calibrated with 100,000 GENERATED causal hops
and 50,000 separately seeded held-out hops. Both directions were covered and
all held-out normal hops were constructible under the fitted law. These are
reference-world controls, not captured traffic or a long-term reliability claim.
No artifact from the rejected traffic-envelope training is used here.

Real deployment still needs trusted site/topology mapping, a common documented
clock/unit, reviewed causal observations, separate holdouts and extended
validation under drift/topology changes. The wire claim format does not prove
that a peer's claimed coordinates are authentic. The model's geometric facts
are conditional on its declared world and the input claims.

This passive component does not itself block attacks, recognize copied secrets,
revoke authority or decide policy. Those are distinct mechanisms. No broader
kernel-security or physical-network guarantee is inferred from these tests.
