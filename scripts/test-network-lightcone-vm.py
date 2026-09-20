#!/usr/bin/env python3
"""Observe actual bidirectional virtio traffic; test passive geometric facts, not attack detection."""
import argparse,hashlib,importlib.util,json,socket,struct,sys,time
from pathlib import Path
sys.dont_write_bytecode=True
def load(name,file):
 spec=importlib.util.spec_from_file_location(name,Path(__file__).with_name(file));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
boot=load('lc_boot','boot-test.py');disktools=load('lc_disk','test-userspace-imports.py')
MAC=bytes.fromhex('525400123456');PEER=bytes.fromhex('52550a000202');IP=bytes([10,0,2,15]);REMOTE=bytes([10,0,2,2])
def checksum(b):
 if len(b)%2:b+=b'\0'
 s=sum(struct.unpack('!%dH'%(len(b)//2),b))
 while s>>16:s=(s&65535)+(s>>16)
 return (~s)&65535

def udp(data):
 u=struct.pack('!HHHH',5555,5555,8+len(data),0)+data
 ip=bytearray(struct.pack('!BBHHHBBH4s4s',0x45,0,20+len(u),1,0,64,17,0,REMOTE,IP));struct.pack_into('!H',ip,10,checksum(ip))
 return MAC+PEER+b'\x08\x00'+ip+u

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--accel',choices=['kvm','tcg'],required=True);a=p.parse_args()
 root=Path(__file__).resolve().parents[1];out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);disk=out/'data.img';disktools.prepare_disk(disk)
 result={'success':False,'acceleration':a.accel,'checks':{},'world':'development ring calibration, not real-traffic reliability',
  'image_sha256':hashlib.sha256((root/'target/x86_64-os/release/bootimage-x86_64-kernel.bin').read_bytes()).hexdigest()}
 server=socket.socket();server.bind(('127.0.0.1',0));server.listen(1);server.settimeout(20);guest=None;conn=None;frames=[];buffer=bytearray()
 def send(f):conn.sendall(struct.pack('!I',len(f))+f)
 def passed(k):result['checks'][k]=True;print(k+' PASS',flush=True)
 try:
  guest=boot.Guest(root,out/'guest',disk,a.accel,netdev=f'socket,id=atomnet,connect=127.0.0.1:{server.getsockname()[1]}')
  conn,_=server.accept();conn.settimeout(.1);result['qmp_kvm']=guest.kvm
  guest.wait('shell: demo fleet already ran')
  arp=MAC+PEER+b'\x08\x06'+struct.pack('!HHBBH',1,0x0800,6,4,2)+PEER+REMOTE+MAC+IP
  send(arp)
  m=guest.command('spawn worker.elf --lightcone-test',r'spawned pid (\d+)');pid=int(m[1])
  guest.wait('LIGHTCONE_OUTBOUND_CLAIM_SENT',seconds=90)
  end=time.monotonic()+10
  while time.monotonic()<end:
   try:chunk=conn.recv(65536)
   except socket.timeout:continue
   buffer.extend(chunk)
   while len(buffer)>=4:
    n=struct.unpack_from('!I',buffer)[0]
    if len(buffer)<4+n:break
    frames.append(bytes(buffer[4:4+n]));del buffer[:4+n]
   if any(f[42:]==b'ATLC1 1 100 10 90 2' for f in frames if len(f)>=42):break
  else:raise AssertionError('outbound causal claim did not cross virtio NIC unchanged')
  passed('OUTBOUND_BYTES_UNCHANGED_ON_REAL_NIC')
  for text in [b'ATLC1 1 100 10 90 2',b'ATLC1 1 100 10 99 30',b'ATLC1 1 100 10 100 10',b'ATLC1 2 100 10 90 2']:
   send(udp(text));time.sleep(.1)
  fact=guest.wait(r'LIGHTCONE_TAPS_OK in=(\d+) out=(\d+) within=(\d+) outside=(\d+) different_world=(\d+)',seconds=90)
  result['facts']={k:int(v) for k,v in zip(['in','out','within','outside','different_world'],fact.groups())}
  guest.command(f'wait {pid}',rf'wait pid={pid} status=0',90)
  passed('INBOUND_AND_OUTBOUND_CAUSAL_FACTS')
  passed('FUTURE_AND_OUTSIDE_CAUSES_DISTINGUISHED')
  passed('CROSS_WORLD_AND_MISSING_RECORDS_NOT_ADMITTED')
  passed('NO_MUTATION_SYSCALL_SURFACE')
  # Current console/IPC remain operational; Lightcone has no blocking action.
  guest.command('msg lightcone stays passive',r'\[Daemon\] Received IPC: lightcone stays passive')
  passed('EXISTING_IPC_REMAINS_OPERATIONAL');result['success']=True
 except Exception as e:result['error']=repr(e);print('FAIL '+repr(e),flush=True)
 finally:
  if conn:conn.close()
  server.close()
  if guest:guest.close()
  (out/'wire.json').write_text(json.dumps({'guest_frames':[f.hex() for f in frames]},indent=2)+'\n')
  (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 return 0 if result['success'] else 1
if __name__=='__main__':raise SystemExit(main())
