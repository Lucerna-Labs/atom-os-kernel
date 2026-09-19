//! E38 host gates — the datagram layer over the ceremony wire.
//!
//! Pure packet path verified here: UDP parse (slirp-shaped frames),
//! pseudo-header checksums, build/verify roundtrip, listener demux,
//! and the cone on datagram DATA (never the addressing header).

use kernel_egress::{force_freeze, gate as cone_gate, reset as cone_reset};
use kernel_net::{
    bind, build_udp, handle_frame, parse_udp, reset, udp_checksum, udp_last_sender, udp_recv_into,
    udp_send, GATEWAY_IP, OUR_IP, PROTO_UDP,
};

fn train_cone() {
    cone_reset();
    let training = b"ordinary outbound operations prose trains the cone to know legitimate traffic shapes before any test";
    while !cone_gate(training) {}
    force_freeze();
}

fn main() {
    reset();
    train_cone();

    // U1: build -> parse roundtrip with verifying checksums.
    let our_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let next_mac = [0x52, 0x54, 0x00, 0x0F, 0x0F, 0x0F];
    let data = b"the datagram carries prose across the wire and back";
    let frame = build_udp(our_mac, next_mac, GATEWAY_IP, 5555, 5555, data);
    {
        let bytes = &frame.bytes[..frame.len];
        // Retarget to us (build targets the gateway).
        let mut inbound = [0u8; 1600];
        inbound[..frame.len].copy_from_slice(bytes);
        inbound[0..6].copy_from_slice(&our_mac);
        inbound[6..12].copy_from_slice(&next_mac);
        {
            let ip = &mut inbound[14..34];
            let mut hdr = [0u8; 20];
            hdr.copy_from_slice(&ip);
            // swap src/dst by rebuilding: src=Gateway, dst=Us
            hdr[12..16].copy_from_slice(&GATEWAY_IP);
            hdr[16..20].copy_from_slice(&OUR_IP);
            hdr[10..12].copy_from_slice(&[0, 0]);
            let hck = kernel_net::checksum(&hdr);
            hdr[10..12].copy_from_slice(&hck.to_be_bytes());
            ip.copy_from_slice(&hdr);
        }
        let dg = parse_udp(&inbound[..frame.len]).expect("built frame must parse");
        assert_eq!(dg.src_port, 5555);
        assert_eq!(dg.dst_port, 5555);
        assert_eq!(dg.data, data);
        // UDP checksum verifies over pseudo-header (zero over field).
        let udp = &inbound[14 + 20..frame.len];
        let scratch_sum = {
            let mut s = udp.to_vec();
            s[6..8].copy_from_slice(&[0, 0]);
            udp_checksum(GATEWAY_IP, OUR_IP, &s)
        };
        assert_eq!(udp[6..8], scratch_sum.to_be_bytes(), "built checksum must verify");
        assert_eq!(inbound[14 + 9], PROTO_UDP);
        println!("U1 PASS: build -> parse roundtrip, checksums verify");
    }

    // U2: demux — a bound port consumes its datagrams; an unbound
    // port's frames fall through to the raw foreign ring.
    reset();
    train_cone();
    assert!(bind(5555));
    {
        let frame = build_udp(our_mac, next_mac, GATEWAY_IP, 5555, 5555, b"for the listener");
        let mut inbound = [0u8; 1600];
        inbound[..frame.len].copy_from_slice(&frame.bytes[..frame.len]);
        inbound[0..6].copy_from_slice(&our_mac);
        {
            let ip = &mut inbound[14..34];
            let mut hdr = [0u8; 20];
            hdr.copy_from_slice(&ip);
            hdr[12..16].copy_from_slice(&GATEWAY_IP);
            hdr[16..20].copy_from_slice(&OUR_IP);
            hdr[10..12].copy_from_slice(&[0, 0]);
            let hck = kernel_net::checksum(&hdr);
            hdr[10..12].copy_from_slice(&hck.to_be_bytes());
            ip.copy_from_slice(&hdr);
        }
        let mut transmit = |_bytes: &[u8]| {};
        handle_frame(&inbound[..frame.len], &mut transmit);
        let mut out = [0u8; 512];
        let len = udp_recv_into(5555, &mut out).expect("listener must hold the datagram");
        assert_eq!(&out[..len], b"for the listener");
        let (sender_ip, sender_port) = udp_last_sender(5555).expect("sender recorded");
        assert_eq!(sender_ip, GATEWAY_IP);
        assert_eq!(sender_port, 5555);
        // Unbound port: falls to the raw ring (foreign material).
        let mut other = build_udp(our_mac, next_mac, GATEWAY_IP, 9999, 5555, b"nobody home");
        other.bytes[14 + 22..14 + 24].copy_from_slice(&9999u16.to_be_bytes()); // dst port
        let mut transmit2 = |_bytes: &[u8]| {};
        handle_frame(&other.bytes[..other.len], &mut transmit2);
        let mut raw = [0u8; 1600];
        let rlen = kernel_net::recv_into(&mut raw).expect("unbound frame reaches the raw ring");
        assert!(rlen > 0);
        println!("U2 PASS: bound ports consume; unbound frames stay foreign");
    }

    // U3: the cone judges DATA — prose leaves, key-shaped never
    // reaches the transmit path; the addressing header is not judged.
    reset();
    train_cone();
    kernel_net::set_mac(our_mac);
    // Learn the gateway MAC (simulated ARP reply).
    let mut reply = [0u8; 42];
    reply[6..12].copy_from_slice(&next_mac);
    reply[12..14].copy_from_slice(&kernel_net::ETHERTYPE_ARP.to_be_bytes());
    reply[20..22].copy_from_slice(&2u16.to_be_bytes());
    reply[22..28].copy_from_slice(&next_mac);
    reply[28..32].copy_from_slice(&GATEWAY_IP);
    {
        let mut t = |_b: &[u8]| {};
        handle_frame(&reply, &mut t);
    }
    let prose = b"a datagram of ordinary words for the log and nothing else at all";
    let mut sent: Vec<u8> = Vec::new();
    {
        let mut transmit = |bytes: &[u8]| {
            sent.extend_from_slice(bytes);
            true
        };
        assert!(udp_send(GATEWAY_IP, 5555, 5555, prose, &mut transmit).is_ok());
    }
    assert!(sent.len() > 42, "the datagram must actually transmit");
    let keyish: Vec<u8> = (0..64u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    let mut never: Vec<u8> = Vec::new();
    {
        let mut transmit = |bytes: &[u8]| {
            never.extend_from_slice(bytes);
            true
        };
        assert!(udp_send(GATEWAY_IP, 5555, 5555, &keyish, &mut transmit).is_err());
    }
    assert!(never.is_empty(), "refused data must never reach the wire");
    println!("U3 PASS: the cone judges datagram data; prose leaves, keys never do");

    println!("E38 HOST PASS: datagrams ring by port; the ceremony covers the socket layer");
}
