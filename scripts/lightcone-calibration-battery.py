#!/usr/bin/env python3
"""Reproducible geometric calibration controls. Generated hops are NOT real traffic measurements."""
import argparse,hashlib,json,random,subprocess
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def generate(path,n,seed,offset):
 rng=random.Random(seed)
 with path.open('x') as f:
  f.write('# GENERATED declared-ring reference world; not a captured network\n')
  for i in range(n):
   d=rng.randrange(1,17);position=rng.randrange(64);elapsed=d*rng.choice([2,3]);tick=offset+i*100
   f.write(f'{"in" if i%2==0 else "out"} {tick} {position} {tick+elapsed} {(position+d)%64}\n')
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
 a.output.mkdir(parents=True,exist_ok=False);root=Path(__file__).resolve().parents[1]
 train=a.output/'train.tsv';held=a.output/'heldout.tsv';world=a.output/'dev-ring.bin'
 generate(train,100000,137,0);generate(held,50000,971,1000000000)
 result=subprocess.run(['bash',str(root/'scripts/train-network-lightcone.sh'),str(train),str(held),'1','64',str(world)],text=True,capture_output=True)
 (a.output/'calibration.log').write_text(result.stdout+result.stderr);result.check_returncode()
 report=json.loads(result.stdout.splitlines()[-1]);assert world.read_bytes()==(root/'kernel-lightcone/worlds/dev-ring.bin').read_bytes()
 report.update({'source_kind':'generated geometric control, not production traffic','seed_train':137,'seed_holdout':971,
  'train_sha256':sha(train),'holdout_sha256':sha(held),'world_sha256':sha(world),'geometry_source_sha256':sha(root/'kernel-lightcone/src/lib.rs'),
  'operator_policy_trained':False,'traffic_thresholds_trained':False,'real_network_calibration_complete':False})
 (a.output/'receipt.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
