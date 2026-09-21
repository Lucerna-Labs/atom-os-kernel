# Network Lightcone

This component follows the installed `lightcone` skill: a passive admission
boundary over a sourced typed causal world with trained full geometry. It is
separate from the kernel's existing normality map, substrate, spider, taint,
key handling, egress filter and authorization.

## What exists

- 63 source-bound network concepts and 66 typed relationships. Every edge
  retains its relation, polarity, condition, exception, mechanism split, scope
  and source IDs. The world covers protocol context; admitted concepts are
  conditional knowledge, not assertions that an observed packet is legitimate.
- A real offline JAX trainer learning 48D geometry from train-only mechanisms
  and text statistics, with independent validation, sealed-test and cross-
  composition groups. Previously observed failures stay in regression records.
- An observer-only train-fitted spatial x,y,z,w projection. The kernel loader
  recomputes the projection and checks its orthonormal certificate. The shadow
  never selects nodes. Coordinates are stored in fixed-point millionths.
- A no_std kernel runtime that checks the embedded pack SHA256, validates
  references, preserves exact typed neighborhoods and emits deterministic
  receipts. Geometry cannot invent edges or turn similarity into truth.
- NIC observation paths for ingress and successfully transmitted egress.
  Structurally validated headers supply exact observation seeds; payload text
  cannot submit node IDs or commands. Unsupported, malformed and fragmented
  inputs remain explicit. Live packets never train or edit the world.
- A 32-receipt bounded ledger with explicit sequence numbers and missing/evicted
  entries. The same pack can serve both directions; wire direction is separate
  from causal graph traversal direction.

Current embedded pack: `worlds/v3-local-rtx-diagnostic`. JAX 0.11.2 trained
it on the local NVIDIA GeForce RTX 5070 Ti in 3.998 seconds. Its recorded
platform remains `local_diagnostic`, **not TPU accepted or sealed**. The
compiler, promotion, training-isolation, malformed-packet and sustained
KVM/TCG gates pass. See
[the RTX validation report](receipts/local-rtx-v3-20260921.md). The runtime
reports training platform and `sealed=false`.

## Build and inspect

From the kernel repository root:

```sh
bash scripts/build.sh
bash scripts/test-network-lightcone.sh
bash scripts/vm-test-network-lightcone.sh
```

Inside Atom OS:

```text
spawn netstat.elf
spawn worker.elf --network-lightcone
```

`SYS_LIGHTCONE` (55) takes subcommand and index. It returns a JSON byte count in
the caller's `LIGHTCONE_PAGE` (the existing network receive page), or `ERROR`.
Subcommands: 0 world information; 1 receipt by sequence (0 latest); 2 node by
index; 3 edge by index; 4 full 48D coordinates plus observer shadow by node.
Other subcommands are invalid. Reading does not retrain or alter the world.
Receipts include the pack, query and receipt hashes, seeds, admitted nodes,
exact edges, excluded frontier and hop frontier, policy and transport direction.
Consumers must honor negative polarity and conditions; traversal is not proof
that every admitted statement is true of this packet.

## Reproduce training

`CONTRACT.md` freezes the gates and records the failed first experiment.
`sources/world.json` and `sources/splits.json` are pinned training inputs.
The source authoring script refreshes public RFC hashes; it is not part of a
replay. Replays use committed inputs and an environment with NumPy and JAX.

```sh
JAX_PLATFORMS=cpu python3 network-lightcone/tools/train.py \
  --world network-lightcone/sources/world.json --out target/new-local-run
```

This command creates a new local diagnostic and refuses an existing output
folder. For accelerator acceptance, run the same trainer with `--require-tpu`
on actual TPU devices; a requested device label cannot satisfy that check.
The clean-revision payload generator is
`network-lightcone/tools/prepare_kaggle.py`. No packet captures, credentials
or sibling trained worlds enter that payload.

The finalizer checks world/trainer/output identities, gates, actual platform,
projection reconstruction and quantization, then creates a new versioned pack:

```sh
python3 network-lightcone/tools/finalize.py \
  --world network-lightcone/sources/world.json \
  --training path/to/verified-tpu-output \
  --out network-lightcone/worlds/NEW_VERSION --kernel kernel-lightcone/src
```

Only explicit `--diagnostic` permits a local pack. Retain prior packs and failed
receipts; rebuild and rerun the NIC tests after any embedded pack changes.
The original compile-time SHA256 remains the integrity anchor. A kernel or
boot-chain compromise is outside that guarantee; this is not a claim that
machine instructions or memory bits are physically unalterable.

## Scope and evidence limits

The exact graph and receipt tests include all seeds, all three causal
directions, five hop settings, six relation filters and three polarity modes;
unknowns, inversion/edge-removal controls, caps, malformed packs and observer
independence. The VM test uses the actual virtio driver in both directions,
verifies packet bytes and recomputes every received graph receipt independently.
It uses only loopback Ethernet and isolated test disks.

The training world is small. Pairwise corrupt-tail ranking is not attack
recognition accuracy. Fresh v3 holdouts pass the frozen pairwise gates but rank
0/6 at top-1, so broad generalization is not established. `coverage.json`
keeps gaps open: IPv6, full
TCP transport, reassembly, TLS/application context, authenticated lineage,
copy-provenance enforcement and sustained adversarial operation. An admission
receipt cannot itself stop copying or authorize packet release. No copied-data
prevention mechanism is claimed by this work.

Sources: original factual summaries of RFCs [768](https://www.rfc-editor.org/rfc/rfc768),
[791](https://www.rfc-editor.org/rfc/rfc791),
[792](https://www.rfc-editor.org/rfc/rfc792),
[826](https://www.rfc-editor.org/rfc/rfc826), and
[9293](https://www.rfc-editor.org/rfc/rfc9293), plus the pinned kernel network
implementation. Source locators, versions, hashes and permitted use are in the
world file. RFC texts are not redistributed or used as bulk training text.
