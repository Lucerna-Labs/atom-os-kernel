//! Atom Lightcone: passive causal geometry, not traffic judgment.
//! Mechanism adapted from E8 and imperium-net-mesh/src/lightcone.rs.
//! The trained world is the only dependency. No policy, score, learning-on-wire,
//! permission, classifier, or packet-blocking interface exists here.
#![cfg_attr(not(any(test, feature="std")), no_std)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event { pub tick:u64, pub position:u64 }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum GeometryError { InvalidWorld, InvalidPosition, DifferentWorld }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct World { id:u64, ring:u64, velocity:u64 }
impl World {
    pub const fn new(id:u64, ring:u64, velocity:u64)->Result<Self,GeometryError> {
        if id==0 || ring<2 || velocity==0 {Err(GeometryError::InvalidWorld)} else {Ok(Self{id,ring,velocity})}
    }
    pub const fn id(&self)->u64{self.id}
    pub const fn ring_size(&self)->u64{self.ring}
    pub const fn velocity(&self)->u64{self.velocity}
    pub fn distance(&self,a:u64,b:u64)->Result<u64,GeometryError>{
        if a>=self.ring || b>=self.ring{return Err(GeometryError::InvalidPosition);}
        let direct=a.abs_diff(b);Ok(direct.min(self.ring-direct))
    }
    pub fn contains(&self,event:Event,cause:Event)->Result<bool,GeometryError>{
        let d=self.distance(event.position,cause.position)?;
        if cause.tick>=event.tick{return Ok(false);}
        // Full-width multiplication: overflow never changes the reachable set.
        Ok(d as u128<=self.velocity as u128*(event.tick-cause.tick)as u128)
    }
    pub fn min_ticks(&self,distance:u64)->u64{
        distance/self.velocity+u64::from(distance%self.velocity!=0)
    }
    pub fn encode(&self)->[u8;32]{
        let mut b=[0;32];b[..8].copy_from_slice(b"ATLCW001");b[8..16].copy_from_slice(&self.id.to_le_bytes());
        b[16..24].copy_from_slice(&self.ring.to_le_bytes());b[24..32].copy_from_slice(&self.velocity.to_le_bytes());b
    }
    pub fn decode(b:&[u8])->Result<Self,GeometryError>{
        if b.len()!=32 || &b[..8]!=b"ATLCW001" {return Err(GeometryError::InvalidWorld);}
        Self::new(u64::from_le_bytes(b[8..16].try_into().unwrap()),u64::from_le_bytes(b[16..24].try_into().unwrap()),u64::from_le_bytes(b[24..32].try_into().unwrap()))
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Observation {pub world_id:u64,pub event:Event,pub cause:Option<Event>}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Fact {
    Unspecified,
    Within {distance:u64,elapsed:u64},
    Outside {distance:u64,elapsed:u64,minimum_ticks:u64},
}
/// Immutable geometry. Queries never change the world or learn from claims.
pub struct Lightcone {world:World}
impl Lightcone {
    pub const fn new(world:World)->Self{Self{world}}
    pub const fn world(&self)->World{self.world}
    pub fn query(&self,observation:Observation)->Result<Fact,GeometryError>{
        if observation.world_id!=self.world.id{return Err(GeometryError::DifferentWorld);}
        self.world.distance(observation.event.position,observation.event.position)?;
        let Some(cause)=observation.cause else{return Ok(Fact::Unspecified);};
        let distance=self.world.distance(observation.event.position,cause.position)?;
        let elapsed=observation.event.tick.saturating_sub(cause.tick);
        if self.world.contains(observation.event,cause)?{Ok(Fact::Within{distance,elapsed})}
        else{Ok(Fact::Outside{distance,elapsed,minimum_ticks:self.world.min_ticks(distance).max(1)})}
    }
}
#[cfg(feature="std")]
pub mod training {
    use super::*;
    /// Calibration samples are independently reviewed causal hops. Direction
    /// records coverage; it is not a different threat policy or geometry.
    #[derive(Clone,Copy)]
    pub struct Sample {pub direction:u8,pub cause:Event,pub event:Event}
    pub struct Calibration {pub world:World,pub used:usize,pub trimmed:usize,pub directions:[usize;2]}
    fn median(a:&mut [f64])->f64{a.sort_by(f64::total_cmp);let n=a.len();if n%2==0{(a[n/2-1]+a[n/2])/2.0}else{a[n/2]}}
    /// Same median+8*MAD trim and 1.5 propagation safety factor as Imperium.
    /// This estimates a declared law; statistics alone cannot prove a physical
    /// propagation bound or authenticate the supplied coordinates and clocks.
    pub fn calibrate(id:u64,ring:u64,samples:&[Sample])->Result<Calibration,&'static str>{
        if samples.len()<32{return Err("at least 16 reviewed samples per direction required");}
        let base=World::new(id,ring,1).map_err(|_|"invalid world")?;
        let mut rates=std::vec::Vec::with_capacity(samples.len());let mut directions=[0;2];
        for s in samples{
            if s.direction>1 || s.cause.tick>=s.event.tick{return Err("invalid direction or elapsed time");}
            let distance=base.distance(s.event.position,s.cause.position).map_err(|_|"invalid position")?;
            if distance==0{return Err("calibration requires positive-distance hops");}
            directions[s.direction as usize]+=1;
            rates.push(distance as f64/(s.event.tick-s.cause.tick)as f64);
        }
        if directions.iter().any(|&n|n<16){return Err("both directions require coverage");}
        let center=median(&mut rates.clone());let mut deviations:std::vec::Vec<_>=rates.iter().map(|v|(v-center).abs()).collect();
        let cut=center+8.0*median(&mut deviations);let kept:std::vec::Vec<_>=rates.iter().copied().filter(|&v|v<=cut).collect();
        if kept.len()<samples.len()/2{return Err("insufficient trusted calibration support");}
        let fastest=kept.iter().copied().fold(0.0,f64::max);
        let velocity=(fastest*1.5).ceil().max(1.0)as u64;
        Ok(Calibration{world:World::new(id,ring,velocity).map_err(|_|"invalid fitted law")?,used:kept.len(),trimmed:samples.len()-kept.len(),directions})
    }
}
#[cfg(test)]
mod tests {
 use super::*;
 fn world()->World{World::new(1,64,2).unwrap()}
 #[test]fn exact_reachability_and_wrapping(){let w=world();assert_eq!(w.contains(Event{tick:100,position:10},Event{tick:96,position:2}),Ok(true));assert_eq!(w.contains(Event{tick:100,position:10},Event{tick:96,position:1}),Ok(false));assert_eq!(w.distance(1,63),Ok(2));}
 #[test]fn same_tick_future_and_invalid_coordinates(){let w=world();for t in [100,101]{assert_eq!(w.contains(Event{tick:100,position:10},Event{tick:t,position:10}),Ok(false));}assert_eq!(w.distance(64,0),Err(GeometryError::InvalidPosition));assert!(World::new(1,0,1).is_err());assert!(World::new(1,64,0).is_err());}
 #[test]fn arithmetic_extremes_do_not_overflow(){let w=World::new(1,u64::MAX,u64::MAX).unwrap();assert_eq!(w.contains(Event{tick:u64::MAX,position:u64::MAX-1},Event{tick:0,position:0}),Ok(true));assert_eq!(w.min_ticks(u64::MAX),1);}
 #[test]fn world_round_trip_and_sealed_queries(){let w=world();assert_eq!(World::decode(&w.encode()),Ok(w));let cone=Lightcone::new(w);for _ in 0..10000{let _=cone.query(Observation{world_id:1,event:Event{tick:5,position:16},cause:Some(Event{tick:4,position:0})});}assert_eq!(cone.world().encode(),w.encode());assert!(World::decode(&w.encode()[..31]).is_err());}
 #[test]fn unknowns_are_not_threat_judgments(){let c=Lightcone::new(world());assert_eq!(c.query(Observation{world_id:1,event:Event{tick:1,position:0},cause:None}),Ok(Fact::Unspecified));assert_eq!(c.query(Observation{world_id:2,event:Event{tick:1,position:0},cause:None}),Err(GeometryError::DifferentWorld));}
 #[test]fn exhaustive_small_world_matches_independent_graph_reach(){for ring in 2..=17u64{for velocity in 1..=3{let w=World::new(1,ring,velocity).unwrap();for from in 0..ring{let mut reachable=std::vec![false;ring as usize];reachable[from as usize]=true;for dt in 1..=8{for _ in 0..velocity{let old=reachable.clone();for i in 0..ring as usize{reachable[i]=old[i]||old[(i+1)%ring as usize]||old[(i+ring as usize-1)%ring as usize];}}for to in 0..ring{assert_eq!(w.contains(Event{tick:dt,position:to},Event{tick:0,position:from}).unwrap(),reachable[to as usize]);}}}}}}
}

#[cfg(all(test,feature="std"))]
mod calibration_tests{
 use super::{*,training::*};
 #[test]fn calibrated_geometry_has_heldout_and_poison_controls(){
  let mut samples=std::vec::Vec::new();
  for i in 0..1000u64{let d=1+i%16;samples.push(Sample{direction:(i%2)as u8,cause:Event{tick:0,position:0},event:Event{tick:d*(2+i%2),position:d}});}
  let clean=calibrate(1,64,&samples).unwrap();assert_eq!(clean.world.velocity(),1);
  for i in 0..40{samples.push(Sample{direction:(i%2)as u8,cause:Event{tick:0,position:0},event:Event{tick:1,position:32}});}
  let contaminated=calibrate(1,64,&samples).unwrap();assert_eq!(contaminated.world,clean.world);assert_eq!(contaminated.trimmed,40);
  let mut wrong_law_accepts=0;
  for d in 2..=30{assert!(!clean.world.contains(Event{tick:d-1,position:d},Event{tick:0,position:0}).unwrap());
   if World::new(1,64,2).unwrap().contains(Event{tick:d-1,position:d},Event{tick:0,position:0}).unwrap(){wrong_law_accepts+=1;}}
  assert!(wrong_law_accepts>0); // control demonstrates the declared geometry matters
 }
 #[test]fn invalid_or_one_direction_training_is_not_a_world(){
  let s=Sample{direction:0,cause:Event{tick:0,position:0},event:Event{tick:2,position:1}};
  assert!(calibrate(1,64,&[s;32]).is_err());assert!(calibrate(1,64,&[]).is_err());
  let mut samples=[s;32];for x in &mut samples[16..]{x.direction=1;}
  assert!(calibrate(1,64,&samples).is_ok());samples[0].event.tick=0;assert!(calibrate(1,64,&samples).is_err());
 }
}
