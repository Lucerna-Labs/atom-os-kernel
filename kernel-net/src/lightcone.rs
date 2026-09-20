//! Network tap around passive Atom geometry. The geometry never receives policy
//! or changes from traffic. This adapter records explicit ATLC1 causal CLAIMS;
//! untagged packets have unknown ancestry. It neither rewrites nor blocks data.
//! Claimed positions/clocks are not authenticated physical measurements.
use super::Lock;
use kernel_lightcone::{Event,Observation,World,Lightcone,Fact,GeometryError};
const WORLD_BYTES:&[u8]=include_bytes!("../../kernel-lightcone/worlds/dev-ring.bin");
const CAPACITY:usize=64;
#[derive(Clone,Copy)]
struct Record {seq:u64,direction:u64,observation:Observation}
const EMPTY:Record=Record{seq:u64::MAX,direction:0,observation:Observation{world_id:0,event:Event{tick:0,position:0},cause:None}};
struct Ledger {records:[Record;CAPACITY],next:u64,packets:[u64;2],untagged:[u64;2],malformed:u64}
static LEDGER:Lock<Ledger>=Lock::new(Ledger{records:[EMPTY;CAPACITY],next:0,packets:[0;2],untagged:[0;2],malformed:0});
fn world()->Result<World,GeometryError>{World::decode(WORLD_BYTES)}
fn number(b:&[u8])->Option<u64>{if b.is_empty(){return None;}let mut n=0u64;for &c in b{if !c.is_ascii_digit(){return None;}n=n.checked_mul(10)?.checked_add((c-b'0')as u64)?;}Some(n)}
/// Wire metadata is explicit, preserving ordinary traffic byte-for-byte:
/// ATLC1 world_id event_tick event_position cause_tick cause_position
/// or ATLC1 world_id event_tick event_position none
fn parse(b:&[u8])->Result<Option<Observation>,()>{
    if !b.starts_with(b"ATLC1 "){return Ok(None);}
    let mut words=b.split(|c|c.is_ascii_whitespace()).filter(|s|!s.is_empty());
    words.next();let id=number(words.next().ok_or(())?).ok_or(())?;
    let tick=number(words.next().ok_or(())?).ok_or(())?;let position=number(words.next().ok_or(())?).ok_or(())?;
    let first=words.next().ok_or(())?;
    let cause=if first==b"none"{None}else{Some(Event{tick:number(first).ok_or(())?,position:number(words.next().ok_or(())?).ok_or(())?})};
    if words.next().is_some(){return Err(());}Ok(Some(Observation{world_id:id,event:Event{tick,position},cause}))
}
fn payload(frame:&[u8])->Option<&[u8]>{
    if frame.len()<34 || frame[12..14]!=[8,0]{return None;}
    let ip=&frame[14..];let ihl=(ip[0]&15)as usize*4;
    if ip[0]>>4!=4 || ihl<20 || ihl>ip.len(){return None;}
    let len=u16::from_be_bytes([ip[2],ip[3]])as usize;
    if len<ihl+8 || len>ip.len() || u16::from_be_bytes([ip[6],ip[7]])&0x3fff!=0{return None;}
    let body=&ip[ihl..len];
    match ip[9]{
        17=>{let n=u16::from_be_bytes([body[4],body[5]])as usize;if n<8 || n>body.len(){None}else{Some(&body[8..n])}},
        1 if body[0]==0 || body[0]==8=>Some(&body[8..]),
        _=>None,
    }
}
/// Bookkeeping only: no containment query, security response or model update.
pub fn observe(direction:u8,frame:&[u8]){
    if direction>1{return;}let mut ledger=LEDGER.lock();let d=direction as usize;
    ledger.packets[d]=ledger.packets[d].saturating_add(1);
    let observation=match payload(frame).map(parse){
        Some(Ok(Some(o)))=>o,
        Some(Err(()))=>{ledger.malformed=ledger.malformed.saturating_add(1);return;},
        _=>{ledger.untagged[d]=ledger.untagged[d].saturating_add(1);return;}
    };
    if ledger.next==u64::MAX{return;}
    let seq=ledger.next;ledger.records[seq as usize%CAPACITY]=Record{seq,direction:direction as u64,observation};ledger.next+=1;
}
/// Read-only tap: world facts and explicit ledger coverage. Eviction never
/// turns missing ancestry into "within"; absent records return None.
pub fn status(field:u64)->Option<u64>{
    let w=world().ok()?;let l=LEDGER.lock();
    match field{0=>Some(w.id()),1=>Some(w.ring_size()),2=>Some(w.velocity()),3=>Some(l.packets[0]),4=>Some(l.packets[1]),
        5=>Some(l.next),6=>Some(l.malformed),7=>Some(l.next.saturating_sub(CAPACITY as u64)),8=>Some(l.untagged[0]),9=>Some(l.untagged[1]),_=>None}
}
pub fn record(index:u64,field:u64)->Option<u64>{
    let r={let l=LEDGER.lock();let r=l.records[index as usize%CAPACITY];if r.seq!=index || index>=l.next{return None;}r};
    let o=r.observation;
    let fact=Lightcone::new(world().ok()?).query(o);
    match field{0=>Some(r.direction),1=>Some(o.world_id),2=>Some(o.event.tick),3=>Some(o.event.position),
        4=>Some(o.cause.is_some()as u64),5=>o.cause.map(|c|c.tick),6=>o.cause.map(|c|c.position),
        7=>Some(match fact{Ok(Fact::Unspecified)=>0,Ok(Fact::Within{..})=>1,Ok(Fact::Outside{..})=>2,Err(GeometryError::InvalidPosition)=>3,Err(_)=>4}),
        8=>match fact{Ok(Fact::Within{distance,..})|Ok(Fact::Outside{distance,..})=>Some(distance),_=>None},
        9=>match fact{Ok(Fact::Outside{minimum_ticks,..})=>Some(minimum_ticks),_=>None},_=>None}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn parse_claims_and_reject_overflow(){assert!(parse(b"ordinary packet").unwrap().is_none());assert!(parse(b"ATLC1 1 20 5 10 0").unwrap().is_some());assert!(parse(b"ATLC1 1 20 5 none").unwrap().is_some());assert!(parse(b"ATLC1 1 18446744073709551616 0 none").is_err());assert!(parse(b"ATLC1 1 2 3 none extra").is_err());}
}

#[cfg(test)]mod tap_tests{
 use super::*;
 #[test]fn both_directions_are_passive_and_ledger_eviction_is_explicit(){
  let mut ledger=LEDGER.lock();*ledger=Ledger{records:[EMPTY;CAPACITY],next:0,packets:[0;2],untagged:[0;2],malformed:0};drop(ledger);
  for i in 0..80u64{
   let text=if i%2==0{b"ATLC1 1 100 10 90 2".as_slice()}else{b"ATLC1 1 100 10 99 30".as_slice()};
   let frame=crate::build_udp([1;6],[2;6],[10,0,2,2],5555,5555,text);
   let before=frame.bytes;observe((i%2)as u8,&frame.bytes[..frame.len]);assert_eq!(frame.bytes,before);
  }
  assert_eq!(status(3),Some(40));assert_eq!(status(4),Some(40));assert_eq!(status(7),Some(16));
  assert_eq!(record(0,7),None);assert_eq!(record(78,7),Some(1));assert_eq!(record(79,7),Some(2));
  assert_eq!(world().unwrap().encode(),*include_bytes!("../../kernel-lightcone/worlds/dev-ring.bin"));
 }
}
