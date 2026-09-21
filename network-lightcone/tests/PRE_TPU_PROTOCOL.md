# Pre-TPU local test protocol (2026-09-21)

Test source baseline: kernel commit 9223955. World and production runtime stay
unchanged while these probes are added. No TPU job or seal is created by tests.
Scratch packs, mutated inputs and network disks live under test-results only.

1. Recheck the existing source/pack/trainer hashes, native gates and real NIC
   checks. Repeat training with the same seed/configuration and compare learned
   arrays, excluding timestamps and receipts that intentionally name a new run.
2. Probe closed-world validation with missing sources, duplicate records,
   forbidden source rights, split leakage and inconsistent polarity. Invalid
   input must be rejected explicitly, including with Python optimization enabled.
3. Attempt to promote the CPU result through the default TPU-only finalizer,
   normally and with python -O. Neither may produce an accepted pack. Empty or
   incomplete required-gate maps must reject, even with diagnostic mode enabled.
   All negative-control outputs are isolated and never embedded.
4. Measure filtered top-1 rank and MRR separately from existing pairwise ranking.
   These are diagnostics; do not quietly reinterpret the original ranking gate.
   Check which evaluation-only node descriptions enter the negative objective.
5. Exercise actual NIC ingress/egress with correct traffic, malformed lengths,
   header/datagram checksum errors, fragments, unsupported protocols and payload
   strings resembling commands. A seed receipt must match the wire and source
   graph; malformed packets must not be mistaken for valid application input.
   Lightcone remains passive: packet-drop authority belongs to the stack.
6. Sustained bounded local load: three rounds of 256 outbound datagrams plus
   matching inbound traffic, both KVM and TCG. Require unchanged payload bytes,
   stable world hash, zero admission failures, no crash, explicit ledger eviction,
   correct retained receipt hashes, application progress, and no frame loss across
   identical second/third measurement phases after warmup.

Retain all failures and classify the owner. A failure blocks TPU readiness;
no threshold reduction or production fixes are bundled silently into this test.
This is a local development gate, not a long-term reliability claim.

## Instrument correction

The first sustained probe could see only 12 printed receipts. A missing console
line is not proof of a missing ledger record. The follow-up counts successful
read-only lookups and full console-write return values separately. It does not
chunk around, disable, or change the existing egress filter. The original failed
run is retained. Memory stability refers to physical free-page frames, distinct
from wire packet counts, which are checked separately.
