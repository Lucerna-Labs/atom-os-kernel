# Atom network Lightcone

## Definition and source

Lightcone is passive causal geometry over an immutable trained world. It
answers which events can reach another event under the world's declared
propagation law. It does not classify traffic, choose policy, grant authority,
score threats, or change its law from packets. The geometric containment
operation is hashless. Hashes in external training receipts identify artifacts;
they are not the geometry or a decision rule.

This kernel port follows:

- `atom block chain/docs/LIGHTCONE-PHYSICS.md` (E5 measured-law interpretation).
- `atom block chain/crates/atom-chain/src/cone.rs` (E8 ring containment).
- `Atom-lightcone-27b-harness/cognitive-engine/docs/DECISIONS.md`, D60 correction:
  the original is geometric; a typed/hash knowledge layer is a different layer.
- `imperium-main/imperium-main/crates/imperium-net-mesh/src/lightcone.rs`,
  including its offline propagation calibration and passive observation model.

This particular network world uses an overlay ring with a protocol tick:
`ring_distance(event_position, cause_position) <= v_max * elapsed_ticks`,
with strictly earlier causes. This is the canonical E8/Imperium network model;
it does not redefine Atom's separate all-spatial 4D projections as spacetime.
The integer implementation validates coordinates and uses full-width arithmetic.

## Components and actual integration

`kernel-lightcone` is a dependency-free `no_std` crate. Its `World` fields are
private, fixed at construction, and have no setter. `Lightcone::query` takes a
causal observation and returns geometric facts: no stated cause, within reach,
outside reach, or an invalid/different-world input. There is no enforcement or
live-training interface.

`kernel-net/src/lightcone.rs` is the transport adapter outside that geometry.
It records incoming frames at `handle_frame` and outgoing frames only after
successful virtio transmit. Its bounded 64-record ledger retains explicit
sequence identities; evicted and missing records remain missing, never valid.
Querying a record does not change the world. Untagged packets count as unknown
ancestry. Existing packet handling, egress filtering, taint, spider and key
policy remain separate.

Ordinary IP headers do not supply trustworthy causal ancestry. An opt-in UDP
or ICMP payload may state a claim using:

```text
ATLC1 world_id event_tick event_position cause_tick cause_position
ATLC1 world_id event_tick event_position none
```

The adapter observes these bytes without rewriting or intercepting them.
Coordinates, clock domain, and ancestry in this format are **claims**, not
cryptographically authenticated physical observations. A geometric fact is
conditional on the declared world and those coordinates. Correct geometry
alone does not prove a packet benign, prevent copying or authenticate a peer.
Unsupported protocols and untagged payloads do not receive invented geometry.

`SYS_LIGHTCONE` (55) exposes read-only taps. Subcommand 0 takes a status selector:
0 world ID, 1 ring size, 2 velocity, 3 incoming frames, 4 outgoing frames,
5 total claims, 6 malformed claim payloads, 7 oldest retained sequence,
8 untagged inbound, 9 untagged outbound. Subcommand 1 takes record sequence and
field: 0 direction, 1 world ID, 2 event tick, 3 event position, 4 cause present,
5 cause tick, 6 cause position, 7 fact, 8 distance, 9 required minimum ticks.
Fact values: 0 no cause, 1 within, 2 outside, 3 invalid position, 4 other world.
Missing fields/records/subcommands return the existing `ERROR` sentinel.

## Build and verify

```sh
bash scripts/build.sh
bash scripts/test-network-lightcone.sh
bash scripts/vm-test-lightcone.sh
```

The first command builds all embedded programs and validates them with the actual
ELF loader. The native suite checks exact geometry against independently expanded
ring reachability, invalid positions, same/future-tick ancestry, overflow,
serialization, immutable queries, calibration controls, and passive bookkeeping.
The VM wrapper builds a fresh image in `atom-os-dev` and tests a real virtio NIC
in both directions under KVM and TCG on an isolated loopback Ethernet transport.
It does not contact external hosts or touch the continuing interactive disk.
`netstat.elf` displays world and observation counts. The worker's
`--lightcone-test` mode is the kernel-side test probe used by the VM harness.

## Training an actual world

Training calibrates the propagation law, not maliciousness thresholds. Offline
calibration rows have five whitespace-separated columns:

```text
# direction cause_tick cause_position event_tick event_position
in  0  0  12  4
out 20  4  32  0
```

Use at least 16 independently reviewed positive-distance causal hops in EACH
direction. All positions must belong to the declared ring; clocks must share a
documented monotonic domain/unit. The paired event must be later than its cause.
The canonical calibration uses median+8*MAD trimming and a 1.5 propagation
safety factor, then declares an integer velocity. That is a calibrated model;
it is not a proof of a universal physical speed bound or immunity to poisoning.

```sh
bash scripts/train-network-lightcone.sh reviewed-train.tsv heldout.tsv 2 64 candidate-world.bin
```

The arguments are train, independent normal holdout, NEW world ID, ring size,
and output artifact. Every held-out hop must be geometrically constructible.
Keep training and held-out sessions separate. Preserve sources, topology/clock
mapping, rejected samples, replay metrics and the prior world artifact. Review
these before replacing the embedded development world and rebuilding. A new law
is a new world; live packets and orchestrators cannot edit the active world.

## Evidence boundary and further training

The committed `worlds/dev-ring.bin` is a DEVELOPMENT CONTROL: a 64-site ring,
world ID 1, velocity 1. Reproduce its 100,000 generated calibration hops and
50,000 separately seeded held-out hops with:

```sh
python3 scripts/lightcone-calibration-battery.py --output target/lightcone-calibration-new
```

The receipt labels these as generated reference-world controls, not captured
traffic. Long-term reliability still requires genuine calibrated network/world
observations across workloads, topology changes and clock conditions; independent
holdouts; corrupted/forged ancestry controls; honest unknowns; and sustained
operation. Neither large generated sample counts nor a successful VM run stand
in for that work. Kernel enforcement and secret-flow authorization are separate
components and have not been added as part of this passive Lightcone port.

## Rejected attempt

The preceding traffic-feature envelope was the wrong interpretation. Its source
and results were preserved locally under
`test-results/rejected-network-envelope-20260920/` and its integration was removed.
It was never committed, deployed, or accepted as Lightcone. Those classifier
scores are not evidence for this geometric component.
