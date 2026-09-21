#!/usr/bin/env python3
"""Probe the current validation and promotion boundaries without changing them."""
import argparse,copy,hashlib,importlib.util,json,os,subprocess,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
spec=importlib.util.spec_from_file_location('train_under_test',ROOT/'network-lightcone/tools/train.py');trainer=importlib.util.module_from_spec(spec);spec.loader.exec_module(trainer)
w=json.loads((ROOT/'network-lightcone/sources/world.json').read_bytes());pack=ROOT/'network-lightcone/worlds/v2-local-diagnostic';checks=[]
def record(name,passed,**details):checks.append(dict(name=name,passed=bool(passed),**details));print(name+(' PASS' if passed else ' FAIL'),flush=True)
def validate_case(name,mutate,required=True):
 v=copy.deepcopy(w);mutate(v)
 try:trainer.validate(v);rejected=False;error=None
 except Exception as e:rejected=True;error=repr(e)
 record(name,rejected,owner='world_compiler',error=error,required=required)
validate_case('dangling_endpoint',lambda v:v['edges'][0].update(target='missing-node'))
validate_case('unknown_source',lambda v:v['nodes'][0].update(source_ids=['unknown-source']))
validate_case('duplicate_id',lambda v:v['nodes'].append(copy.deepcopy(v['nodes'][0])))
validate_case('empty_provenance',lambda v:v['edges'][0].update(source_ids=[]))
validate_case('forbidden_rights',lambda v:v['sources'][0].update(rights_lane='restricted',use='No training permission'))
validate_case('missing_rights',lambda v:v['sources'][0].pop('rights_lane'))
validate_case('duplicate_statement',lambda v:v['nodes'][1].update(statement=v['nodes'][0]['statement'],title=v['nodes'][0]['title']))
validate_case('missing_source_digest',lambda v:v['sources'][0].pop('sha256'))
validate_case('negative_relation_positive_polarity',lambda v:next(e for e in v['edges'] if e['relation']=='does_not_establish').update(polarity=1))
def leak(v):
 e=copy.deepcopy(v['edges'][0]);e.update(id='leaked-edge',split='sealed_test',mechanism_group='renamed-leaked-group');v['edges'].append(e)
validate_case('same_relation_in_train_and_test_with_renamed_group',leak)
# Finalizer outputs only to audit-owned locations; production files stay untouched.
final=ROOT/'network-lightcone/tools/finalize.py'
def promotion(name,optimized=False,empty_gates=False):
 directory=out/name;directory.mkdir();training=directory/'training';training.mkdir()
 for f in ['training.json','geometry.json']:(training/f).write_bytes((pack/f).read_bytes())
 if empty_gates:
  r=json.loads((training/'training.json').read_bytes());r['gates']={};r['passed']=True;(training/'training.json').write_text(json.dumps(r))
 cmd=[sys.executable]+(['-O'] if optimized else [])+[str(final),'--world',str(ROOT/'network-lightcone/sources/world.json'),'--training',str(training),'--out',str(directory/'candidate'),'--kernel',str(directory/'not-active-kernel')]
 if empty_gates:cmd+=['--diagnostic']
 run=subprocess.run(cmd,capture_output=True,text=True);(directory/'stdout.txt').write_text(run.stdout);(directory/'stderr.txt').write_text(run.stderr)
 candidate=directory/'candidate/manifest.json';manifest=json.loads(candidate.read_bytes()) if candidate.exists() else None
 record(name,run.returncode!=0 and manifest is None,owner='artifact_promotion',returncode=run.returncode,produced_status=manifest and manifest.get('status'))
promotion('cpu_cannot_promote_normally')
promotion('cpu_cannot_promote_under_python_optimization',optimized=True)
promotion('missing_named_gates_rejected',empty_gates=True)
# Independent ranking diagnostics preserve the meaning of the existing gate.
g=json.loads((pack/'geometry.json').read_bytes());ids={n['id']:i for i,n in enumerate(w['nodes'])};ranks={}
for e in w['edges']:
 src,dst=ids[e['source']],ids[e['target']];q=[x+y for x,y in zip(g['coordinates'][src],g['relations'][w['relations'][e['relation']]])];ds=[sum((x-y)**2 for x,y in zip(q,z)) for z in g['coordinates']];excluded={src}|{ids[k['target']] for k in w['edges'] if k['source']==e['source'] and k['relation']==e['relation']}
 rank=1+sum(ds[j]<=ds[dst] for j in range(len(ds)) if j not in excluded);ranks.setdefault(e['split'],[]).append(dict(edge=e['id'],rank=rank))
rank_metrics={s:dict(count=len(v),top1=sum(k['rank']==1 for k in v)/len(v),mrr=sum(1/k['rank'] for k in v)/len(v),ranks=v) for s,v in ranks.items()}
train_nodes={e[k] for e in w['edges'] if e['split']=='train' for k in ['source','target']};heldout=sorted({e[k] for e in w['edges'] if e['split']in ['validation','sealed_test','cross_composition'] for k in ['source','target']}-train_nodes)
# Read the concrete loss shape: corrupt-tail candidates include every node.
code=(ROOT/'network-lightcone/tools/train.py').read_text();all_nodes_negative='q[:,None,:]-z[None,:,:]' in code
record('evaluation_only_nodes_excluded_from_training_negatives',not heldout or not all_nodes_negative,owner='training_isolation',evaluation_only_nodes=heldout,qualification='Transductive negatives do not use held-out edge labels, but they do use held-out node descriptions; strict mechanism isolation is not established.')
result=dict(baseline_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),passed=all(x['passed'] for x in checks),checks=checks,ranking_diagnostics=rank_metrics,production_pack_sha256=hashlib.sha256((ROOT/'kernel-lightcone/src/world.bin').read_bytes()).hexdigest(),accelerator_submitted=False,sealed=False)
(out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'passed':result['passed'],'failures':[v['name'] for v in checks if not v['passed']],'ranking':{s:{k:v for k,v in v.items() if k!='ranks'} for s,v in rank_metrics.items()}},indent=2))
raise SystemExit(0 if result['passed'] else 1)
