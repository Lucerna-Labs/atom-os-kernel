#!/usr/bin/env python3
"""Validate a training receipt and assemble a new immutable world pack."""

import argparse
import json
import math
import re
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from train import (  # noqa: E402
    REQUIRED_GATES,
    SPLITS,
    canonical,
    digest,
    split_manifest,
    validate,
)

HEX_REVISION = re.compile(r"[0-9a-f]{40,64}\Z")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def finite_number(value):
    return isinstance(value, (int, float)) and math.isfinite(value)


parser = argparse.ArgumentParser()
parser.add_argument("--world", type=Path, required=True)
parser.add_argument("--training", type=Path, required=True)
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--kernel", type=Path, required=True)
parser.add_argument("--diagnostic", action="store_true")
args = parser.parse_args()

require(not args.out.exists(), f"refusing to overwrite {args.out}")
raw_world = args.world.read_bytes()
world = json.loads(raw_world)
node_ids = validate(world)
node_index = {node_id: index for index, node_id in enumerate(node_ids)}

receipt_bytes = (args.training / "training.json").read_bytes()
receipt = json.loads(receipt_bytes)
geometry_bytes = (args.training / "geometry.json").read_bytes()
geometry = json.loads(geometry_bytes)
expected_manifest = split_manifest(world)

require(receipt.get("schema") == 2, "training receipt schema must be 2")
require(receipt.get("world_sha256") == digest(raw_world), "world hash mismatch")
require(
    receipt.get("split_manifest_sha256")
    == digest(canonical(expected_manifest) + b"\n"),
    "split-manifest hash mismatch",
)
require(
    receipt.get("trainer_sha256")
    == digest((Path(__file__).parent / "train.py").read_bytes()),
    "trainer hash mismatch",
)
require(
    receipt.get("geometry_sha256") == digest(geometry_bytes),
    "geometry hash mismatch",
)
require(receipt.get("dimensions") == 48, "receipt dimensions must be 48")
require(receipt.get("seed") == 73129, "unexpected training seed")
require(receipt.get("steps") == 3000, "unexpected training step count")
require(
    isinstance(receipt.get("objective"), str) and receipt["objective"],
    "missing objective",
)
require(
    isinstance(receipt.get("jax"), str) and receipt["jax"],
    "missing JAX identity",
)
require(
    isinstance(receipt.get("devices"), list) and receipt["devices"],
    "missing device identity",
)
require(
    finite_number(receipt.get("started"))
    and finite_number(receipt.get("elapsed_seconds"))
    and receipt["elapsed_seconds"] >= 0,
    "invalid timing evidence",
)
require(
    receipt.get("required_gates") == list(REQUIRED_GATES),
    "required-gate manifest mismatch",
)
require(
    isinstance(receipt.get("gates"), dict)
    and set(receipt["gates"]) == set(REQUIRED_GATES),
    "named gate set is incomplete or unexpected",
)
require(
    all(receipt["gates"].get(name) is True for name in REQUIRED_GATES),
    "one or more required gates failed",
)
require(receipt.get("passed") is True, "training receipt did not pass")
require(
    isinstance(receipt.get("metrics"), dict)
    and set(receipt["metrics"]) == set(SPLITS),
    "metric splits are incomplete",
)
for split in SPLITS:
    metric = receipt["metrics"][split]
    require(metric.get("edges", 0) > 0, f"{split} has no evaluated edges")
    for field in ("pairwise_tail_ranking", "worst", "top1", "mrr"):
        require(
            finite_number(metric.get(field)),
            f"{split} metric {field} is missing or non-finite",
        )
require(
    receipt.get("training_node_ids")
    == receipt.get("negative_candidate_ids"),
    "training and negative-candidate node sets differ",
)
train_nodes = {
    edge[field]
    for edge in world["edges"]
    if edge["split"] == "train"
    for field in ("source", "target")
}
require(
    set(receipt.get("training_node_ids", [])) == train_nodes,
    "training node manifest does not match train endpoints",
)
require(
    receipt.get("train_mechanisms") == expected_manifest["train"],
    "train mechanism manifest mismatch",
)

if args.diagnostic:
    require(
        receipt.get("platform") == "local_diagnostic",
        "diagnostic finalization requires a local-diagnostic receipt",
    )
    status = "local_diagnostic"
else:
    require(
        receipt.get("platform") == "kaggle_tpu",
        "accelerator finalization requires Kaggle TPU evidence",
    )
    require(
        all(device.get("platform") == "tpu" for device in receipt["devices"]),
        "every recorded device must be an actual TPU",
    )
    require(
        isinstance(receipt.get("source_revision"), str)
        and HEX_REVISION.fullmatch(receipt["source_revision"]),
        "accelerator receipt has no immutable source revision",
    )
    require(
        str(receipt.get("source_dirty")).startswith("false"),
        "accelerator source must be clean",
    )
    require(
        isinstance(receipt.get("job"), str)
        and receipt["job"]
        not in {"", "local", "unrecorded"},
        "accelerator job identity is missing",
    )
    status = "accelerator_validated"

require(geometry.get("node_ids") == node_ids, "geometry node order mismatch")
coordinates = np.asarray(geometry.get("coordinates"), dtype=np.float64)
relations = np.asarray(geometry.get("relations"), dtype=np.float64)
shadow = np.asarray(geometry.get("shadow"), dtype=np.float64)
projection = geometry.get("projection", {})
center = np.asarray(projection.get("center"), dtype=np.float64)
basis = np.asarray(projection.get("basis"), dtype=np.float64)
require(
    coordinates.shape == (len(node_ids), 48),
    "coordinate shape mismatch",
)
require(
    relations.shape == (len(world["relations"]), 48),
    "relation geometry shape mismatch",
)
require(
    shadow.shape == (len(node_ids), 4),
    "shadow shape mismatch",
)
require(center.shape == (48,), "projection center shape mismatch")
require(basis.shape == (48, 4), "projection basis shape mismatch")
require(
    np.isfinite(coordinates).all()
    and np.isfinite(relations).all()
    and np.isfinite(shadow).all()
    and np.isfinite(center).all()
    and np.isfinite(basis).all(),
    "geometry contains non-finite values",
)
require(projection.get("observer_only") is True, "shadow is not observer-only")
require(
    projection.get("dimensions") == "x,y,z,w (spatial)",
    "observer axes are not spatial x,y,z,w",
)
require(
    projection.get("fit_node_ids")
    == receipt["training_node_ids"],
    "projection was not fitted on exactly the train nodes",
)
require(
    np.max(np.abs((coordinates - center) @ basis - shadow))
    < 1e-5,
    "shadow does not reconstruct",
)
require(
    np.max(np.abs(basis.T @ basis - np.eye(4))) < 1e-5,
    "projection basis is not orthonormal",
)

metadata = bytearray()
nodes = bytearray()
edges = bytearray()


def metadata_record(value):
    encoded = canonical(value)
    offset = len(metadata)
    metadata.extend(encoded)
    return struct.pack("<II", offset, len(encoded))


for index, node in enumerate(world["nodes"]):
    quantized = np.round(
        np.r_[coordinates[index], shadow[index]] * 1_000_000
    ).astype(np.int64)
    require(
        np.max(np.abs(quantized)) < 1_000_000_000,
        f"node {node['id']} cannot be safely quantized",
    )
    nodes += metadata_record(node) + struct.pack(
        "<52i", *quantized
    )

for edge in world["edges"]:
    edges += struct.pack(
        "<HHBbBB",
        node_index[edge["source"]],
        node_index[edge["target"]],
        world["relations"][edge["relation"]],
        edge["polarity"],
        SPLITS.index(edge["split"]),
        0,
    ) + metadata_record(edge)

header = struct.pack(
    "<8sHHHHI32s32s",
    b"ATLCNET2",
    len(node_ids),
    len(world["edges"]),
    48,
    world["version"],
    len(metadata),
    bytes.fromhex(digest(raw_world)),
    bytes.fromhex(digest(receipt_bytes)),
)
transform = np.round(
    np.r_[center, basis.reshape(-1)] * 1_000_000
).astype(np.int64)
require(
    len(transform) == 240
    and np.max(np.abs(transform)) < 1_000_000_000,
    "projection transform cannot be safely quantized",
)
pack = (
    header
    + struct.pack("<240i", *transform)
    + nodes
    + edges
    + metadata
)

args.out.mkdir(parents=True)
for name, data in (
    ("world.bin", pack),
    ("world.json", raw_world),
    ("geometry.json", geometry_bytes),
    ("training.json", receipt_bytes),
):
    (args.out / name).write_bytes(data)
manifest = {
    "schema": 3,
    "world_id": world["id"],
    "version": world["version"],
    "status": status,
    "sealed": False,
    "passive": True,
    "full_dimensions": 48,
    "observer_axes": ["x", "y", "z", "w"],
    "source_sha256": digest(raw_world),
    "split_manifest_sha256": receipt["split_manifest_sha256"],
    "files": {
        name: digest((args.out / name).read_bytes())
        for name in (
            "world.bin",
            "world.json",
            "geometry.json",
            "training.json",
        )
    },
}
(args.out / "manifest.json").write_bytes(
    canonical(manifest) + b"\n"
)

args.kernel.mkdir(parents=True, exist_ok=True)
(args.kernel / "world.bin").write_bytes(pack)
generated = (
    "//! Generated by network-lightcone/tools/finalize.py; "
    "provenance is retained in the versioned pack.\n"
)
generated += (
    "pub const TRAINING_PLATFORM: &str = "
    + json.dumps(receipt["platform"])
    + ";\n"
)
generated += (
    'pub const PACK: &[u8] = include_bytes!("world.bin");\n'
)
generated += (
    "pub const HASH: [u8;32] = "
    + repr(list(bytes.fromhex(digest(pack))))
    + ";\n"
)
for index, name in enumerate(node_ids):
    generated += (
        f"pub const {name.upper()}: usize = {index};\n"
    )
(args.kernel / "embedded.rs").write_text(generated)
print(json.dumps(manifest, indent=2))
