#![no_std]
#![no_main]
// A long-lived process for exercising ps and kill. It never exits on its own.
user_rt::entry!(main);
fn main() {
    user_rt::print("SLEEPER_START\n");
    loop { user_rt::sleep(50); }
}
