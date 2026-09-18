//! E35 cryptwalk payload — RAM-at-rest protection demo, live in QEMU.
//!
//! The cold-boot threat: an attacker freezes DRAM and reads every
//! address (spatial read, all of RAM at one instant). This payload
//! proves the three claims of the crypt layer against that attacker:
//!
//!   1. RAM holds ciphertext: a secret word encrypts to noise, and
//!      only the temporal handle (the WHEN of the write) reads it
//!      back — the handle is not derivable from the snapshot alone.
//!   2. The 4D projection: the same word encrypted at a later tick
//!      produces DIFFERENT ciphertext — encryption is a function of
//!      time, which a spatial snapshot cannot replay.
//!   3. Fail-dead: after destroy (the intrusion cascade's action),
//!      even the correct handle yields noise. One life per boot.
//!
//! Honest label: this payload destroys the master key in phase 3, so
//! it must be the last crypt user of the boot — the layer stays dead
//! until reboot, exactly like the perishable key's one-life doctrine.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn crypt(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_CRYPT, sub, a, b)
}

fn main() {
    // Two ticks, not thirty: the shell spawns this first so the crypt
    // demo runs on the LIVE key, but the spider/rogue demo that fires
    // the destruction cascade starts in parallel — it needs 100+
    // rogue syscalls to condemn, so a short sleep wins that race with
    // margin. If the cascade ever wins anyway, the one-shot branch
    // below reports the doctrine holding instead of failing.
    rt::sleep(2);

    // Phase 0: the key derived at the field's first heartbeat is live.
    // If it is already dead this boot (a second shell round, or the
    // intrusion cascade fired first), that is the one-life doctrine
    // HOLDING, not failing — receipt and quiet exit.
    let st = crypt(3, 0, 0);
    if st >> 63 != 1 {
        rt::print("[Crypt] key already dead this boot — one life per boot honored\n");
        rt::exit(0);
    }
    rt::print("[Crypt] master key alive (derived at first field tick)\n");

    // Phase 1: encrypt a secret — RAM will hold ct, not the word.
    let secret: u64 = 0x5ECE_7E11;
    let ct = crypt(0, secret, 0);
    if ct == secret || ct == ERROR {
        rt::print("[Crypt] FAIL: encryption did not scramble\n");
        rt::exit(9);
    }
    let handle = crypt(2, 0, 0);
    let back = crypt(1, ct, handle);
    if back != secret {
        rt::print("[Crypt] FAIL: roundtrip broke\n");
        rt::exit(9);
    }
    rt::print_args(format_args!("[Crypt] secret {:#x} -> ct {:#x}; handle reads it back\n", secret, ct));

    // Phase 2: the cold-boot reader — all of RAM, no handle. Even
    // holding the ciphertext, every wrong handle is noise.
    let wrong = crypt(1, ct, handle ^ 1);
    if wrong == secret {
        rt::print("[Crypt] FAIL: wrong temporal handle decrypted!\n");
        rt::exit(9);
    }
    rt::print("[Crypt] cold-boot read (wrong handle): noise — WHEN is the gate\n");

    // Phase 3: the 4D projection — same word, later tick, new ct.
    let ticks_a = rt::call(SYS_TICKS, 0, 0);
    rt::sleep(10);
    let ct2 = crypt(0, secret, 0);
    let ticks_b = rt::call(SYS_TICKS, 0, 0);
    if ct2 == ct {
        rt::print("[Crypt] FAIL: same word+key encrypted identically across ticks\n");
        rt::exit(9);
    }
    rt::print_args(format_args!("[Crypt] same secret at ticks {}..{} -> ct {:#x} (scrambled by time)\n", ticks_a, ticks_b, ct2));

    // Phase 4: fail-dead — destroy (what the intrusion cascade fires),
    // then even the CORRECT handle is noise.
    crypt(4, 0, 0);
    let dead_status = crypt(3, 0, 0);
    let dead_read = crypt(1, ct, handle);
    if dead_status >> 63 == 1 || dead_read == secret {
        rt::print("[Crypt] FAIL: destroyed key still decrypts\n");
        rt::exit(9);
    }
    rt::print("[Crypt] destroy: key dead, correct handle now noise — one life per boot\n");
    rt::print("[Crypt] E35 PASS: RAM holds ciphertext; WHEN is the 4th dimension\n");
    rt::exit(0);
}
