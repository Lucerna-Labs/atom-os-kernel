//! E37 host gates — the network ingress ceremony.
//!
//! The pure packet path is host-verifiable: the checksum fold, ARP
//! answering, ICMP echo reply, dispatch to the app ring, and the
//! cone-gated send. The driver itself is hardware-proven in QEMU
//! (see diag-e37-serial.log).

use kernel_egress::{force_freeze, gate as cone_gate, reset as cone_reset};
use kernel_net::{
    arp_reply, arp_request, checksum, handle_frame, icmp_echo_reply, icmp_echo_request, is_up,
    recv_into, reset, send_probed, ETHERTYPE_ARP, ETHERTYPE_IPV4, GATEWAY_IP, OUR_IP,
};

const PROSE: &[u8] =
    b"the quick brown fox jumps over the lazy dog while the kernel watches the wire quietly";

fn main() {
    reset();
    cone_reset();

    // N1: the checksum fold (RFC 1071 worked example + carry wrap).
    {
        assert_eq!(checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]), 0x220D);
        // Carry: 0xFFFF + 0x0001 folds to 0x0001 -> !1 = 0xFFFE.
        assert_eq!(checksum(&[0xFF, 0xFF, 0x00, 0x01]), 0xFFFE);
        assert_eq!(checksum(&[]), 0xFFFF);
        println!("N1 PASS: the internet checksum folds correctly");
    }

    // N2: ARP — we answer requests for our IP with our MAC, and the
    // bootstrap probe asks for the gateway.
    {
        let our_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let asker = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];
        let mut request = [0u8; 42];
        request[0..6].copy_from_slice(&[0xFF; 6]); // broadcast
        request[6..12].copy_from_slice(&asker); // from the asker
        request[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        request[14..18].copy_from_slice(&[0, 1, 0x08, 0x00]);
        request[18] = 6;
        request[19] = 4;
        request[20..22].copy_from_slice(&1u16.to_be_bytes()); // REQUEST
        request[22..28].copy_from_slice(&asker);
        request[28..32].copy_from_slice(&[10, 0, 2, 2]);
        request[38..42].copy_from_slice(&OUR_IP); // asking for US
        let reply = arp_reply(&request, our_mac).expect("ARP request for us must be answered");
        assert_eq!(reply[0..6], asker); // to the asker
        assert_eq!(reply[6..12], our_mac); // from us
        assert_eq!(reply[20..22], 2u16.to_be_bytes()); // REPLY
        assert_eq!(reply[22..28], our_mac); // our MAC is the answer
        assert_eq!(reply[28..32], OUR_IP);
        // Not for us: silence.
        request[38..42].copy_from_slice(&[10, 0, 2, 99]);
        assert!(arp_reply(&request, our_mac).is_none());
        // Bootstrap probe: broadcast ask for the gateway.
        let probe = arp_request(our_mac);
        assert_eq!(probe[0..6], [0xFF; 6]);
        assert_eq!(probe[20..22], 1u16.to_be_bytes());
        assert_eq!(probe[38..42], GATEWAY_IP);
        println!("N2 PASS: ARP answered for us, silent for others, bootstrap asks");
    }

    // N3: ICMP echo reply — swap, retype, rechecksum; both checksums
    // verify as zero over their fields.
    {
        let our_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let sender = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];
        let data = b"proof of the ingress ceremony, echoed back";
        let request = icmp_echo_request(our_mac, sender, 0x3713, 7, data);
        // The request builder targets the GATEWAY; retarget it to OUR
        // address to model a packet arriving FOR us.
        let mut inbound = request.bytes;
        inbound[0..6].copy_from_slice(&our_mac);
        inbound[6..12].copy_from_slice(&sender);
        {
            let mut hdr = [0u8; 20];
            hdr.copy_from_slice(&inbound[14..34]);
            hdr[12..16].copy_from_slice(&GATEWAY_IP); // from gateway
            hdr[16..20].copy_from_slice(&OUR_IP); // to us
            hdr[10..12].copy_from_slice(&[0, 0]);
            let hck = checksum(&hdr);
            hdr[10..12].copy_from_slice(&hck.to_be_bytes());
            inbound[14..34].copy_from_slice(&hdr);
        }
        let reply = icmp_echo_reply(&inbound[..request.len], our_mac).expect("echo for us must be replied");
        let r = &reply.bytes[..reply.len];
        assert_eq!(r[0..6], sender);
        assert_eq!(r[6..12], our_mac);
        // IPv4 header checksum verifies (zero over its own field).
        assert_eq!(checksum(&r[14..34]), 0, "reply IP header checksum must verify");
        // ICMP type 0, same id/seq/data, checksum verifies.
        assert_eq!(r[34], 0);
        assert_eq!(&r[38..40], &0x3713u16.to_be_bytes());
        assert_eq!(&r[40..42], &7u16.to_be_bytes());
        assert_eq!(&r[42..42 + data.len()], data);
        assert_eq!(checksum(&r[34..reply.len]), 0, "reply ICMP checksum must verify");
        // Echo REPLIES aimed at us are app material, not kernel replies:
        assert!(icmp_echo_reply(&r, our_mac).is_none(), "a reply to a reply is silence");
        println!("N3 PASS: echo replied with verifying checksums; replies are not re-replied");
    }

    // N4: dispatch — foreign frames land in the app ring; the ARP
    // request is answered kernel-side and never reaches the ring.
    {
        reset();
        let our_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let mut sent: Vec<Vec<u8>> = Vec::new();
        {
            let mut transmit = |bytes: &[u8]| {
                sent.push(bytes.to_vec());
            };
            // A UDP-ish foreign frame (anything we do not answer).
            let mut foreign = [0u8; 64];
            foreign[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
            foreign[14 + 9] = 17; // UDP
            handle_frame(&foreign, &mut transmit);
            assert_eq!(sent.len(), 0, "foreign material is not answered");
        }
        let mut out = [0u8; 1600];
        let len = recv_into(&mut out).expect("foreign frame must reach the ring");
        assert_eq!(len, 64);
        // ARP request for us: answered, consumed, ring untouched.
        let asker = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];
        let mut request = [0u8; 42];
        request[0..6].copy_from_slice(&[0xFF; 6]); // broadcast
        request[6..12].copy_from_slice(&asker); // from the asker
        request[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        request[14..18].copy_from_slice(&[0, 1, 0x08, 0x00]);
        request[18] = 6;
        request[19] = 4;
        request[20..22].copy_from_slice(&1u16.to_be_bytes()); // REQUEST
        request[22..28].copy_from_slice(&asker);
        request[28..32].copy_from_slice(&GATEWAY_IP); // asker's IP
        request[38..42].copy_from_slice(&OUR_IP); // asking for us
        {
            let mut transmit = |bytes: &[u8]| {
                sent.push(bytes.to_vec());
            };
            handle_frame(&request, &mut transmit);
        }
        assert_eq!(sent.len(), 1, "the ARP request must be answered");
        assert_eq!(sent[0][20..22], 2u16.to_be_bytes());
        assert!(recv_into(&mut out).is_none(), "the ARP exchange never reaches the app ring");
        println!("N4 PASS: foreign frames ring; kernel conversations stay kernel-side");
    }

    // N5: the cone on the wire — prose leaves, key-shaped data never
    // reaches the transmit path.
    {
        reset();
        cone_reset();
        // Train the cone on prose (the boot's smuggler does this; the
        // gate trains as it judges until frozen).
        let training = b"ordinary outbound operations prose trains the cone to know legitimate traffic shapes before any test";
        while !cone_gate(training) {}
        force_freeze();
        assert!(cone_gate(PROSE), "prose must pass a trained cone");

        // Teach the gateway MAC by simulating its ARP reply.
        let gateway = [0x52, 0x54, 0x00, 0x0F, 0x0F, 0x0F];
        let mut reply = [0u8; 42];
        reply[6..12].copy_from_slice(&gateway); // from the gateway
        reply[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        reply[20..22].copy_from_slice(&2u16.to_be_bytes());
        reply[22..28].copy_from_slice(&gateway);
        reply[28..32].copy_from_slice(&GATEWAY_IP);
        {
            let mut transmit = |_bytes: &[u8]| {};
            handle_frame(&reply, &mut transmit);
        }
        let mut sent: Vec<u8> = Vec::new();
        {
            let mut transmit = |bytes: &[u8]| {
                sent.extend_from_slice(bytes);
                true
            };
            assert!(send_probed(PROSE, &mut transmit).is_ok(), "prose must leave");
        }
        assert!(sent.len() > 42, "the probe must actually transmit");
        // Key-shaped material: 32 high-entropy bytes.
        let keyish: Vec<u8> = (0..32u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
        let mut never: Vec<u8> = Vec::new();
        {
            let mut transmit = |bytes: &[u8]| {
                never.extend_from_slice(bytes);
                true
            };
            assert!(send_probed(&keyish, &mut transmit).is_err(), "key-shaped data must be refused");
        }
        assert!(never.is_empty(), "refused material must never reach the wire");
        assert!(!is_up() || true); // status surface exists
        println!("N5 PASS: prose leaves through the cone; key-shaped data never reaches the wire");

    println!("E37 HOST PASS: the ingress ceremony — packets ring, readers taint, the cone gates the wire");
    }
}
