#!/usr/bin/env python3
"""Train full geometry. CPU diagnostics and actual TPU acceptance are distinct."""
import argparse,hashlib,json,os,re,time
from pathlib import Path
import numpy as np

def canonical(x): return json.dumps(x,sort_keys=True,separators=(',',':')).encode()
def digest(b): return hashlib.sha256(b).hexdigest()
def validate(w):
 assert w['schema']==2 and w['dimensions']==48
 nodes=w['nodes']; edges=w['edges']; ids=[n['id'] for n in nodes]
 assert len(ids)==len(set(ids)) and 0<len(ids)<=128 and len(edges)<=256
 assert len({e['id'] for e in edges})==len(edges)
 sources={s['id'] for s in w['sources']};groups={}
 for n in nodes:
  assert n['source_ids'] and set(n['source_ids'])<=sources
  assert n['statement'] and n['scope'] and n['epistemic_class']
 for e in edges:
  assert e['source'] in ids and e['target'] in ids and e['relation'] in w['relations']
  assert e['conditions'] and e['exceptions'] and e['evidence'] and e['source_ids']
  assert set(e['source_ids'])<=sources and e['polarity'] in [-1,1]
  assert e['split'] in ['train','validation','sealed_test','cross_composition','regression']
  assert groups.setdefault(e['mechanism_group'],e['split'])==e['split']
 for s in ['train','validation','sealed_test','cross_composition','regression']: assert s in groups.values()
 return ids

def main():
 p=argparse.ArgumentParser();p.add_argument('--world',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--require-tpu',action='store_true');a=p.parse_args()
 a.out.mkdir(parents=True,exist_ok=False)
 raw=a.world.read_bytes();w=json.loads(raw);ids=validate(w);idx={v:i for i,v in enumerate(ids)}
 import jax,jax.numpy as jnp
 devices=[dict(platform=d.platform,kind=d.device_kind,id=d.id) for d in jax.devices()]
 if a.require_tpu and not all(d['platform']=='tpu' for d in devices): raise RuntimeError('actual TPU required')
 started=time.time();rng=np.random.default_rng(73129);n=len(ids);d=48
 # Transductive training on sourced train mechanisms only. Text representations
 # contain no held-out edges or split labels; vocabulary/IDF fitted on train nodes.
 train=[e for e in w['edges'] if e['split']=='train'];train_ids=sorted({idx[e[k]] for e in train for k in ['source','target']})
 docs=[re.findall(r'[a-z0-9]+', (x['title']+' '+x['statement']).lower()) for x in w['nodes']]
 vocab=sorted({t for i in train_ids for t in docs[i]});vid={t:i for i,t in enumerate(vocab)}
 x=np.zeros((n,len(vocab)),np.float32)
 for i,doc in enumerate(docs):
  for t in doc:
   if t in vid:x[i,vid[t]]+=1
 df=(x[train_ids]>0).sum(0);idf=np.log((1+len(train_ids))/(1+df))+1
 x*=idf;x/=np.maximum(np.linalg.norm(x,axis=1,keepdims=True),1e-6)
 # Shared text-to-coordinate map, trained with typed translation and explicit
 # polarity. Holdout nodes use the same train-fitted text transform.
 x=jnp.array(x);s=jnp.array([idx[e['source']] for e in train]);t=jnp.array([idx[e['target']] for e in train]);r=jnp.array([w['relations'][e['relation']] for e in train])
 valid=np.ones((len(train),n),np.float32)
 for row,e in enumerate(train):
  valid[row,idx[e['source']]]=0
  # No held-out graph references in optimizer construction.
 # Only true train targets may mask a training negative: held-out edges never
 # enter the training objective or negative selection.
 valid[:]=1
 for row,e in enumerate(train):
  valid[row,idx[e['source']]]=0
  for other in train:
   if other['source']==e['source'] and other['relation']==e['relation']:valid[row,idx[other['target']]]=0
 valid=jnp.array(valid)
 params=(jnp.array(rng.normal(0,.12,(len(vocab),d)),dtype=jnp.float32),jnp.array(rng.normal(0,.08,(5,d)),dtype=jnp.float32))
 def loss(params):
  z=x@params[0];rel=params[1];q=z[s]+rel[r];pos=jnp.sum((q-z[t])**2,1)
  neg=jnp.sum((q[:,None,:]-z[None,:,:])**2,2)
  hinge=jnp.sum(jnp.maximum(0,1+pos[:,None]-neg)*valid)/jnp.sum(valid)
  return hinge+.01*jnp.mean(pos)+.0001*sum(jnp.mean(v*v) for v in params)
 value_grad=jax.jit(jax.value_and_grad(loss));m=tuple(jnp.zeros_like(v) for v in params);v=m;curve=[]
 for step in range(1,3001):
  l,g=value_grad(params);m=tuple(.9*a+.1*b for a,b in zip(m,g));v=tuple(.999*a+.001*b*b for a,b in zip(v,g))
  params=tuple(a-.01*(b/(1-.9**step))/(jnp.sqrt(c/(1-.999**step))+1e-8) for a,b,c in zip(params,m,v))
  if step in [1,100,500,1000,2000,3000]:curve.append(dict(step=step,loss=float(l)))
 z=np.array(x@params[0]);rel=np.array(params[1]);assert np.isfinite(z).all()
 metrics={}
 for split in ['train','validation','sealed_test','cross_composition','regression']:
  ranks=[]
  for e in w['edges']:
   if e['split']!=split:continue
   si,ti=idx[e['source']],idx[e['target']];q=z[si]+rel[w['relations'][e['relation']]]
   ds=((z-q)**2).sum(1);excluded={si}|{idx[o['target']] for o in w['edges'] if o['source']==e['source'] and o['relation']==e['relation']}
   neg=[j for j in range(n) if j not in excluded]
   ranks.append(float((ds[ti]<ds[neg]).mean()))
  metrics[split]=dict(edges=len(ranks),pairwise_tail_ranking=float(np.mean(ranks)),worst=float(min(ranks)))
 # Shadow fitted on train-node coordinates only. No admission authority.
 center=z[train_ids].mean(0);_,_,vt=np.linalg.svd(z[train_ids]-center,full_matrices=False);basis=vt[:4].T;shadow=(z-center)@basis
 pair=np.linalg.norm(z[:,None]-z[None,:],axis=2);spair=np.linalg.norm(shadow[:,None]-shadow[None,:],axis=2);upper=np.triu_indices(n,1)
 placebo,_=np.linalg.qr(rng.normal(size=(48,4)));ppair=np.linalg.norm(((z-center)@placebo)[:,None]-((z-center)@placebo)[None,:],axis=2)
 def recall(dist):
  return float(np.mean([len(set(np.argsort(pair[i])[1:6]) & set(np.argsort(dist[i])[1:6]))/5 for i in range(n)]))
 projection=dict(center=center.tolist(),basis=basis.tolist(),dimensions='x,y,z,w (spatial)',fit_node_ids=[ids[i] for i in train_ids],orthonormal_error=float(np.max(np.abs(basis.T@basis-np.eye(4)))),max_expansion=float(np.max(spair-pair)),collision_pairs=int(np.sum(spair[upper]<1e-6)),mean_contraction=float(np.mean(spair[upper]/np.maximum(pair[upper],1e-9))),neighborhood_recall=recall(spair),random_basis_recall=recall(ppair),observer_only=True)
 gates={'train_ranking':metrics['train']['pairwise_tail_ranking']>=.95,'validation_ranking':metrics['validation']['pairwise_tail_ranking']>=.60,'sealed_ranking':metrics['sealed_test']['pairwise_tail_ranking']>=.60,'cross_composition_ranking':metrics['cross_composition']['pairwise_tail_ranking']>=.60,'finite':bool(np.isfinite(z).all()),'orthonormal':projection['orthonormal_error']<1e-5,'nonexpansion':projection['max_expansion']<1e-5}
 output=dict(node_ids=ids,coordinates=z.tolist(),relations=rel.tolist(),shadow=shadow.tolist(),projection=projection)
 (a.out/'geometry.json').write_bytes(canonical(output)+b'\n')
 receipt=dict(schema=1,world_sha256=digest(raw),trainer_sha256=digest(Path(__file__).read_bytes()),platform='kaggle_tpu' if a.require_tpu else 'local_diagnostic',devices=devices,jax=jax.__version__,seed=73129,steps=3000,dimensions=48,objective='shared train-fitted TFIDF projection; typed translation; corrupt-tail hinge; no heldout-edge gradients',vocabulary=vocab,idf=idf.tolist(),train_mechanisms=sorted({e['mechanism_group'] for e in train}),curve=curve,metrics=metrics,gates=gates,passed=all(gates.values()),geometry_sha256=digest((a.out/'geometry.json').read_bytes()),started=started,elapsed_seconds=time.time()-started,source_revision=os.environ.get('LIGHTCONE_SOURCE_REVISION','unrecorded'),source_dirty=os.environ.get('LIGHTCONE_SOURCE_DIRTY','unknown'),job=os.environ.get('LIGHTCONE_JOB','local'))
 (a.out/'training.json').write_bytes(canonical(receipt)+b'\n');print(json.dumps({k:receipt[k] for k in ['platform','devices','metrics','gates','passed','elapsed_seconds']},indent=2))
 return 0 if receipt['passed'] else 1
if __name__=='__main__':raise SystemExit(main())
