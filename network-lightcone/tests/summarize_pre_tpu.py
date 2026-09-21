#!/usr/bin/env python3
"""Bind the pre-TPU audit evidence without changing earlier receipts or the pack."""
import argparse,hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--vm-results',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
files={
 'audit':ROOT/'test-results/pre-tpu-audit-20260921/result.json',
 'replay':ROOT/'test-results/pre-tpu-replay-20260921/result.json',
 'isolation':ROOT/'test-results/pre-tpu-isolation-20260921/result.json',
 'kvm':a.vm_results/'pre-tpu-kvm/result.json',
 'tcg':a.vm_results/'pre-tpu-tcg/result.json',
 'baseline_kvm':a.vm_results/'kvm/result.json',
 'baseline_tcg':a.vm_results/'tcg/result.json',
}
r={name:json.loads(path.read_bytes()) for name,path in files.items()}
assert r['baseline_kvm']['success'] and r['baseline_tcg']['success'] and r['replay']['passed']
for name in ['kvm','tcg']:
 assert not r[name].get('error'),r[name]
 assert r[name]['checks']['all_sustained_rounds_complete']['passed']
 assert r[name]['checks']['kernel_ledger_retains_32']['passed']
assert subprocess.run(['git','diff','--exit-code','9223955','--','kernel-lightcone','kernel-net','kernel-orchestrator','user-rt','kernel-egress','network-lightcone/tools','network-lightcone/sources','network-lightcone/worlds'],cwd=ROOT,capture_output=True).returncode==0
result=dict(baseline='9223955321899966be5ed3b3a318926382e505f9',pre_tpu_ready=False,upload_authorized=True,upload_submitted=False,sealed=False,production_runtime_and_world_unchanged=True,production_pack_sha256=r['audit']['production_pack_sha256'],evidence={name:dict(path=str(path.resolve()),sha256=hashlib.sha256(path.read_bytes()).hexdigest()) for name,path in files.items()},results=r)
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n')
rows=[]
for name in ['kvm','tcg']:
 c=r[name]['checks'];rounds=c['all_sustained_rounds_complete']['rounds'];counts=c['kernel_ledger_retains_32']['counts']
 rows.append(f"- {name.upper()}: 768 outbound datagrams and 768 matching inbound responses; three rounds completed; physical free frames {[x['free'] for x in rounds]}; {counts[0]} ledger records readable, {counts[1]} printed, {counts[2]} rejected by the existing console gate.")
text='''# Pre-TPU test result — not ready

Tested the existing implementation at commit `9223955`. Production Lightcone,
packet processing, trainer, sources and embedded world were unchanged; added
userspace probes and independent test drivers only. No TPU job was submitted
and no seal was created. The user authorized the prepared upload conditional
on testing first; the remaining blocker is these test failures, not permission.

## What works

- Current pack hashes and source/trainer identities verify.
- Two fresh CPU runs reproduce the exact original learned geometry hash.
- All 40 existing native tests pass, including 14,850 independent graph-oracle
  queries. All seven original NIC checks pass on each of KVM and TCG.
- Sustained actual virtio traffic preserves bytes/checksums, keeps the world
  unchanged, reports zero admission failures and retains interactive IPC.
'''+ '\n'.join(rows)+'''

## Blocking findings

1. **Malformed UDP reaches an application.** On both accelerators the existing
   UDP delivery path accepted bad IPv4 checksums, bad UDP checksums, truncated
   IP total lengths, truncated UDP lengths, nonzero fragment offsets, and a
   different destination IP. Lightcone correctly records checksum/length faults
   and fragment context, but does not own delivery authority. Fix the protocol
   validation at `kernel-net/src/lib.rs::parse_udp` and its receiving boundary.
   The valid control is delivered and invalid short IHL is rejected.
2. **Artifact promotion checks can disappear.** Running the existing finalizer
   with `python -O` labels a CPU result `accelerator_validated`, because its
   acceptance checks use Python assertions. A diagnostic promotion also accepts
   an empty required-gate map. These negative-control artifacts exist only in
   the audit scratch folder and were never embedded. Replace optional assertions
   with explicit checks and require every named gate and provenance field.
3. **Training/validation isolation is incomplete.** Evaluation-only node text
   enters the global corrupt-tail candidate matrix. Altering just the held-out
   `ip_df` description changes train-node coordinates by a maximum 0.11464824.
   This does not demonstrate use of held-out edge labels; it does disprove strict
   isolation of held-out mechanism descriptions. Duplicate training relations
   also pass validation when copied into a test split under a new group name.
4. **Receipt display is unreliable after warm-up.** The kernel ledger still
   contains all 32 retained records. The existing console egress filter rejects
   some complete JSON receipts containing hashes, and the current printing API
   ignores the error. The refined probe records syscall success/failure rather
   than bypassing or disabling that filter. Fix diagnostic delivery explicitly.
5. **Compiler preflight is incomplete.** Restricted/missing source rights,
   missing source digests, duplicate statement content and conflicting
   relation/polarity combinations are accepted. Dangling endpoints, unknown
   source IDs, duplicate IDs and empty edge provenance are rejected correctly.

The independent audit passes 5/14 checks. The original pairwise ranking gates
still pass, but filtered top-1 retrieval gets only 2 of the 6 fresh held-out
relationships right (validation 0/2, sealed-test 1/2, cross-composition 1/2).
That is a separate diagnostic, not a changed acceptance threshold or attack
recognition accuracy. Six examples cannot establish broad generalization.

## Evidence handling and next gate

The initial sustained probe inferred missing ledger records from missing console
lines. Its failed receipt remains preserved at
`test-results/network-world-20260921T170500038893532Z`. The follow-up separately
counts successful ledger reads and rejected console writes, locating the failure
at the output boundary. Earlier results have not been rewritten.

Repair the validation/promotion boundary, isolate training splits, validate
network delivery, and provide reliable authorized diagnostic output. Rerun these
same local controls before submitting a corrected version to TPU. Keep this
world unsealed; the long-term coverage/reliability gates remain open.

Reproduce the independent audit (expected to fail on the current implementation):

```sh
python3 network-lightcone/tests/pre_tpu_audit.py --output test-results/NEW_AUDIT
ATOM_PRE_TPU=1 bash scripts/vm-test-network-lightcone.sh
```

Machine-readable results and evidence hashes are in `pre-tpu-20260921.json`.
'''
a.output.with_suffix('.md').write_text(text)
print(a.output);print(a.output.with_suffix('.md'))
