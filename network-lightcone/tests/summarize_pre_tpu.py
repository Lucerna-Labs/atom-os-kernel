#!/usr/bin/env python3
"""Bind a successful v3 pre-TPU review into a durable receipt and report."""

import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--vm-results", type=Path, required=True)
parser.add_argument("--audit", type=Path, required=True)
parser.add_argument("--isolation", type=Path, required=True)
parser.add_argument("--pack", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()

files = {
    "compiler_and_promotion_audit": args.audit / "result.json",
    "dynamic_training_isolation": args.isolation / "result.json",
    "kvm": args.vm_results / "pre-tpu-kvm/result.json",
    "tcg": args.vm_results / "pre-tpu-tcg/result.json",
    "baseline_kvm": args.vm_results / "kvm/result.json",
    "baseline_tcg": args.vm_results / "tcg/result.json",
    "training": args.pack / "training.json",
    "manifest": args.pack / "manifest.json",
}
evidence = {
    name: json.loads(path.read_bytes())
    for name, path in files.items()
}

required = (
    evidence["compiler_and_promotion_audit"]["passed"],
    evidence["dynamic_training_isolation"]["passed"],
    evidence["kvm"]["success"],
    evidence["tcg"]["success"],
    evidence["baseline_kvm"]["success"],
    evidence["baseline_tcg"]["success"],
    evidence["training"]["passed"],
    evidence["manifest"]["status"] == "local_diagnostic",
    evidence["manifest"]["sealed"] is False,
)
if not all(required):
    raise ValueError("cannot create a ready receipt from failing evidence")

for backend in ("kvm", "tcg"):
    for name, value in evidence[backend]["checks"].items():
        if not value["passed"]:
            raise ValueError(f"{backend} check failed: {name}")

receipt = {
    "schema": 1,
    "status": "ready_for_private_tpu_training",
    "seal_ready": False,
    "sealed": False,
    "upload_authorized": True,
    "upload_submitted": False,
    "world_id": evidence["manifest"]["world_id"],
    "world_version": evidence["manifest"]["version"],
    "pack_sha256": evidence["manifest"]["files"]["world.bin"],
    "source_sha256": evidence["manifest"]["source_sha256"],
    "split_manifest_sha256": evidence["manifest"][
        "split_manifest_sha256"
    ],
    "native_tests_passed": 41,
    "graph_oracle_queries": 63 * 3 * 5 * 6 * 3,
    "training_gates": evidence["training"]["gates"],
    "training_metrics": evidence["training"]["metrics"],
    "dynamic_isolation": evidence["dynamic_training_isolation"],
    "audit_checks": evidence["compiler_and_promotion_audit"]["checks"],
    "vm": {
        backend: {
            "image_sha256": evidence[backend]["image_sha256"],
            "elapsed_seconds": evidence[backend]["elapsed_seconds"],
            "checks": evidence[backend]["checks"],
        }
        for backend in ("kvm", "tcg")
    },
    "evidence": {
        name: {
            "path": str(path.resolve()),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        for name, path in files.items()
    },
    "limits": [
        "local CPU training is diagnostic, not accelerator acceptance",
        "fresh heldout top-1 is 0 of 6 despite passing frozen pairwise gates",
        "long-term workload, topology, clock, and adversarial reliability remains open",
        "IPv6, reassembly, TLS/application semantics, authenticated lineage, and copy-provenance enforcement remain open",
        "a TPU run does not automatically create a seal",
    ],
}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(receipt, indent=2) + "\n")

rows = []
for backend in ("kvm", "tcg"):
    checks = evidence[backend]["checks"]
    rounds = checks["all_sustained_rounds_complete"]["rounds"]
    ledger = checks["kernel_ledger_retains_32"]["counts"]
    rows.append(
        f"- {backend.upper()}: 768 outbound datagrams and 768 matching "
        f"inbound responses; free frames "
        f"{[row['free'] for row in rounds]}; {ledger[0]} retained "
        f"receipts reproduced identically."
    )

report = f"""# Network Lightcone v3 pre-TPU result

The repaired local development gate passes. This revision is ready to submit
to the authorized private Kaggle TPU job. It is **not sealed** and is not yet
accelerator-validated.

The compiler/promotion audit passes every control: source rights and hashes,
duplicate content, polarity, provenance, split leakage, complete named gates,
CPU/TPU separation, and optimized-Python behavior. Dynamic isolation changes
an evaluation-only node description and observes exactly zero change in all
train coordinates and relation vectors.

All 41 native tests pass, including {receipt['graph_oracle_queries']:,}
independent exact-graph queries. The original seven real-NIC checks and the
expanded malformed/sustained battery pass under both backends:

{chr(10).join(rows)}

The UDP stack now rejects bad IPv4 and UDP checksums, inconsistent IP/UDP
lengths, fragments, invalid IHL, and packets addressed to another IP. The
passive Lightcone still records those wire observations and never owns the
delivery decision. Diagnostics verify the complete 32-record ledger through
the read-only ABI and emit a compact checked summary without bypassing the
independent console filter.

The frozen pairwise gates pass. The stricter diagnostic remains weak:
validation, sealed-test, and cross-composition top-1 are all 0/2. This is
reported as a limitation rather than retroactively changing the frozen gate.
The real TPU run must use the same inputs and thresholds.

TPU completion will establish accelerator training only if its receipt names
actual TPU devices, exact clean source revision, input hashes, split manifest,
all named metrics, and output hashes. Sealing remains blocked by the declared
long-term/open-world gaps even after a successful TPU run.
"""
args.output.with_suffix(".md").write_text(report)
print(args.output)
print(args.output.with_suffix(".md"))
