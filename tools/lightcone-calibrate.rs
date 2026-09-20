//! Offline world calibration; geometry only. No packet classifiers or policy.
use kernel_lightcone::{World,Event,Lightcone,Observation,Fact};
use kernel_lightcone::training::{Sample,calibrate};
use std::{fs,path::Path};
fn load(path:&Path)->Result<Vec<Sample>,String>{
 let text=fs::read_to_string(path).map_err(|e|e.to_string())?;let mut samples=Vec::new();
 for (line,text) in text.lines().enumerate(){if text.is_empty()||text.starts_with('#'){continue;}
  let v:Vec<_>=text.split_whitespace().collect();if v.len()!=5{return Err(format!("line {}: expected direction cause_tick cause_position event_tick event_position",line+1));}
  let d=match v[0]{"in"=>0,"out"=>1,_=>return Err("bad direction".into())};
  let n:Result<Vec<u64>,_>=v[1..].iter().map(|s|s.parse()).collect();let n=n.map_err(|_|"invalid coordinate")?;
  samples.push(Sample{direction:d,cause:Event{tick:n[0],position:n[1]},event:Event{tick:n[2],position:n[3]}});
 }Ok(samples)
}
fn main(){
 let args:Vec<_>=std::env::args().collect();
 if args.len()!=6{eprintln!("usage: lightcone-calibrate TRAIN HOLDOUT WORLD_ID RING_SIZE OUTPUT");std::process::exit(2);}
 let train=load(Path::new(&args[1])).expect("reviewed training input");let held=load(Path::new(&args[2])).expect("independent held-out input");
 assert!(!held.is_empty());let id=args[3].parse().unwrap();let ring=args[4].parse().unwrap();
 let fit=calibrate(id,ring,&train).expect("calibration");let cone=Lightcone::new(fit.world);
 let mut directions=[0usize;2];
 for s in &held{directions[s.direction as usize]+=1;assert!(matches!(cone.query(Observation{world_id:id,event:s.event,cause:Some(s.cause)}),Ok(Fact::Within{..})),"held-out normal geometry outside law");}
 assert!(directions.iter().all(|&n|n>=16));
 assert_eq!(World::decode(&fit.world.encode()),Ok(fit.world));fs::write(&args[5],fit.world.encode()).unwrap();
 println!("{{\"training_samples\":{},\"kept\":{},\"trimmed\":{},\"train_in\":{},\"train_out\":{},\"holdout_samples\":{},\"holdout_in\":{},\"holdout_out\":{},\"world_id\":{},\"ring_size\":{},\"v_max\":{},\"all_holdout_constructible\":true,\"production_reliability_proven\":false}}",train.len(),fit.used,fit.trimmed,fit.directions[0],fit.directions[1],held.len(),directions[0],directions[1],id,ring,fit.world.velocity());
}
