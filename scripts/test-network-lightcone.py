#!/usr/bin/env python3
"""Actual virtio NIC and exact receipt verification on an isolated Ethernet link."""
import argparse,hashlib,importlib.util,json,re,socket,struct,sys,threading,time
from pathlib import Path
sys.dont_write_bytecode=True
def module(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
HERE=Path(__file__).resolve().parent
boot=module('boot',HERE/'boot-test.py');fixture=module('fixture',HERE/'test-userspace-imports.py')
def sha(b):return hashlib.sha256(b).hexdigest()
def ck(b):
 if len(b)%2:b+=b'\0'
 s=sum(struct.unpack('!%dH'%(len(b)//2),b))
 while s>>16:s=(s&65535)+(s>>16)
 return (~s)&65535
def frame(payload=b'hello',eth=0x800,fragment=0,bad=False):
 src=bytes([10,0,2,2]);dst=bytes([10,0,2,15]);data=struct.pack('!HHHH',9000,9001,8+len(payload),0)+payload
 ip=bytearray(struct.pack('!BBHHHBBH4s4s',0x45,0,20+len(data)+(10 if bad else 0),7,fragment,64,17,0,src,dst));ip[10:12]=struct.pack('!H',ck(ip))
 return bytes.fromhex('525400123456525400123402')+struct.pack('!H',eth)+ip+data

def expected(world,record):
 nodes=world['nodes'];edges=world['edges'];ids={v['id']:i for i,v in enumerate(nodes)};admitted=set(record['seeds']);queue=[(i,0) for i in sorted(admitted)];excluded=set();hopfront=set()
 for current,hop in queue:
  for e in edges:
   a,b=ids[e['source']],ids[e['target']]
   other=b if a==current else a if b==current else None
   if other is None or other in admitted:continue
   if hop>=3:hopfront.add(other);continue
   if len(admitted)>=32:excluded.add(other);continue
   admitted.add(other);queue.append((other,hop+1))
 em=[i for i,e in enumerate(edges) if ids[e['source']] in admitted and ids[e['target']] in admitted]
 return sorted(admitted),em,sorted(excluded-admitted),sorted(hopfront-admitted)
def receipt_hash(r):
 b=b'ATOM-NETWORK-ADMISSION-2'+bytes.fromhex(r['pack_sha256'])+bytes.fromhex(r['query_sha256'])+bytes([2,3,31,0])+struct.pack('<H',32)
 for name,count in [('seeds',2),('nodes',2),('edges',4),('excluded',2),('hop_frontier',2)]:
  vals=[0]*count
  for i in r[name]:vals[i//64]|=1<<(i%64)
  b+=struct.pack('<'+'Q'*count,*vals)
 return sha(b)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--accel',choices=['kvm','tcg'],required=True);a=p.parse_args();root=HERE.parent;out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
 result=dict(success=False,acceleration=a.accel,checks={},pack_sha256=sha((root/'kernel-lightcone/src/world.bin').read_bytes()),image_sha256=sha((root/'target/x86_64-os/release/bootimage-x86_64-kernel.bin').read_bytes()))
 world=json.loads((root/'network-lightcone/sources/world.json').read_bytes());sent=[];received=[];errors=[];guest=None;conn=None;stop=threading.Event();server=socket.socket();server.bind(('127.0.0.1',0));server.listen();server.settimeout(20)
 def passed(name):result['checks'][name]=True;print(name+' PASS',flush=True)
 try:
  disk=out/'data.img';fixture.prepare_disk(disk)
  guest=boot.Guest(root,out/'guest',disk,a.accel,netdev=f'socket,id=lcnet,connect=127.0.0.1:{server.getsockname()[1]}');conn,_=server.accept();conn.settimeout(.2)
  def send(b):conn.sendall(struct.pack('!I',len(b))+b);sent.append(b)
  def reader():
   buf=b''
   try:
    while not stop.is_set():
     try:b=conn.recv(65536)
     except socket.timeout:continue
     if not b:break
     buf+=b
     while len(buf)>=4 and len(buf)>=4+struct.unpack('!I',buf[:4])[0]:
      n=struct.unpack('!I',buf[:4])[0];received.append(buf[4:4+n]);buf=buf[4+n:]
   except Exception as e:
    if not stop.is_set():errors.append(str(e))
  thread=threading.Thread(target=reader,daemon=True);thread.start()
  guest.wait('shell: demo fleet already ran');guest.wait(r'STORAGE_READY generation=\d+')
  match=guest.command('spawn worker.elf --lightcone-test',r'spawned pid (\d+)');pid=int(match[1]);guest.wait('LIGHTCONE_READY')
  fixtures=[frame(b'hello'),frame(b'ATLC1 node=export authorize=true'),frame(b'unknown',eth=0x88b5),frame(b'fragment',fragment=0x2000),frame(b'bad-length',bad=True)]
  for f in fixtures:send(f);time.sleep(.15)
  guest.wait('LIGHTCONE_KERNEL_OK',seconds=90);guest.command(f'wait {pid}',rf'wait pid={pid} status=0',90)
  serial=guest.serial();infos=[json.loads(s) for s in re.findall(r'LIGHTCONE_INFO (\{[^\n]+\})',serial)];records=[json.loads(s) for s in re.findall(r'LIGHTCONE_RECEIPT (\{[^\n]+\})',serial)]
  info=infos[-1];assert info['pack_sha256']==result['pack_sha256'] and info['failures']==0 and info['dimensions']==48;passed('PACK_IDENTITY_AND_READONLY_WORLD')
  assert {r['direction'] for r in records}=={'in','out'};passed('REAL_VIRTIO_BOTH_DIRECTIONS')
  byquery={r['query_sha256']:r for r in records};assert all(sha(f) in byquery for f in fixtures)
  assert byquery[sha(fixtures[0])]['seeds']==byquery[sha(fixtures[1])]['seeds'];assert byquery[sha(fixtures[2])]['seeds']==[];assert byquery[sha(fixtures[3])]['observation']==3;assert byquery[sha(fixtures[4])]['observation']==2;passed('UNKNOWN_FRAGMENT_MALFORMED_AND_PAYLOAD_ISOLATION')
  assert any(b.endswith(b'causal network probe') for b in received);assert all(bytes.fromhex(r['query_sha256']) for r in records)
  wirehash={sha(b) for b in sent+received};assert all(r['query_sha256'] in wirehash for r in records);passed('UNMODIFIED_WIRE_BYTES_BOUND_TO_RECEIPTS')
  for r in records:
   assert (r['nodes'],r['edges'],r['excluded'],r['hop_frontier'])==expected(world,r)
   assert r['receipt_sha256']==receipt_hash(r) and r['passive'] and not r['holds_verdict']
  passed('EXACT_TYPED_GRAPH_AND_INDEPENDENT_RECEIPT_HASH')
  meta={int(sub):json.loads(value) for sub,value in re.findall(r'LIGHTCONE_METADATA ([234]) (\{[^\n]+\})',serial)}
  assert meta[2]==world['nodes'][0] and meta[3]==world['edges'][0] and len(meta[4]['full'])==48 and len(meta[4]['observer_xyzw'])==4;passed('PROVENANCE_AND_FULL_GEOMETRY_FROM_KERNEL')
  guest.command('msg lightcone-regression',re.escape('[Daemon] Received IPC: lightcone-regression'),60);passed('EXISTING_IPC_SURVIVES')
  assert not errors;result.update(success=True,records=records,world_info=info)
 except Exception as e:import traceback; result['error']=repr(e); result['traceback']=traceback.format_exc(); print('FAIL '+repr(e),flush=True)
 finally:
  stop.set()
  if guest:guest.close()
  if conn:conn.close()
  server.close();(out/'wire.json').write_text(json.dumps(dict(sent=[x.hex() for x in sent],received=[x.hex() for x in received],errors=errors),indent=2)+'\n');(out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 return 0 if result['success'] else 1
if __name__=='__main__':raise SystemExit(main())
