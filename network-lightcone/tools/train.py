#!/usr/bin/env python3
"""Train full geometry while keeping holdout mechanisms outside optimization."""

import argparse
import hashlib
import json
import os
import re
import time
from pathlib import Path

import numpy as np

SPLITS = ("train", "validation", "sealed_test", "cross_composition", "regression")
REQUIRED_GATES = (
    "train_ranking",
    "validation_ranking",
    "sealed_ranking",
    "cross_composition_ranking",
    "finite",
    "orthonormal",
    "nonexpansion",
)
ALLOWED_RIGHTS = {"open", "green", "citation_only", "user_authorized"}
ALLOWED_EPISTEMIC = {
    "source_grounded_mechanism",
    "implementation_observation",
    "unsupported_inference",
    "synthetic_control",
}
HEX_256 = re.compile(r"[0-9a-f]{64}\Z")
IDENTIFIER = re.compile(r"[a-z0-9][a-z0-9_.-]*\Z")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def normalized(text):
    return " ".join(text.casefold().split())


def split_manifest(world):
    return {
        split: sorted(
            {
                edge["mechanism_group"]
                for edge in world["edges"]
                if edge["split"] == split
            }
        )
        for split in SPLITS
    }


def validate(world):
    """Validate source rights, graph closure, polarity, and split isolation."""
    require(world.get("schema") == 2, "world schema must be 2")
    require(
        world.get("dimensions") == 48, "authoritative geometry must be 48D"
    )
    require(
        isinstance(world.get("id"), str) and IDENTIFIER.fullmatch(world["id"]),
        "invalid world id",
    )
    require(
        isinstance(world.get("version"), int) and world["version"] > 0,
        "invalid world version",
    )

    nodes = world.get("nodes")
    edges = world.get("edges")
    sources = world.get("sources")
    relations = world.get("relations")
    require(
        isinstance(nodes, list) and 0 < len(nodes) <= 128,
        "node count outside 1..=128",
    )
    require(
        isinstance(edges, list) and len(edges) <= 256,
        "edge count exceeds 256",
    )
    require(isinstance(sources, list) and sources, "source registry is empty")
    require(
        isinstance(relations, dict) and relations, "relation registry is empty"
    )
    require(
        set(relations.values()) == set(range(len(relations))),
        "relation codes must be contiguous",
    )

    source_by_id = {}
    for source in sources:
        source_id = source.get("id")
        require(
            isinstance(source_id, str) and IDENTIFIER.fullmatch(source_id),
            "invalid source id",
        )
        require(
            source_id not in source_by_id, f"duplicate source id: {source_id}"
        )
        for field in (
            "title",
            "author",
            "url",
            "version",
            "use",
            "trust_role",
            "attribution",
        ):
            require(
                isinstance(source.get(field), str) and source[field].strip(),
                f"source {source_id} missing {field}",
            )
        require(
            source.get("rights_lane") in ALLOWED_RIGHTS,
            f"source {source_id} has forbidden or unknown rights",
        )
        require(
            isinstance(source.get("sha256"), str)
            and HEX_256.fullmatch(source["sha256"]),
            f"source {source_id} has invalid sha256",
        )
        source_by_id[source_id] = source

    node_ids = []
    statement_owners = {}
    for node in nodes:
        node_id = node.get("id")
        require(
            isinstance(node_id, str) and IDENTIFIER.fullmatch(node_id),
            "invalid node id",
        )
        require(node_id not in node_ids, f"duplicate node id: {node_id}")
        node_ids.append(node_id)
        for field in (
            "title",
            "statement",
            "scope",
            "limitations",
            "record_kind",
            "as_of",
            "version_boundary",
        ):
            require(
                isinstance(node.get(field), str) and node[field].strip(),
                f"node {node_id} missing {field}",
            )
        require(
            node.get("epistemic_class") in ALLOWED_EPISTEMIC,
            f"node {node_id} has unknown epistemic class",
        )
        require(
            isinstance(node.get("source_ids"), list) and node["source_ids"],
            f"node {node_id} has no sources",
        )
        require(
            set(node["source_ids"]) <= source_by_id.keys(),
            f"node {node_id} references an unknown source",
        )
        require(
            isinstance(node.get("lanes"), list) and node["lanes"],
            f"node {node_id} has no lanes",
        )
        require(
            isinstance(node.get("coverage_axes"), list)
            and node["coverage_axes"],
            f"node {node_id} has no coverage axes",
        )
        statement = normalized(node["statement"])
        require(
            statement not in statement_owners,
            f"duplicate normalized statement: {node_id} and {statement_owners.get(statement)}",
        )
        statement_owners[statement] = node_id

    node_set = set(node_ids)
    edge_ids = set()
    mechanism_splits = {}
    relation_splits = {}
    expected_polarity = {
        name: (-1 if name == "does_not_establish" else 1)
        for name in relations
    }
    for edge in edges:
        edge_id = edge.get("id")
        require(
            isinstance(edge_id, str) and IDENTIFIER.fullmatch(edge_id),
            "invalid edge id",
        )
        require(edge_id not in edge_ids, f"duplicate edge id: {edge_id}")
        edge_ids.add(edge_id)
        require(
            edge.get("source") in node_set and edge.get("target") in node_set,
            f"edge {edge_id} has dangling endpoint",
        )
        relation = edge.get("relation")
        require(relation in relations, f"edge {edge_id} has unknown relation")
        require(
            edge.get("polarity") == expected_polarity[relation],
            f"edge {edge_id} polarity conflicts with {relation}",
        )
        require(
            edge.get("split") in SPLITS,
            f"edge {edge_id} has unknown split",
        )
        for field in (
            "mechanism_group",
            "conditions",
            "exceptions",
            "scope",
            "epistemic_class",
            "evidence",
            "direction",
        ):
            require(
                isinstance(edge.get(field), str) and edge[field].strip(),
                f"edge {edge_id} missing {field}",
            )
        require(
            isinstance(edge.get("source_ids"), list) and edge["source_ids"],
            f"edge {edge_id} has no provenance",
        )
        require(
            set(edge["source_ids"]) <= source_by_id.keys(),
            f"edge {edge_id} references an unknown source",
        )
        group = edge["mechanism_group"]
        previous = mechanism_splits.setdefault(group, edge["split"])
        require(
            previous == edge["split"],
            f"mechanism group {group} leaks across splits",
        )
        identity = (
            edge["source"],
            edge["target"],
            relation,
            edge["polarity"],
        )
        previous = relation_splits.setdefault(identity, edge["split"])
        require(
            previous == edge["split"],
            f"relation {identity} leaks across splits",
        )

    manifest = split_manifest(world)
    for split in SPLITS:
        require(manifest[split], f"required split {split} is empty")
    return node_ids


def ranking_metrics(world, node_index, coordinates, relation_vectors):
    metrics = {}
    for split in SPLITS:
        pairwise = []
        ranks = []
        for edge in world["edges"]:
            if edge["split"] != split:
                continue
            source = node_index[edge["source"]]
            target = node_index[edge["target"]]
            query = (
                coordinates[source]
                + relation_vectors[world["relations"][edge["relation"]]]
            )
            distances = ((coordinates - query) ** 2).sum(1)
            excluded = {source} | {
                node_index[other["target"]]
                for other in world["edges"]
                if other["source"] == edge["source"]
                and other["relation"] == edge["relation"]
            }
            negatives = [
                candidate
                for candidate in range(len(coordinates))
                if candidate not in excluded
            ]
            pairwise.append(
                float((distances[target] < distances[negatives]).mean())
            )
            ranks.append(
                1
                + sum(
                    distances[candidate] <= distances[target]
                    for candidate in negatives
                )
            )
        require(pairwise, f"split {split} has no evaluation edges")
        metrics[split] = {
            "edges": len(pairwise),
            "pairwise_tail_ranking": float(np.mean(pairwise)),
            "worst": float(min(pairwise)),
            "top1": float(np.mean([rank == 1 for rank in ranks])),
            "mrr": float(np.mean([1.0 / rank for rank in ranks])),
            "ranks": ranks,
        }
    return metrics


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--world", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--require-tpu", action="store_true")
    args = parser.parse_args()
    if args.out.exists():
        raise FileExistsError(f"refusing to overwrite {args.out}")
    args.out.mkdir(parents=True)

    raw_world = args.world.read_bytes()
    world = json.loads(raw_world)
    node_ids = validate(world)
    node_index = {
        node_id: index for index, node_id in enumerate(node_ids)
    }

    import jax
    import jax.numpy as jnp

    devices = [
        {
            "platform": device.platform,
            "kind": device.device_kind,
            "id": device.id,
        }
        for device in jax.devices()
    ]
    if args.require_tpu and (
        not devices
        or not all(device["platform"] == "tpu" for device in devices)
    ):
        raise RuntimeError("actual TPU devices are required")

    started = time.time()
    random = np.random.default_rng(73129)
    dimensions = 48
    train_edges = [
        edge for edge in world["edges"] if edge["split"] == "train"
    ]
    train_indices = sorted(
        {
            node_index[edge[field]]
            for edge in train_edges
            for field in ("source", "target")
        }
    )
    train_position = {
        node_id: position for position, node_id in enumerate(train_indices)
    }

    documents = [
        re.findall(
            r"[a-z0-9]+",
            (node["title"] + " " + node["statement"]).lower(),
        )
        for node in world["nodes"]
    ]
    vocabulary = sorted(
        {term for index in train_indices for term in documents[index]}
    )
    vocabulary_index = {
        term: index for index, term in enumerate(vocabulary)
    }
    features = np.zeros(
        (len(node_ids), len(vocabulary)), np.float32
    )
    for row, document in enumerate(documents):
        for term in document:
            if term in vocabulary_index:
                features[row, vocabulary_index[term]] += 1
    document_frequency = (features[train_indices] > 0).sum(0)
    inverse_document_frequency = (
        np.log(
            (1 + len(train_indices))
            / (1 + document_frequency)
        )
        + 1
    )
    features *= inverse_document_frequency
    features /= np.maximum(
        np.linalg.norm(features, axis=1, keepdims=True), 1e-6
    )

    feature_matrix = jnp.array(features)
    sources = jnp.array(
        [node_index[edge["source"]] for edge in train_edges]
    )
    targets = jnp.array(
        [node_index[edge["target"]] for edge in train_edges]
    )
    relation_ids = jnp.array(
        [world["relations"][edge["relation"]] for edge in train_edges]
    )
    candidate_indices = jnp.array(train_indices)
    valid_negatives = np.ones(
        (len(train_edges), len(train_indices)), np.float32
    )
    for row, edge in enumerate(train_edges):
        valid_negatives[
            row, train_position[node_index[edge["source"]]]
        ] = 0
        for other in train_edges:
            if (
                other["source"] == edge["source"]
                and other["relation"] == edge["relation"]
            ):
                valid_negatives[
                    row, train_position[node_index[other["target"]]]
                ] = 0
    valid_negatives = jnp.array(valid_negatives)

    parameters = (
        jnp.array(
            random.normal(
                0, 0.12, (len(vocabulary), dimensions)
            ),
            dtype=jnp.float32,
        ),
        jnp.array(
            random.normal(
                0, 0.08, (len(world["relations"]), dimensions)
            ),
            dtype=jnp.float32,
        ),
    )

    def loss(parameters):
        coordinates = feature_matrix @ parameters[0]
        query = (
            coordinates[sources]
            + parameters[1][relation_ids]
        )
        positive = jnp.sum(
            (query - coordinates[targets]) ** 2, 1
        )
        negative = jnp.sum(
            (
                query[:, None, :]
                - coordinates[candidate_indices][None, :, :]
            )
            ** 2,
            2,
        )
        hinge = (
            jnp.sum(
                jnp.maximum(
                    0, 1 + positive[:, None] - negative
                )
                * valid_negatives
            )
            / jnp.sum(valid_negatives)
        )
        return (
            hinge
            + 0.01 * jnp.mean(positive)
            + 0.0001
            * sum(
                jnp.mean(value * value)
                for value in parameters
            )
        )

    value_and_gradient = jax.jit(jax.value_and_grad(loss))
    first_moment = tuple(
        jnp.zeros_like(value) for value in parameters
    )
    second_moment = tuple(
        jnp.zeros_like(value) for value in parameters
    )
    curve = []
    for step in range(1, 3001):
        value, gradient = value_and_gradient(parameters)
        first_moment = tuple(
            0.9 * old + 0.1 * new
            for old, new in zip(first_moment, gradient)
        )
        second_moment = tuple(
            0.999 * old + 0.001 * new * new
            for old, new in zip(second_moment, gradient)
        )
        parameters = tuple(
            parameter
            - 0.01
            * (moment / (1 - 0.9**step))
            / (
                jnp.sqrt(
                    square / (1 - 0.999**step)
                )
                + 1e-8
            )
            for parameter, moment, square in zip(
                parameters, first_moment, second_moment
            )
        )
        if step in (1, 100, 500, 1000, 2000, 3000):
            curve.append({"step": step, "loss": float(value)})

    coordinates = np.array(feature_matrix @ parameters[0])
    relation_vectors = np.array(parameters[1])
    require(
        np.isfinite(coordinates).all()
        and np.isfinite(relation_vectors).all(),
        "training produced non-finite values",
    )
    metrics = ranking_metrics(
        world, node_index, coordinates, relation_vectors
    )

    center = coordinates[train_indices].mean(0)
    _, _, right_vectors = np.linalg.svd(
        coordinates[train_indices] - center,
        full_matrices=False,
    )
    basis = right_vectors[:4].T
    shadow = (coordinates - center) @ basis
    pairwise = np.linalg.norm(
        coordinates[:, None] - coordinates[None, :], axis=2
    )
    shadow_pairwise = np.linalg.norm(
        shadow[:, None] - shadow[None, :], axis=2
    )
    upper = np.triu_indices(len(node_ids), 1)
    placebo, _ = np.linalg.qr(
        random.normal(size=(48, 4))
    )
    placebo_coordinates = (coordinates - center) @ placebo
    placebo_pairwise = np.linalg.norm(
        placebo_coordinates[:, None]
        - placebo_coordinates[None, :],
        axis=2,
    )

    def recall(distances):
        return float(
            np.mean(
                [
                    len(
                        set(np.argsort(pairwise[index])[1:6])
                        & set(
                            np.argsort(distances[index])[1:6]
                        )
                    )
                    / 5
                    for index in range(len(node_ids))
                ]
            )
        )

    projection = {
        "center": center.tolist(),
        "basis": basis.tolist(),
        "dimensions": "x,y,z,w (spatial)",
        "fit_node_ids": [
            node_ids[index] for index in train_indices
        ],
        "orthonormal_error": float(
            np.max(np.abs(basis.T @ basis - np.eye(4)))
        ),
        "max_expansion": float(
            np.max(shadow_pairwise - pairwise)
        ),
        "collision_pairs": int(
            np.sum(shadow_pairwise[upper] < 1e-6)
        ),
        "mean_contraction": float(
            np.mean(
                shadow_pairwise[upper]
                / np.maximum(pairwise[upper], 1e-9)
            )
        ),
        "neighborhood_recall": recall(shadow_pairwise),
        "random_basis_recall": recall(placebo_pairwise),
        "observer_only": True,
    }
    gates = {
        "train_ranking": (
            metrics["train"]["pairwise_tail_ranking"] >= 0.95
        ),
        "validation_ranking": (
            metrics["validation"]["pairwise_tail_ranking"]
            >= 0.60
        ),
        "sealed_ranking": (
            metrics["sealed_test"]["pairwise_tail_ranking"]
            >= 0.60
        ),
        "cross_composition_ranking": (
            metrics["cross_composition"][
                "pairwise_tail_ranking"
            ]
            >= 0.60
        ),
        "finite": bool(np.isfinite(coordinates).all()),
        "orthonormal": (
            projection["orthonormal_error"] < 1e-5
        ),
        "nonexpansion": (
            projection["max_expansion"] < 1e-5
        ),
    }
    require(
        tuple(gates) == REQUIRED_GATES,
        "internal required-gate order changed",
    )

    geometry = {
        "node_ids": node_ids,
        "coordinates": coordinates.tolist(),
        "relations": relation_vectors.tolist(),
        "shadow": shadow.tolist(),
        "projection": projection,
    }
    geometry_path = args.out / "geometry.json"
    geometry_path.write_bytes(canonical(geometry) + b"\n")
    manifest = split_manifest(world)
    receipt = {
        "schema": 2,
        "world_sha256": digest(raw_world),
        "split_manifest_sha256": digest(
            canonical(manifest) + b"\n"
        ),
        "trainer_sha256": digest(Path(__file__).read_bytes()),
        "platform": (
            "kaggle_tpu"
            if args.require_tpu
            else "local_diagnostic"
        ),
        "devices": devices,
        "jax": jax.__version__,
        "seed": 73129,
        "steps": 3000,
        "dimensions": 48,
        "objective": (
            "train-only TFIDF projection; typed translation; "
            "train-node corrupt-tail hinge; no holdout nodes or "
            "edges in gradients"
        ),
        "vocabulary": vocabulary,
        "idf": inverse_document_frequency.tolist(),
        "training_node_ids": [
            node_ids[index] for index in train_indices
        ],
        "negative_candidate_ids": [
            node_ids[index] for index in train_indices
        ],
        "train_mechanisms": manifest["train"],
        "curve": curve,
        "metrics": metrics,
        "required_gates": list(REQUIRED_GATES),
        "gates": gates,
        "passed": all(gates.values()),
        "geometry_sha256": digest(
            geometry_path.read_bytes()
        ),
        "started": started,
        "elapsed_seconds": time.time() - started,
        "source_revision": os.environ.get(
            "LIGHTCONE_SOURCE_REVISION", "unrecorded"
        ),
        "source_dirty": os.environ.get(
            "LIGHTCONE_SOURCE_DIRTY", "unknown"
        ),
        "job": os.environ.get("LIGHTCONE_JOB", "local"),
    }
    (args.out / "training.json").write_bytes(
        canonical(receipt) + b"\n"
    )
    print(
        json.dumps(
            {
                key: receipt[key]
                for key in (
                    "platform",
                    "devices",
                    "metrics",
                    "gates",
                    "passed",
                    "elapsed_seconds",
                )
            },
            indent=2,
        )
    )
    return 0 if receipt["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
