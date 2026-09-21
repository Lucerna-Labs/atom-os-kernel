#!/usr/bin/env python3
"""Prove evaluation-only descriptions cannot affect trained parameters."""

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
if args.output.exists():
    raise FileExistsError(f"refusing to overwrite {args.output}")
args.output.mkdir(parents=True)

world_path = ROOT / "network-lightcone/sources/world.json"
world = json.loads(world_path.read_bytes())
baseline = json.loads(
    (
        ROOT
        / "network-lightcone/worlds/v3-local-diagnostic/geometry.json"
    ).read_bytes()
)
train_nodes = {
    edge[field]
    for edge in world["edges"]
    if edge["split"] == "train"
    for field in ("source", "target")
}
heldout_nodes = sorted(
    {
        edge[field]
        for edge in world["edges"]
        if edge["split"]
        in ("validation", "sealed_test", "cross_composition")
        for field in ("source", "target")
    }
    - train_nodes
)
if not heldout_nodes:
    raise ValueError("no evaluation-only node exists for isolation test")

changed_id = heldout_nodes[0]
for node in world["nodes"]:
    if node["id"] == changed_id:
        node["title"] = "Counterfactual audit description"
        node["statement"] = (
            "UDP application bytes checksum destination datagram "
            "counterfactual isolation marker"
        )
        node["epistemic_class"] = "synthetic_control"
        node["limitations"] = (
            "Audit-only perturbation; never promote as protocol evidence."
        )
        break
perturbed_world = args.output / "world.json"
perturbed_world.write_text(
    json.dumps(world, sort_keys=True, separators=(",", ":")) + "\n"
)
run = subprocess.run(
    [
        sys.executable,
        str(ROOT / "network-lightcone/tools/train.py"),
        "--world",
        str(perturbed_world),
        "--out",
        str(args.output / "trained"),
    ],
    env={
        **dict(__import__("os").environ),
        "JAX_PLATFORMS": "cpu",
        "LIGHTCONE_SOURCE_REVISION": "isolation-control",
        "LIGHTCONE_SOURCE_DIRTY": "test-only",
        "LIGHTCONE_JOB": "local-isolation-control",
    },
    capture_output=True,
    text=True,
)
(args.output / "stdout.txt").write_text(run.stdout)
(args.output / "stderr.txt").write_text(run.stderr)
candidate = json.loads(
    (args.output / "trained/geometry.json").read_bytes()
)
indices = {
    node_id: index
    for index, node_id in enumerate(baseline["node_ids"])
}
max_train_change = max(
    abs(before - after)
    for node_id in train_nodes
    for before, after in zip(
        baseline["coordinates"][indices[node_id]],
        candidate["coordinates"][indices[node_id]],
    )
)
max_relation_change = max(
    abs(before - after)
    for baseline_relation, candidate_relation in zip(
        baseline["relations"], candidate["relations"]
    )
    for before, after in zip(
        baseline_relation, candidate_relation
    )
)
result = {
    "passed": (
        max_train_change <= 1e-9
        and max_relation_change <= 1e-9
    ),
    "changed_evaluation_only_node": changed_id,
    "evaluation_only_nodes": heldout_nodes,
    "max_training_coordinate_change": max_train_change,
    "max_relation_vector_change": max_relation_change,
    "trainer_returncode": run.returncode,
}
(args.output / "result.json").write_text(
    json.dumps(result, indent=2) + "\n"
)
print(json.dumps(result, indent=2))
raise SystemExit(0 if result["passed"] else 1)
