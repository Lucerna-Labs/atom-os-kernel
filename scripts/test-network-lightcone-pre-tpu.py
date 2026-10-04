#!/usr/bin/env python3
"""Pre-TPU tests of current runtime/stack. No enforcement or trained-world edits."""
import argparse,json,re,socket,struct,sys,threading,time,traceback
from pathlib import Path
sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent
import importlib.util
spec=importlib.util.spec_from_file_location('nettest',HERE/'test-network-lightcone.py');base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
ROOT=HERE.parent

def input_frame(tag,mutate=None):
 b=bytearray(base.frame(tag.encode()));struct.pack_into('!H',b,36,9100)
 if mutate:mutate(b)
 if tag!='bad-ip-checksum':b[24:26]=b'\0\0';b[24:26]=struct.pack('!H',base.ck(b[14:34]))
 return bytes(b)
def fixtures():
 return [
 ('valid',input_frame('valid'),True),
 ('bad-ip-checksum',input_frame('bad-ip-checksum',lambda b:b.__setitem__(24,b[24]^1)),False),
 ('bad-udp-checksum',input_frame('bad-udp-checksum',lambda b:b.__setitem__(slice(40,42),b'\x12\x34')),False),
 ('ip-total-too-long',input_frame('ip-total-too-long',lambda b:struct.pack_into('!H',b,16,len(b)+100)),False),
 ('udp-length-too-long',input_frame('udp-length-too-long',lambda b:struct.pack_into('!H',b,38,400)),False),
 ('fragment-offset',input_frame('fragment-offset',lambda b:struct.pack_into('!H',b,20,16)),False),
 ('wrong-destination',input_frame('wrong-destination',lambda b:b.__setitem__(slice(30,34),bytes([10,0,2,99]))),False),
 ('short-ihl',input_frame('short-ihl',lambda b:b.__setitem__(14,0x41)),False),
 ]

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--accel',choices=['kvm','tcg'],required=True);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
 result=dict(success=False,accel=a.accel,checks={},failures=[],runtime_source='unchanged from 9223955; userspace test probes added',pack_sha256=base.sha((ROOT/'kernel-lightcone/src/world.bin').read_bytes()),image_sha256=base.sha((ROOT/'target/x86_64-os/release/bootimage-x86_64-kernel.bin').read_bytes()),sealed=False,accelerator_submitted=False)
 world=json.loads((ROOT/'network-lightcone/sources/world.json').read_bytes());sent=[];received=[];reader_errors=[];echoes=[];guest=None;conn=None;stop=threading.Event();write_lock=threading.Lock();server=socket.socket();server.bind(('127.0.0.1',0));server.listen();server.settimeout(20);started=time.monotonic()
 def check(name,value,**detail):
  result['checks'][name]=dict(passed=bool(value),**detail)
  if not value:result['failures'].append(name)
  print(name+(' PASS' if value else ' FAIL'),flush=True)
 try:
  disk=out/'data.img';base.fixture.prepare_disk(disk)
  guest=base.boot.Guest(ROOT,out/'guest',disk,a.accel,netdev=f'socket,id=lcnet,connect=127.0.0.1:{server.getsockname()[1]}');conn,_=server.accept();conn.settimeout(.2)
  def send(b):
   with write_lock:conn.sendall(struct.pack('!I',len(b))+b);sent.append(b)
  def reader():
   buffer=b''
   try:
    while not stop.is_set():
     try:b=conn.recv(65536)
     except socket.timeout:continue
     if not b:break
     buffer+=b
     while len(buffer)>=4 and len(buffer)>=4+struct.unpack_from('!I',buffer)[0]:
      n=struct.unpack_from('!I',buffer)[0];f=buffer[4:4+n];buffer=buffer[4+n:];received.append(f)
      if len(f)>=42 and f[12:14]==b'\x08\x00' and f[23]==17 and f[42:].startswith(b'soak round '):
       if base.ck(f[14:34])!=0:reader_errors.append('bad outbound IPv4 checksum')
       udp=f[34:];pseudo=f[26:34]+bytes([0,17])+struct.pack('!H',len(udp))
       if base.ck(pseudo+udp)!=0:reader_errors.append('bad outbound UDP checksum')
       echoes.append(f[42:]);send(base.frame(f[42:]))
   except Exception as e:
    if not stop.is_set():reader_errors.append(repr(e))
  thread=threading.Thread(target=reader,daemon=True);thread.start()
  guest.wait('shell: ready');guest.wait('STORAGE_READY generation=1')
  pid=int(guest.command('spawn worker.elf --lightcone-ingress-audit',r'spawned pid (\d+)')[1]);guest.wait('AUDIT_INGRESS_READY');cases=fixtures()
  for _,b,_ in cases:send(b);time.sleep(.2)
  guest.wait('AUDIT_INGRESS_DONE',seconds=90);guest.command(f'wait {pid}',rf'wait pid={pid} status=0',90)
  serial=guest.serial();delivered=re.findall(r'AUDIT_DELIVERED ([^\n]+)',serial);result['delivered_tags']=delivered
  for tag,b,should in cases:check('udp_delivery_'+tag,(tag in delivered)==should,expected_delivery=should,observed_delivery=tag in delivered)
  first=[json.loads(s) for s in re.findall(r'LIGHTCONE_RECEIPT (\{[^\n]+\})',serial)];byhash={r['query_sha256']:r for r in first}
  for tag,b,_ in cases:
   check('wire_observed_'+tag,base.sha(b) in byhash)
  result['ingress_observations']={tag:byhash.get(base.sha(b),{}).get('observation') for tag,b,_ in cases}
  # Continue after delivery failures to separate passive admission from stack defects.
  offset=len(guest.serial());pid=int(guest.command('spawn worker.elf --lightcone-soak-audit',r'spawned pid (\d+)')[1]);guest.wait('AUDIT_SOAK_DONE',offset=offset,seconds=120);guest.command(f'wait {pid}',rf'wait pid={pid} status=0',90)
  serial=guest.serial();soak=serial[offset:];rounds=[dict(round=int(i),free=int(f),info=json.loads(s)) for i,f,s in re.findall(r'AUDIT_ROUND (\d+) free=(\d+) (\{[^\n]+\})',soak)]
  check('all_sustained_rounds_complete',len(rounds)==3,rounds=rounds)
  expected_payloads={f'soak round {r} sample {s}'.encode() for r in range(3) for s in range(256)}
  check('sustained_wire_bytes_and_checksums',len(echoes)==768 and set(echoes)==expected_payloads and not reader_errors,outbound=len(echoes),echoed_inbound=len(echoes),reader_errors=reader_errors)
  ledger=re.search(r'AUDIT_LEDGER retrieved=(\d+) verified=(\d+) first=(\d+) latest=(\d+)',soak)
  counts=list(map(int,ledger.groups())) if ledger else []
  check('kernel_ledger_retains_32',bool(counts) and counts[0]==32 and counts[3]-counts[2]==31,counts=counts)
  check('ledger_summary_is_reliable',bool(counts) and counts[1]==32,counts=counts)
  check('free_frames_stable_after_warmup',len(rounds)==3 and rounds[1]['free']==rounds[2]['free'])
  wire={base.sha(b) for b in sent+received};ok=True
  for r in first:
   ok &= (r['nodes'],r['edges'],r['excluded'],r['hop_frontier'])==base.expected(world,r)
   ok &= r['receipt_sha256']==base.receipt_hash(r) and r['query_sha256'] in wire and r['pack_sha256']==result['pack_sha256'] and r['passive'] and not r['holds_verdict']
  check('observed_receipts_exact_and_immutable',ok,verified_receipts=len(first))
  guest.command('msg pre-tpu-survives',re.escape('[Daemon] Received IPC: pre-tpu-survives'),60);check('interactive_ipc_after_sustained_load',True)
  result['success']=not result['failures']
 except Exception as e:result['error']=repr(e);result['traceback']=traceback.format_exc();print('FAIL '+repr(e),flush=True)
 finally:
  stop.set()
  if guest:guest.close()
  if conn:conn.close()
  server.close();result['elapsed_seconds']=time.monotonic()-started
  (out/'wire.json').write_text(json.dumps(dict(sent=[b.hex() for b in sent],received=[b.hex() for b in received],errors=reader_errors))+'\n');(out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 return 0 if result['success'] else 1
if __name__=='__main__':raise SystemExit(main())
