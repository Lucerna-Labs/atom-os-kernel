use user_rt::{self as rt,abi::*};
pub fn run(){
 assert_eq!(rt::call3(SYS_LIGHTCONE,0,0,0),1);
 let mut payload=[0u8;80];payload[..4].copy_from_slice(&[10,0,2,2]);payload[4..6].copy_from_slice(&5555u16.to_be_bytes());payload[6..8].copy_from_slice(&5555u16.to_be_bytes());
 let text=b"ATLC1 1 100 10 90 2";payload[8..8+text.len()].copy_from_slice(text);
 let _=rt::call3(SYS_NET,4,0,0);rt::sleep(5);
 assert_eq!(rt::call3(SYS_NET,6,payload.as_ptr()as u64,(8+text.len())as u64),1);
 rt::print("LIGHTCONE_OUTBOUND_CLAIM_SENT\n");
 for _ in 0..1000{
  let n=rt::call3(SYS_LIGHTCONE,0,5,0);
  if n>=5{
   let mut inside=0;let mut outside=0;let mut unknown=0;let mut directions=[0;2];
   for i in 0..n{
    let d=rt::call3(SYS_LIGHTCONE,1,i,0);if d<2{directions[d as usize]+=1;}
    match rt::call3(SYS_LIGHTCONE,1,i,7){1=>inside+=1,2=>outside+=1,4=>unknown+=1,_=>{}}
   }
   assert!(inside>=2 && outside>=2 && unknown>=1 && directions[0]>=4 && directions[1]>=1);
   assert_eq!(rt::call3(SYS_LIGHTCONE,1,n,7),ERROR);
   assert_eq!(rt::call3(SYS_LIGHTCONE,9,0,0),ERROR); // no mutation surface
   rt::print_args(format_args!("LIGHTCONE_TAPS_OK in={} out={} within={} outside={} different_world={}\n",directions[0],directions[1],inside,outside,unknown));
   rt::exit(0)
  }
  rt::sleep(1);
 }
 panic!("expected inbound Lightcone claims did not arrive")
}
