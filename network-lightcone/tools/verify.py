#!/usr/bin/env python3
"""Independent standard-library verification of the compiled, embedded world."""
import argparse,hashlib,json,math,re,struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(Path(__file__).resolve().parent))
from train import REQUIRED_GATES,SPLITS,canonical,split_manifest,validate
def sha(b):return hashlib.sha256(b).hexdigest()
def check(test,message):
 if not test:raise ValueError(message)
def main():
 p=argparse.ArgumentParser();p.add_argument('--pack',type=Path,default=ROOT/'network-lightcone/worlds/v3-local-diagnostic');p.add_argument('--require-tpu',action='store_true');a=p.parse_args()
 m=json.loads((a.pack/'manifest.json').read_bytes())
 for name,expected in m['files'].items():check(sha((a.pack/name).read_bytes())==expected,'file hash: '+name)
 raw=(a.pack/'world.bin').read_bytes();wraw=(a.pack/'world.json').read_bytes();w=json.loads(wraw);t=json.loads((a.pack/'training.json').read_bytes());g=json.loads((a.pack/'geometry.json').read_bytes())
 validate(w)
 check(raw==(ROOT/'kernel-lightcone/src/world.bin').read_bytes(),'embedded pack mismatch')
 check(wraw==(ROOT/'network-lightcone/sources/world.json').read_bytes(),'current sources differ from pack')
 check(t['world_sha256']==sha(wraw),'training source hash')
 check(t['trainer_sha256']==sha((ROOT/'network-lightcone/tools/train.py').read_bytes()),'trainer revision')
 check(t['geometry_sha256']==m['files']['geometry.json'],'geometry receipt hash')
 check(t.get('required_gates')==list(REQUIRED_GATES),'required gate manifest')
 check(set(t.get('gates',{}))==set(REQUIRED_GATES),'named gate set')
 check(t['passed'] and all(t['gates'].get(name) is True for name in REQUIRED_GATES),'failed gate')
 check(t.get('split_manifest_sha256')==sha(canonical(split_manifest(w))+b'\n'),'split manifest hash')
 train_nodes={e[k] for e in w['edges'] if e['split']=='train' for k in ['source','target']}
 check(set(t.get('training_node_ids',[]))==train_nodes,'training node manifest')
 check(t.get('negative_candidate_ids')==t.get('training_node_ids'),'holdout nodes entered negative candidates')
 if a.require_tpu:check(t['platform']=='kaggle_tpu' and t['devices'] and all(d['platform']=='tpu' for d in t['devices']),'TPU acceptance absent')
 magic,n,e,d,version,size,wh,th=struct.unpack_from('<8sHHHHI32s32s',raw)
 check(magic==b'ATLCNET2' and (n,e,d,version)==(len(w['nodes']),len(w['edges']),48,w['version']),'pack header')
 check(wh.hex()==sha(wraw) and th.hex()==sha((a.pack/'training.json').read_bytes()),'binary provenance')
 strings=1044+n*216+e*16;check(strings+size==len(raw),'binary extent')
 ids={v['id']:i for i,v in enumerate(w['nodes'])}
 check(g['node_ids']==list(ids),'geometry node order')
 def meta(o):
  start,length=struct.unpack_from('<II',raw,o);return json.loads(raw[strings+start:strings+start+length])
 for i,node in enumerate(w['nodes']):
  off=1044+i*216;check(meta(off)==node,'node metadata');coords=struct.unpack_from('<52i',raw,off+8)
  check(list(coords)==[round(x*1_000_000) for x in g['coordinates'][i]+g['shadow'][i]],'quantized coordinates')
  check(all(math.isfinite(x) for x in g['coordinates'][i]),'nonfinite geometry')
 for i,edge in enumerate(w['edges']):
  off=1044+n*216+i*16;check(meta(off+8)==edge,'edge evidence')
  src,dst,rel,pol,split,res=struct.unpack_from('<HHBbBB',raw,off)
  check((src,dst,rel,pol,split,res)==(ids[edge['source']],ids[edge['target']],w['relations'][edge['relation']],edge['polarity'],['train','validation','sealed_test','cross_composition','regression'].index(edge['split']),0),'typed edge mapping')
 # Recompute every ranking gate from the exact emitted coordinates/relations.
 for split in SPLITS:
  ranks=[]
  for edge in w['edges']:
   if edge['split']!=split:continue
   src,dst=ids[edge['source']],ids[edge['target']];q=[a+b for a,b in zip(g['coordinates'][src],g['relations'][w['relations'][edge['relation']]])]
   dist=[sum((a-b)**2 for a,b in zip(q,z)) for z in g['coordinates']]
   excludes={src}|{ids[o['target']] for o in w['edges'] if o['source']==edge['source'] and o['relation']==edge['relation']}
   negative=[j for j in range(n) if j not in excludes];ranks.append(sum(dist[dst]<dist[j] for j in negative)/len(negative))
  score=sum(ranks)/len(ranks);check(abs(score-t['metrics'][split]['pairwise_tail_ranking'])<1e-6,'metric replay: '+split)
  if split!='regression':check(score>=(.95 if split=='train' else .60),'recomputed gate: '+split)
 print(json.dumps(dict(verified=True,platform=t['platform'],sealed=False,nodes=n,edges=e,dimensions=d,pack_sha256=sha(raw))))
if __name__=='__main__':main()
