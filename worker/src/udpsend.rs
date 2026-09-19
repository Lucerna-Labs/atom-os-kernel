//! udpsend — a message to the host, through the cone.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn net(sub: u64, a: u64, b: u64) -> u64 {
    rt::call3(SYS_NET, sub, a, b)
}

fn udp_send(packet: &[u8; 8 + 256], len: usize) -> u64 {
    net(6, packet.as_ptr() as u64, (8 + len) as u64)
}

fn main() {
    let argv = rt::args();
    if argv.len() < 2 {
        rt::print("usage: udpsend <message>\n");
        rt::exit(1);
    }
    // [dest IPv4 be][dest port be][src port be][payload] — 10.0.2.2:5555 is the QEMU gateway.
    let mut packet = [0u8; 8 + 256];
    packet[0..4].copy_from_slice(&[10, 0, 2, 2]);
    packet[4..6].copy_from_slice(&5555u16.to_be_bytes());
    packet[6..8].copy_from_slice(&5555u16.to_be_bytes());
    // argv[0] is the program name; the message is the words after it, joined with single spaces.
    let mut len = 0usize;
    for (index, word) in argv.iter().skip(1).enumerate() {
        if len == 256 { break; }
        if index > 0 { packet[8 + len] = b' '; len += 1; }
        let take = (256 - len).min(word.len());
        packet[8 + len..8 + len + take].copy_from_slice(&word.as_bytes()[..take]);
        len += take;
    }
    if net(0, 0, 0) >> 63 != 1 {
        rt::print("udpsend: no wire on this machine\n");
        rt::exit(0);
    }
    let _ = net(4, 0, 0); // ARP bootstrap probe
    rt::sleep(6);
    let mut cause = udp_send(&packet, len);
    if cause == 3 {
        let _ = net(4, 0, 0); // no route yet: re-bootstrap and send once more
        rt::sleep(6);
        cause = udp_send(&packet, len);
        if cause != 1 {
            rt::print("udpsend: no route\n");
            rt::exit(5);
        }
    }
    match cause {
        1 => {
            rt::print_args(format_args!("sent {len} bytes to 10.0.2.2:5555\n"));
            rt::exit(0);
        }
        2 => { rt::print("udpsend: the cone refused this message (key-shaped?)\n"); rt::exit(4); }
        _ => { rt::print("udpsend: driver error\n"); rt::exit(6); }
    }
}
