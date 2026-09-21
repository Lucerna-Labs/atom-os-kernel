#!/usr/bin/env python3
"""Create a private TPU payload bound to the current clean Git revision."""

import argparse
import base64
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument(
    "--job-id",
    default="jessealicea/atom-os-network-lightcone-v3",
)
args = parser.parse_args()
if args.output.exists():
    raise FileExistsError(f"refusing to overwrite {args.output}")
status = subprocess.check_output(
    ["git", "status", "--porcelain"], cwd=ROOT, text=True
)
if status:
    raise ValueError("Kaggle payload requires a clean Git revision")
revision = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
).strip()

inputs = {
    "world.json": ROOT / "network-lightcone/sources/world.json",
    "train.py": ROOT / "network-lightcone/tools/train.py",
}
for relative, path in inputs.items():
    committed = subprocess.check_output(
        ["git", "show", f"HEAD:{path.relative_to(ROOT)}"],
        cwd=ROOT,
    )
    if committed != path.read_bytes():
        raise ValueError(f"{relative} differs from committed source")

args.output.mkdir(parents=True)
program = [
    "import base64,os,subprocess,sys",
    "from pathlib import Path",
    "root=Path('/kaggle/working/atom-network')",
    "root.mkdir(exist_ok=True)",
]
for name, path in inputs.items():
    encoded = base64.b64encode(path.read_bytes()).decode()
    program.append(
        f"(root/{name!r}).write_bytes(base64.b64decode({encoded!r}))"
    )
program.extend(
    [
        f"os.environ['LIGHTCONE_SOURCE_REVISION']={revision!r}",
        "os.environ['LIGHTCONE_SOURCE_DIRTY']='false'",
        f"os.environ['LIGHTCONE_JOB']={args.job_id!r}",
        "subprocess.run([sys.executable,str(root/'train.py'),"
        "'--world',str(root/'world.json'),'--out',"
        "str(root/'trained'),'--require-tpu'],check=True)",
    ]
)
(args.output / "run.py").write_text("\n".join(program) + "\n")
metadata = {
    "id": args.job_id,
    "title": "Atom OS Network Lightcone v3",
    "code_file": "run.py",
    "language": "python",
    "kernel_type": "script",
    "is_private": True,
    "enable_gpu": False,
    "enable_tpu": True,
    "enable_internet": False,
    "dataset_sources": [],
    "competition_sources": [],
    "kernel_sources": [],
}
(args.output / "kernel-metadata.json").write_text(
    json.dumps(metadata, indent=2) + "\n"
)
manifest = {
    "destination": f"Private Kaggle job {args.job_id}",
    "source_revision": revision,
    "contents": (
        "Trainer source and sourced typed network world only. "
        "No packet captures, private user data, credentials, or "
        "sibling trained artifacts."
    ),
    "inputs": {
        name: {
            "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        for name, path in inputs.items()
    },
}
manifest["payload"] = {
    path.name: {
        "bytes": path.stat().st_size,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }
    for path in sorted(args.output.glob("*"))
}
(args.output / "UPLOAD-MANIFEST.json").write_text(
    json.dumps(manifest, indent=2) + "\n"
)
print(json.dumps(manifest, indent=2))
