#![no_std]
#![no_main]
user_rt::entry!(main);
fn main() {
    user_rt::print("FAULT_PROBE_START\n");
    unsafe { core::ptr::write_volatile(0xffff_ffff_800f_7000 as *mut u8, 1); }
    user_rt::exit(99)
}
