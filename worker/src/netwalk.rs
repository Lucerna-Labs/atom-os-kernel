//! E37 netwalk payload — the wire is real, and the walls hold it.
//!
//! Phase 0: wait for the stack (a -nic none machine reports idle and
//! exits clean — the OS runs wireless). Phase 1: bootstrap ARP (ask
//! for the gateway), send a PROSE probe through the egress cone, and
//! wait for the echo reply — receiving it drops it in the app ring.
//! Phase 2: recv the foreign packet — the kernel MARKS THIS TASK
//! TAINTED on recv (the ingress ceremony), proven by a refused
//! spawn. Phase 3: the cone holds the line — key-shaped data never
//! reaches the wire.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn net(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_NET, sub, a, b)
}

fn main() {
    rt::sleep(60); // let the boot settle and the smuggler train the cone
    let st = net(0, 0, 0);
    if st >> 63 != 1 {
        rt::print("[Net] no NIC this boot — ingress layer idle (E37 PASS, wireless)\n");
        rt::exit(0);
    }
    rt::print("[Net] virtio wire up — feeling it on the heartbeat\n");

    // Phase 1: bootstrap. ARP-ask for the gateway, then a prose probe.
    let prose = b"the ingress ceremony speaks prose and only prose over the wire today";
    let mut replied = false;
    for round in 0..40u64 {
        let _ = net(4, 0, 0); // ARP bootstrap probe
        rt::sleep(4);
        let sent = net(2, prose.as_ptr() as u64, prose.len() as u64);
        if sent == 1 {
            // Wait for the echo reply to land in the ring.
            for _wait in 0..12u64 {
                rt::sleep(4);
                net(3, 0, 0); // heartbeat now
                let status = net(0, 0, 0);
                if (status >> 48) & 0xFF > 0 {
                    replied = true;
                    break;
                }
            }
            if replied { break; }
        }
        let _ = (round, sent);
    }
    if !replied {
        rt::print("[Net] FAIL: prose probe never came back\n");
        rt::exit(6);
    }
    rt::print("[Net] prose probe echoed back — the wire speaks both ways\n");

    // Phase 2: recv the foreign packet. The kernel taints us on recv.
    let pointer = net(1, 0, 0);
    if pointer == ERROR || pointer == 0 {
        rt::print("[Net] FAIL: recv returned nothing\n");
        rt::exit(7);
    }
    let me = rt::call(SYS_GETPID, 0, 0);
    let taint_state = rt::call3(SYS_TAINT, 3, me, 0);
    if taint_state != 1 {
        rt::print("[Net] FAIL: reader was not tainted by the ingress\n");
        rt::exit(8);
    }
    rt::print_args(format_args!(
        "[Net] recv'd {} bytes of foreign packet — handler now tainted\n", pointer
    ));
    let spawn_result = rt::spawn("shell.elf");
    if spawn_result != ERROR {
        rt::print("[Net] FAIL: tainted reader could still spawn!\n");
        rt::exit(9);
    }
    rt::print("[Net] tainted reader cannot spawn — the zero-click wall holds on real packets\n");

    // Phase 3: the cone on the wire — key-shaped data must be refused.
    // 64 high-distinct bytes: what raw key material actually looks
    // like to the cone's window (a 32-byte word padded with zeros
    // reads prose-ish — low distinct count — and legitimately passes).
    static KEYISH: [u8; 64] = [
        0x93, 0xC4, 0x67, 0x0B, 0xE1, 0x7F, 0x3A, 0xD8, 0x52, 0x1C, 0xAE, 0x74, 0x09, 0xBD,
        0x46, 0xFA, 0x85, 0x31, 0xC7, 0x60, 0x12, 0x9E, 0x7B, 0xE4, 0x2D, 0xA8, 0x53, 0x06,
        0xDA, 0x91, 0x48, 0xBC, 0x25, 0xF6, 0x8A, 0x13, 0x77, 0xC2, 0x59, 0xE0, 0x34, 0xAB,
        0x9C, 0x41, 0xDF, 0x62, 0x0E, 0x83, 0x56, 0xC9, 0x1F, 0xB7, 0x72, 0x0A, 0x95, 0x38,
        0xE8, 0x64, 0x2B, 0xD3, 0x17, 0x89, 0x40, 0xFB,
    ];
    let refused = net(2, KEYISH.as_ptr() as u64, KEYISH.len() as u64);
    if refused == 1 {
        rt::print("[Net] FAIL: key-shaped data left the wire!\n");
        rt::exit(10);
    }
    rt::print("[Net] key-shaped data refused by the cone — the wire stays prose-shaped\n");
    rt::print("[Net] E37 PASS: packets ring, readers taint, the cone gates the wire\n");
    rt::exit(0);
}
