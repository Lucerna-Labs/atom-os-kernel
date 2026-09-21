# Network Lightcone contract — revision 1

Source of architecture: the installed Lightcone skill and the read-only
new-complete-atom-lightcone reference. This is a new network-specific world;
no sibling trained packs, feedback, or binaries are imported.

Lightcone owns passive admission from an immutable sourced typed causal graph
and full trained 48-dimensional coordinates. Packet parsing belongs to the
observation adapter. Kernel authorization, spider, taint, encryption, and
packet release remain separate owners. A receipt is context, never permission.
The observer projection has four spatial axes x,y,z,w and never admits nodes.

The world describes protocol mechanisms and their limits. A wire observation
selects represented mechanisms; admitting a conditional mechanism does not
assert its consequence occurred. Headers, checksums, or geometric proximity
cannot establish peer identity, confidentiality, legitimacy, or copy provenance.

## Frozen validation gates before training

- Closed source-bound graph; unique stable IDs; typed relations and polarity,
  conditions/exceptions/scope; mechanism-grouped train/validation/sealed and
  cross-composition splits fixed before training. No packet payload training.
- Train only the declared train relations; full learned geometry is 48D.
  Report train and held-out relation discrimination separately, plus loss,
  seed, configuration, actual devices, elapsed time and artifact hashes.
- Train relation versus corrupt-tail ranking >= 0.95. Held-out ranking >= 0.60.
  Report each split separately; no averaging away failures.
- Independent exact graph reachability must match runtime admission in past,
  future and both directions, with relation/polarity filters, hops and caps.
- Unknown seeds admit nothing; direction inversion and removed-edge controls
  must change the expected neighborhoods. Similarity cannot add graph edges.
- Immutable pack hash verification; mutations, invalid references, unexpected
  dimensions, malformed strings, and missing evidence are rejected.
- Shadow projection: train-only fit, orthonormal basis within 1e-5, no expansion
  above 1e-5; report collisions, contraction, recall and random-basis placebo.
- Real Kaggle TPU training is the accelerator acceptance boundary. Local runs
  are diagnostics, not substitutes. No paid quota purchase is authorized.
- VM: actual virtio ingress and successful egress both produce reproducible
  receipts with the same pack hash; packet bytes remain unchanged; payloads
  cannot inject seeds; malformed/unsupported frames stay explicit; missing or
  evicted receipts return missing; no world mutation syscall; IPC still works.
- Long-term reliability requires independent workload captures, clock/topology
  variation, sustained operation and adversarial review. It is an open gate,
  not something a generated corpus or this development turn can establish.

## Status and coverage

Discovered -> sourced -> compiled -> trained -> locally_validated ->
accelerator_validated -> VM_validated. Sealing additionally requires the
long-term gates and a closed versioned coverage ledger. Failures are retained;
thresholds cannot be changed after seeing held-out results.

Revision 1 covers the observed Ethernet/ARP/IPv4/UDP/ICMP boundaries and TCP
header context. TCP transport service, IPv6, TLS/application semantics,
reassembly, authenticated session lineage, declassification and copy prevention
remain visible coverage gaps. Reasoning and model adapters are not inside this
passive network component. Future reasoning consumers must retain explicit lanes.

## Source revision 2: recorded correction before retraining

Local run 001 failed cross-composition ranking (0.34669). Its entire evaluated
holdout is now regression evidence, not an unseen seal. Revision 2 adds missing
sourced transport-content, encapsulation, checksum and header mechanisms. New
mechanism groups for fragmentation constraints, optional UDP source ports, ICMP
error variants and fragmented transports were frozen before the next run. The
ranking gates remain 0.95 train and 0.60 for each new held-out split. Prior
regression metrics remain separately reported. New heldouts are not optimizer
inputs. This is a small protocol world, not extensive general security training.
