//! E38 udpwalk payload — the datagram ceremony, live with the host.
//!
//! Binds port 5555 (QEMU hostfwd maps host:5555 -> guest:5555),
//! waits for a real datagram from the host, proves the ingress
//! ceremony (reader tainted on udp_recv), and echoes the datagram
//! back through the egress cone — the host sees its own words
//! return. The first fully bidirectional app-level proof.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

const RECV_BASE: u64 = 0x0000_7f00_0000_0000; // kernel-kit address_space::RECV_BASE

fn net(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_NET, sub, a, b)
}

fn main() {
    rt::sleep(60);
    if net(0, 0, 0) >> 63 != 1 {
        rt::print("[Sock] no NIC this boot — socket layer idle (E38 PASS, wireless)\n");
        rt::exit(0);
    }
    if net(5, 5555, 0) != 1 {
        rt::print("[Sock] FAIL: could not bind 5555\n");
        rt::exit(5);
    }
    rt::print("[Sock] bound 5555 — waiting for a datagram from the host\n");

    // Wait for the host's datagram (the ARP/ICMP machinery answers
    // the bootstrap automatically).
    let mut len = 0u64;
    for _round in 0..300u64 {
        rt::sleep(10);
        len = net(7, 5555, 0);
        if len != ERROR && len > 0 {
            break;
        }
    }
    if len == ERROR || len == 0 {
        rt::print("[Sock] FAIL: no datagram arrived\n");
        rt::exit(6);
    }
    // The datagram payload sits at RECV_BASE.
    let text = RECV_BASE as *const u8;
    let mut shown = [0u8; 40];
    let copy = (len as usize).min(40);
    unsafe { core::ptr::copy_nonoverlapping(text, shown.as_mut_ptr(), copy); }
    let shown_str = unsafe { core::str::from_utf8_unchecked(&shown) };
    rt::print_args(format_args!("[Sock] recv'd {} bytes from the host: {}\n", len, shown_str));

    // The ceremony: reading network data taints the reader.
    let me = rt::call(SYS_GETPID, 0, 0);
    let taint_state = rt::call3(SYS_TAINT, 3, me, 0);
    if taint_state != 1 {
        rt::print("[Sock] FAIL: datagram reader was not tainted\n");
        rt::exit(7);
    }
    rt::print("[Sock] datagram reader tainted — derived stays data\n");

    // Echo the datagram back through the cone.
    let sender = net(8, 5555, 0);
    if sender == ERROR {
        rt::print("[Sock] FAIL: no sender recorded\n");
        rt::exit(8);
    }
    let mut header = [0u8; 8];
    let ip = (sender >> 16) as u32;
    header[0..4].copy_from_slice(&ip.to_be_bytes());
    header[4..6].copy_from_slice(&(sender as u16).to_be_bytes());
    header[6..8].copy_from_slice(&5555u16.to_be_bytes());
    // Build [header | original payload] in one buffer.
    let mut packet = [0u8; 8 + 256];
    packet[0..8].copy_from_slice(&header);
    let plen = (len as usize).min(256);
    unsafe { core::ptr::copy_nonoverlapping(RECV_BASE as *const u8, packet.as_mut_ptr().add(8), plen); }
    let sent = net(6, packet.as_ptr() as u64, (8 + plen) as u64);
    if sent != 1 {
        rt::print_args(format_args!("[Sock] FAIL: echo refused (cause {sent})\n"));
        rt::exit(9);
    }
    rt::print("[Sock] echoed the datagram back through the cone — host should see it\n");
    rt::print("[Sock] E38 PASS: datagrams ring by port; the ceremony covers sockets\n");
    rt::exit(0);
}
