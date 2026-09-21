//! kernel-net — E37: the network as the ingress ceremony.
//!
//! The wire is the zero-click seam made real: packets arrive
//! unbidden (nobody clicked), so the kernel treats every received
//! byte as foreign by nature. This crate owns that contract:
//!
//! - **Inbound**: the driver is polled on the kernel's heartbeat
//!   (timer tick) — packets exist when the web looks. ARP requests
//!   are answered and ICMP echoes replied IN THE KERNEL (the OS
//!   must speak to be spoken to); everything else lands in the app
//!   packet ring. SYS_NET_RECV hands a packet to a task AND MARKS
//!   THE TASK TAINTED — E34's doctrine, now with a real source:
//!   "in a real system, opening a network resource would do this
//!   automatically." It does.
//! - **Outbound**: SYS_NET_SEND passes through the E24 egress cone
//!   (prose passes, key-shaped data does not) and leaves as an ICMP
//!   echo request — the network is the cone's real channel.
//!
//! Pure packet functions (parse/build/checksum) are host-testable;
//! the driver and rings are kernel-side. no_std + atomics + the
//! house spin lock, like the senses beside it.

#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(any(test, feature = "std"))]
extern crate std;

pub mod lightcone;

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// Ethernet etherTypes we speak.
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

/// Slirp user-network constants (QEMU -netdev user): guest
/// 10.0.2.15/24, gateway/host 10.0.2.2.
pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];
pub const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];

/// App packet ring: fixed slots, the house pattern.
pub const RING_SLOTS: usize = 8;
pub const PACKET_BYTES: usize = 1600;

/// Internet checksum fold (RFC 1071): sum 16-bit words + carry,
/// ones-complement. House style: a fold.
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut words = data.chunks_exact(2);
    for word in &mut words {
        sum = sum.wrapping_add(u16::from_be_bytes([word[0], word[1]]) as u32);
    }
    for byte in words.remainder() {
        sum = sum.wrapping_add((*byte as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

/// Ethernet frame header (14 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EthHeader {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
}

pub fn parse_eth(frame: &[u8]) -> Option<(EthHeader, &[u8])> {
    if frame.len() < 14 {
        return None;
    }
    let mut dst = [0u8; 6];
    let mut src = [0u8; 6];
    dst.copy_from_slice(&frame[0..6]);
    src.copy_from_slice(&frame[6..12]);
    Some((
        EthHeader {
            dst,
            src,
            ethertype: u16::from_be_bytes([frame[12], frame[13]]),
        },
        &frame[14..],
    ))
}

/// Build an ARP reply for an ARP request aimed at our IP. Returns
/// None when the payload is not an IPv4-Ethernet ARP request for us.
pub fn arp_reply(packet: &[u8], our_mac: [u8; 6]) -> Option<[u8; 42]> {
    // ARP frame = eth header (14) + 28 bytes of ARP.
    if packet.len() < 14 + 28 {
        return None;
    }
    let arp = &packet[14..];
    let htype = u16::from_be_bytes([arp[0], arp[1]]);
    let ptype = u16::from_be_bytes([arp[2], arp[3]]);
    let oper = u16::from_be_bytes([arp[6], arp[7]]);
    if htype != 1 || ptype != ETHERTYPE_IPV4 || oper != 1 {
        return None;
    }
    let mut target_ip = [0u8; 4];
    target_ip.copy_from_slice(&arp[24..28]);
    if target_ip != OUR_IP {
        return None;
    }
    let (_, _) = parse_eth(packet)?;
    let (header, _) = parse_eth(packet)?;
    let mut out = [0u8; 42];
    out[0..6].copy_from_slice(&header.src); // to the asker
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    out[14..18].copy_from_slice(&[0, 1, 0x08, 0x00]); // htype, ptype
    out[18] = 6; // hlen
    out[19] = 4; // plen
    out[20..22].copy_from_slice(&2u16.to_be_bytes()); // REPLY
    out[22..28].copy_from_slice(&our_mac);
    out[28..32].copy_from_slice(&OUR_IP);
    out[32..38].copy_from_slice(&header.src);
    out[38..42].copy_from_slice(&target_ip);
    Some(out)
}

/// Build an ICMP echo reply for an echo request aimed at our IP.
/// Returns None when the packet is not IPv4/ICMP echo for us.
pub fn icmp_echo_reply(packet: &[u8], our_mac: [u8; 6]) -> Option<Vec2> {
    let _ = our_mac;
    let (header, payload) = parse_eth(packet)?;
    if header.ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    if payload.len() < 20 + 8 {
        return None;
    }
    if payload[0] >> 4 != 4 {
        return None;
    }
    let ihl = (payload[0] & 0xF) as usize * 4;
    if ihl < 20 || payload.len() < ihl + 8 {
        return None;
    }
    let protocol = payload[9];
    if protocol != 1 {
        return None;
    }
    let mut dst_ip = [0u8; 4];
    dst_ip.copy_from_slice(&payload[16..20]);
    if dst_ip != OUR_IP {
        return None;
    }
    let icmp = &payload[ihl..];
    if icmp[0] != 8 {
        return None;
    } // echo REQUEST only
    let total_len = u16::from_be_bytes([payload[2], payload[3]]) as usize;
    let icmp_len = total_len.saturating_sub(ihl).min(icmp.len());
    if icmp_len < 8 {
        return None;
    }

    // Reply = swap MACs, swap IPs, type 0, recompute ICMP checksum.
    let mut out_len = 14 + total_len;
    if out_len > PACKET_BYTES {
        out_len = PACKET_BYTES;
    }
    let mut out = [0u8; PACKET_BYTES];
    out[0..6].copy_from_slice(&header.src);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    // IPv4 header: copy, swap addresses, recompute header checksum.
    out[14..14 + ihl].copy_from_slice(&payload[..ihl]);
    out[14 + 12..14 + 16].copy_from_slice(&dst_ip_swapped(&payload));
    out[14 + 10..14 + 12].copy_from_slice(&[0, 0]);
    let hck = {
        let header_bytes = &out[14..14 + ihl];
        checksum(header_bytes)
    };
    out[14 + 10..14 + 12].copy_from_slice(&hck.to_be_bytes());
    // ICMP: type 0, zero checksum, then copy, then checksum.
    let icmp_out_len = icmp_len.min(out_len - 14 - ihl);
    out[14 + ihl] = 0;
    out[14 + ihl + 1..14 + ihl + 2].copy_from_slice(&icmp[1..2]);
    out[14 + ihl + 2..14 + ihl + 4].copy_from_slice(&[0, 0]);
    let copy = icmp_len.min(icmp_out_len).saturating_sub(4);
    if copy > 0 {
        out[14 + ihl + 4..14 + ihl + 4 + copy].copy_from_slice(&icmp[4..4 + copy]);
    }
    let ick = checksum(&out[14 + ihl..14 + ihl + icmp_out_len]);
    out[14 + ihl + 2..14 + ihl + 4].copy_from_slice(&ick.to_be_bytes());
    Some(Vec2 {
        bytes: out,
        len: out_len,
    })
}

/// Source IP of the IPv4 payload becomes the reply's destination.
fn dst_ip_swapped(payload: &[u8]) -> [u8; 4] {
    let mut src = [0u8; 4];
    src.copy_from_slice(&payload[12..16]);
    src
}

/// A fixed-capacity owned buffer — no Vec in no_std without alloc
/// plumbing; the rings are static anyway, and this return type keeps
/// the pure builders host-testable.
pub struct Vec2 {
    pub bytes: [u8; PACKET_BYTES],
    pub len: usize,
}

/// Build an IPv4 ICMP echo REQUEST to the gateway carrying `data`
/// (the cone has ALREADY approved the bytes — this function is the
/// post-cone path and never sees refused material).
pub fn icmp_echo_request(
    our_mac: [u8; 6],
    gateway_mac: [u8; 6],
    id: u16,
    seq: u16,
    data: &[u8],
) -> Vec2 {
    let icmp_len = 8 + data.len();
    let total = 20 + icmp_len;
    let mut out = [0u8; PACKET_BYTES];
    out[0..6].copy_from_slice(&gateway_mac);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    {
        let ip = &mut out[14..14 + 20];
        ip[0] = 0x45;
        ip[1] = 0;
        ip[2..4].copy_from_slice(&((total as u16).to_be_bytes()));
        ip[4..6].copy_from_slice(&[0x37, 0x13]); // demutable id
        ip[6..8].copy_from_slice(&[0, 0]); // no flags, no frag
        ip[8] = 64;
        ip[9] = 1; // ICMP
        ip[10..12].copy_from_slice(&[0, 0]);
        ip[12..16].copy_from_slice(&OUR_IP);
        ip[16..20].copy_from_slice(&GATEWAY_IP);
    }
    let hck = checksum(&out[14..14 + 20]);
    out[14 + 10..14 + 12].copy_from_slice(&hck.to_be_bytes());
    {
        let icmp = &mut out[14 + 20..14 + total];
        icmp[0] = 8; // echo REQUEST
        icmp[1] = 0;
        icmp[2..4].copy_from_slice(&[0, 0]);
        icmp[4..6].copy_from_slice(&id.to_be_bytes());
        icmp[6..8].copy_from_slice(&seq.to_be_bytes());
        icmp[8..].copy_from_slice(data);
    }
    let ick = checksum(&out[14 + 20..14 + total]);
    out[14 + 20 + 2..14 + 20 + 4].copy_from_slice(&ick.to_be_bytes());
    Vec2 {
        bytes: out,
        len: 14 + total,
    }
}

/// ---------------------------------------------------------------------------
/// UDP — the datagram layer over the ceremony wire (E38).
/// ---------------------------------------------------------------------------

pub const PROTO_UDP: u8 = 17;
/// Per-listener datagram capacity.
pub const DG_SLOTS: usize = 4;
pub const DG_BYTES: usize = 512;
/// Fixed listener table — the house pattern (ports earn a slot).
pub const LISTENERS: usize = 8;

/// A parsed inbound datagram: addresses/ports plus the payload.
pub struct UdpIn<'a> {
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub data: &'a [u8],
}

pub fn parse_udp(frame: &[u8]) -> Option<UdpIn<'_>> {
    let (header, payload) = parse_eth(frame)?;
    if header.ethertype != ETHERTYPE_IPV4 || payload.len() < 20 {
        return None;
    }
    if payload[0] >> 4 != 4 {
        return None;
    }
    let ihl = (payload[0] & 0xF) as usize * 4;
    if ihl < 20 || payload.len() < ihl + 8 {
        return None;
    }
    let total = u16::from_be_bytes([payload[2], payload[3]]) as usize;
    if total < ihl + 8 || total > payload.len() {
        return None;
    }
    if checksum(&payload[..ihl]) != 0 {
        return None;
    }
    if u16::from_be_bytes([payload[6], payload[7]]) & 0x3FFF != 0 {
        return None;
    }
    if payload[9] != PROTO_UDP || payload[16..20] != OUR_IP {
        return None;
    }
    let udp = &payload[ihl..total];
    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || udp_len != udp.len() || udp_len > DG_BYTES + 8 {
        return None;
    }
    let mut src_ip = [0u8; 4];
    src_ip.copy_from_slice(&payload[12..16]);
    let mut dst_ip = [0u8; 4];
    dst_ip.copy_from_slice(&payload[16..20]);
    let transmitted_checksum = u16::from_be_bytes([udp[6], udp[7]]);
    if transmitted_checksum != 0 && udp_checksum(src_ip, dst_ip, udp) != 0 {
        return None;
    }
    Some(UdpIn {
        src_ip,
        src_port: u16::from_be_bytes([udp[0], udp[1]]),
        dst_port: u16::from_be_bytes([udp[2], udp[3]]),
        data: &udp[8..],
    })
}

/// UDP checksum over the IPv4 pseudo-header + datagram. A zero
/// checksum is legal on IPv4 UDP (unchecked send); we compute ours
/// and verify inbound ones when present.
pub fn udp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp: &[u8]) -> u16 {
    let mut scratch = [0u8; 12 + DG_BYTES + 8];
    scratch[0..4].copy_from_slice(&src_ip);
    scratch[4..8].copy_from_slice(&dst_ip);
    scratch[8] = 0;
    scratch[9] = PROTO_UDP;
    scratch[10..12].copy_from_slice(&(udp.len() as u16).to_be_bytes());
    let n = udp.len().min(DG_BYTES + 8);
    scratch[12..12 + n].copy_from_slice(&udp[..n]);
    checksum(&scratch[..12 + n])
}

/// Build an Ethernet/IPv4/UDP frame. The cone has ALREADY approved
/// `data` — this is the post-cone path.
pub fn build_udp(
    our_mac: [u8; 6],
    next_mac: [u8; 6],
    dst_ip: [u8; 4],
    dst_port: u16,
    src_port: u16,
    data: &[u8],
) -> Vec2 {
    let udp_len = 8 + data.len();
    let total = 20 + udp_len;
    let mut out = [0u8; PACKET_BYTES];
    out[0..6].copy_from_slice(&next_mac);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    {
        let ip = &mut out[14..34];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        ip[4..6].copy_from_slice(&[0x38, 0x38]);
        ip[6..8].copy_from_slice(&[0, 0]);
        ip[8] = 64;
        ip[9] = PROTO_UDP;
        ip[12..16].copy_from_slice(&OUR_IP);
        ip[16..20].copy_from_slice(&dst_ip);
    }
    let hck = checksum(&out[14..34]);
    out[14 + 10..14 + 12].copy_from_slice(&hck.to_be_bytes());
    out[14 + 20..14 + 22].copy_from_slice(&src_port.to_be_bytes());
    out[14 + 22..14 + 24].copy_from_slice(&dst_port.to_be_bytes());
    out[14 + 24..14 + 26].copy_from_slice(&(udp_len as u16).to_be_bytes());
    out[14 + 26..14 + 28].copy_from_slice(&[0, 0]);
    out[14 + 28..14 + 28 + data.len()].copy_from_slice(data);
    let ick = udp_checksum(OUR_IP, dst_ip, &out[14 + 20..14 + 20 + udp_len]);
    out[14 + 26..14 + 28].copy_from_slice(&ick.to_be_bytes());
    Vec2 {
        bytes: out,
        len: 14 + total,
    }
}

// ---------------------------------------------------------------------------
// Kernel-side state (behind the house lock).
// ---------------------------------------------------------------------------

/// Minimal spin lock (same envelope as kernel-sense's).
pub struct Lock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}
unsafe impl<T: Send> Sync for Lock<T> {}
pub struct Guard<'a, T> {
    lock: &'a Lock<T>,
}
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}
impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}
impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() }
    }
}
impl<T> Lock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }
    pub fn lock(&self) -> Guard<'_, T> {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
        Guard { lock: self }
    }
}

struct Listener {
    port: u16,
    ring: [[u8; DG_BYTES]; DG_SLOTS],
    lens: [usize; DG_SLOTS],
    head: usize,
    count: usize,
    last_sender_ip: [u8; 4],
    last_sender_port: u16,
}

impl Listener {
    const EMPTY: Self = Self {
        port: 0,
        ring: [[0; DG_BYTES]; DG_SLOTS],
        lens: [0; DG_SLOTS],
        head: 0,
        count: 0,
        last_sender_ip: [0; 4],
        last_sender_port: 0,
    };
}

struct NetState {
    up: bool,
    mac: [u8; 6],
    gateway_mac: [u8; 6],
    listeners: [Listener; LISTENERS],
    /// App packet ring (foreign payloads beyond ARP/ICMP we answer).
    ring: [[u8; PACKET_BYTES]; RING_SLOTS],
    lens: [usize; RING_SLOTS],
    head: usize,
    count: usize,
    /// Stats (receipts).
    rx_total: u64,
    arp_replied: u64,
    icmp_replied: u64,
    tx_total: u64,
    cone_blocked: u64,
    echo_id: u16,
}

static STATE: Lock<NetState> = Lock::new(NetState {
    up: false,
    mac: [0; 6],
    gateway_mac: [0; 6],
    ring: [[0; PACKET_BYTES]; RING_SLOTS],
    lens: [0; RING_SLOTS],
    head: 0,
    count: 0,
    listeners: [Listener::EMPTY; LISTENERS],
    rx_total: 0,
    arp_replied: 0,
    icmp_replied: 0,
    tx_total: 0,
    cone_blocked: 0,
    echo_id: 0x3713,
});

/// Bring the stack up on a discovered device. Called once at boot
/// (or never, on a -nic none machine: the layer stays down and every
/// syscall answers ERROR — the OS without a wire).
pub fn bring_up(device: &mut kernel_kit::virtio_net::VirtioNet) {
    let mac = device.mac();
    let mut state = STATE.lock();
    state.mac = mac;
    state.up = true;
}

pub fn is_up() -> bool {
    STATE.lock().up
}

/// The kernel's own handling of one received frame. ARP requests for
/// us and ICMP echoes to us are answered IN KERNEL; everything else
/// (including ICMP echo REPLIES to our probes) lands in the app ring.
/// The `transmit` closure is the driver's send path.
pub fn handle_frame(packet: &[u8], transmit: &mut dyn FnMut(&[u8])) {
    lightcone::observe(0, packet);
    let (header, _) = match parse_eth(packet) {
        Some(split) => split,
        None => return,
    };
    let mut state = STATE.lock();
    state.rx_total += 1;
    // Learn the gateway's MAC from anything it sends (slirp ARPs and
    // replies arrive from 10.0.2.2's MAC).
    if header.ethertype == ETHERTYPE_IPV4 {
        let ip = &packet[14..];
        if ip.len() >= 20 {
            let src_ip = &ip[12..16];
            if src_ip == GATEWAY_IP {
                state.gateway_mac = header.src;
            }
        }
    }
    if header.ethertype == ETHERTYPE_ARP {
        // Replies teach us the gateway's MAC (the bootstrap answer).
        if packet.len() >= 42 {
            let oper = u16::from_be_bytes([packet[20], packet[21]]);
            let sender_ip = &packet[28..32];
            if oper == 2 && sender_ip == GATEWAY_IP {
                state.gateway_mac = header.src;
                return; // consumed by the stack, not app material
            }
        }
        if let Some(reply) = arp_reply(packet, state.mac) {
            transmit(&reply);
            state.arp_replied += 1;
            return;
        }
    }
    if header.ethertype == ETHERTYPE_IPV4 {
        let want_icmp = {
            let ip = &packet[14..];
            ip.len() >= 28 && ip[9] == 1 && ip[20] == 8 && {
                let dst = &ip[16..20];
                dst == OUR_IP
            }
        };
        if want_icmp {
            if let Some(reply) = icmp_echo_reply(packet, state.mac) {
                transmit(&reply.bytes[..reply.len]);
                state.icmp_replied += 1;
                return;
            }
        }
    }
    // UDP: demux by destination port to the listener that earned it.
    if header.ethertype == ETHERTYPE_IPV4 {
        if let Some(dg) = parse_udp(packet) {
            if let Some(listener) = state
                .listeners
                .iter_mut()
                .find(|l| l.port == dg.dst_port && l.port != 0)
            {
                if listener.count < DG_SLOTS && dg.data.len() <= DG_BYTES {
                    let slot = (listener.head + listener.count) % DG_SLOTS;
                    listener.ring[slot][..dg.data.len()].copy_from_slice(dg.data);
                    listener.lens[slot] = dg.data.len();
                    listener.count += 1;
                    listener.last_sender_ip = dg.src_ip;
                    listener.last_sender_port = dg.src_port;
                    return; // consumed by the socket layer
                }
            }
        }
    }
    // Foreign material: the app ring. A reader will pay taint for it.
    if state.count < RING_SLOTS && packet.len() <= PACKET_BYTES {
        let slot = (state.head + state.count) % RING_SLOTS;
        state.ring[slot][..packet.len()].copy_from_slice(packet);
        state.lens[slot] = packet.len();
        state.count += 1;
    }
}

/// SYS_NET_RECV: hand the oldest foreign packet to the caller — and
/// MARK THE CALLER TAINTED (the ingress ceremony: reading network
/// data is handling foreign content; derived stays data). Copies the
/// packet into `out`; returns its length, or ERROR (u64::MAX) if the
/// ring is empty.
pub fn recv_into(out: &mut [u8]) -> Option<usize> {
    let (len, src) = {
        let mut state = STATE.lock();
        if state.count == 0 {
            return None;
        }
        let slot = state.head;
        let len = state.lens[slot].min(out.len());
        let src = state.ring[slot];
        state.head = (state.head + 1) % RING_SLOTS;
        state.count -= 1;
        (len, src)
    };
    out[..len].copy_from_slice(&src[..len]);
    Some(len)
}

/// SYS_NET_SEND: the egress cone first (prose passes; key-shaped data
/// never reaches the wire), then out as an ICMP echo request to the
/// gateway. Returns Ok(()) or Err(cause) for the receipt.
pub enum SendCause {
    Cone,
    NoRoute,
    Driver,
}
pub fn send_probed(data: &[u8], transmit: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), SendCause> {
    if !kernel_egress::gate(data) {
        STATE.lock().cone_blocked += 1;
        return Err(SendCause::Cone);
    }
    let (mac, gateway_mac, id) = {
        let state = STATE.lock();
        (state.mac, state.gateway_mac, state.echo_id)
    };
    if gateway_mac == [0; 6] {
        return Err(SendCause::NoRoute);
    }
    let mut probe = icmp_echo_request(mac, gateway_mac, id, 1, data);
    {
        let mut state = STATE.lock();
        state.echo_id = state.echo_id.wrapping_add(1);
        state.tx_total += 1;
    }
    if !transmit(&probe.bytes[..probe.len]) {
        return Err(SendCause::Driver);
    }
    Ok(())
}

/// Packed status for SYS_NET sub 0:
/// (up<<63) | arp_replied<<48 | icmp_replied<<32 | rx_total... too
/// rich for one word; this surface returns the receipts that matter:
/// (up<<63) | (ring_count<<56) | (rx_total&0xFF_FFFF<<32)... — the
/// payload only needs up/ring/arp/icmp/cone. Keep it simple:
/// (up<<63) | (ring_count << 48) | (arp<<32) | (icmp<<16) | cone.
pub fn status() -> u64 {
    let state = STATE.lock();
    (u64::from(state.up) << 63)
        | ((state.count as u64) << 48)
        | ((state.arp_replied.min(0xFFFF) as u64) << 32)
        | ((state.icmp_replied.min(0xFFFF) as u64) << 16)
        | (state.cone_blocked.min(0xFFFF) as u64)
}

/// Broadcast ARP request for the gateway (bootstrap: nothing can be
/// sent until the gateway's MAC is known, and slirp speaks only when
/// spoken to).
pub fn arp_request(our_mac: [u8; 6]) -> [u8; 42] {
    let mut out = [0u8; 42];
    out[0..6].copy_from_slice(&[0xFF; 6]);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    out[14..18].copy_from_slice(&[0, 1, 0x08, 0x00]);
    out[18] = 6;
    out[19] = 4;
    out[20..22].copy_from_slice(&1u16.to_be_bytes()); // REQUEST
    out[22..28].copy_from_slice(&our_mac);
    out[28..32].copy_from_slice(&OUR_IP);
    out[32..38].copy_from_slice(&[0; 6]);
    out[38..42].copy_from_slice(&GATEWAY_IP);
    out
}

/// The device registry: one NIC, held behind the house lock. Absent
/// on a -nic none machine — the layer stays down, syscalls error,
/// the OS runs wireless.
static DEVICE: Lock<Option<kernel_kit::virtio_net::VirtioNet>> = Lock::new(None);

/// Discover and bring up the NIC at boot. Failure is silence: a
/// machine without a wire is a valid machine.
pub fn boot_discover() -> bool {
    lightcone::initialize();
    match kernel_kit::virtio_net::VirtioNet::discover() {
        Ok(mut device) => {
            bring_up(&mut device);
            *DEVICE.lock() = Some(device);
            true
        }
        Err(_) => false,
    }
}

/// One heartbeat of the wire: poll the device (batch-collected, so
/// the driver lock is released before the stack lock is taken — no
/// nesting), then run the kernel's own handling per packet. Called
/// from timer context; bounded to a small batch per beat.
pub fn heartbeat() {
    let mut batch: [[u8; PACKET_BYTES]; 4] = [[0; PACKET_BYTES]; 4];
    let mut lens = [0usize; 4];
    let mut count = 0usize;
    {
        let mut device = DEVICE.lock();
        if let Some(net) = device.as_mut() {
            let mut sink = |packet: &[u8]| {
                if count < 4 {
                    let len = packet.len().min(PACKET_BYTES);
                    batch[count][..len].copy_from_slice(packet);
                    lens[count] = len;
                    count += 1;
                }
            };
            net.poll_rx(&mut sink);
        }
    }
    for i in 0..count {
        let transmit = &mut |bytes: &[u8]| {
            let _ = send_raw(bytes);
        };
        handle_frame(&batch[i][..lens[i]], transmit);
    }
}

/// Raw transmit through the registry. Returns success.
pub fn send_raw(bytes: &[u8]) -> bool {
    let sent = {
        let mut device = DEVICE.lock();
        match device.as_mut() {
            Some(net) => net.transmit(bytes).is_ok(),
            None => false,
        }
    };
    if sent {
        lightcone::observe(1, bytes);
    }
    sent
}

/// SYS_NET sub 4: ARP bootstrap probe (broadcast ask for the
/// gateway). Receipt: 1 sent, 0 when the device is absent.
pub fn arp_probe() -> u64 {
    let mac = STATE.lock().mac;
    if mac == [0; 6] {
        return 0;
    }
    let request = arp_request(mac);
    if send_raw(&request) {
        1
    } else {
        0
    }
}

/// Bind a listener port (the ceremony: a port is a place you chose
/// to receive foreign material). Returns success.
pub fn bind(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let mut state = STATE.lock();
    if state.listeners.iter().any(|l| l.port == port) {
        return true;
    }
    match state.listeners.iter_mut().find(|l| l.port == 0) {
        Some(listener) => {
            listener.port = port;
            true
        }
        None => false,
    }
}

/// Take the oldest datagram for a port. Returns None when empty;
/// Some(len) with the payload copied into `out`.
pub fn udp_recv_into(port: u16, out: &mut [u8]) -> Option<usize> {
    let mut state = STATE.lock();
    let listener = state.listeners.iter_mut().find(|l| l.port == port)?;
    if listener.count == 0 {
        return None;
    }
    let slot = listener.head;
    let len = listener.lens[slot].min(out.len());
    out[..len].copy_from_slice(&listener.ring[slot][..len]);
    listener.head = (listener.head + 1) % DG_SLOTS;
    listener.count -= 1;
    Some(len)
}

/// The last sender on a port: (ip, port) for the reply path.
pub fn udp_last_sender(port: u16) -> Option<([u8; 4], u16)> {
    let state = STATE.lock();
    let listener = state.listeners.iter().find(|l| l.port == port)?;
    if listener.last_sender_port == 0 {
        return None;
    }
    Some((listener.last_sender_ip, listener.last_sender_port))
}

/// The socket-layer send: the egress cone first (the DATA — never
/// the addressing header), then out via the gateway next-hop.
pub enum UdpCause {
    Cone,
    NoRoute,
    Driver,
}
pub fn udp_send(
    dst_ip: [u8; 4],
    dst_port: u16,
    src_port: u16,
    data: &[u8],
    transmit: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<(), UdpCause> {
    if !kernel_egress::gate(data) {
        STATE.lock().cone_blocked += 1;
        return Err(UdpCause::Cone);
    }
    let (mac, next) = {
        let state = STATE.lock();
        (state.mac, state.gateway_mac)
    };
    if next == [0; 6] || mac == [0; 6] {
        return Err(UdpCause::NoRoute);
    }
    let frame = build_udp(mac, next, dst_ip, dst_port, src_port, data);
    STATE.lock().tx_total += 1;
    if !transmit(&frame.bytes[..frame.len]) {
        return Err(UdpCause::Driver);
    }
    Ok(())
}

/// Host-test support: set our MAC without a device.
#[cfg(feature = "std")]
pub fn set_mac(mac: [u8; 6]) {
    STATE.lock().mac = mac;
}

/// Host-test reset.
#[cfg(feature = "std")]
pub fn reset() {
    let mut state = STATE.lock();
    state.up = false;
    state.gateway_mac = [0; 6];
    state.head = 0;
    state.count = 0;
    state.listeners = [Listener::EMPTY; LISTENERS];
    state.rx_total = 0;
    state.arp_replied = 0;
    state.icmp_replied = 0;
    state.tx_total = 0;
    state.cone_blocked = 0;
    state.echo_id = 0x3713;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// N1: the checksum fold — known values.
    #[test]
    fn gate_n1_checksum() {
        assert_eq!(checksum(&[]), 0xFFFF);
        // 0x0001 + 0x0002 = 3 -> !3 = 0xFFFC
        assert_eq!(checksum(&[0x00, 0x01, 0x00, 0x02]), 0xFFFC);
        // RFC 1071 example data.
        let data = [0x00u8, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(checksum(&data), 0x220D);
    }

    fn inbound_udp(data: &[u8]) -> Vec2 {
        build_udp(
            [0x52, 0x54, 0, 0x12, 0x34, 0x02],
            [0x52, 0x54, 0, 0x12, 0x34, 0x56],
            OUR_IP,
            9100,
            9000,
            data,
        )
    }

    fn repair_ipv4_checksum(frame: &mut [u8]) {
        frame[24..26].copy_from_slice(&[0, 0]);
        let value = checksum(&frame[14..34]);
        frame[24..26].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn udp_delivery_rejects_invalid_ip_and_transport_boundaries() {
        let valid = inbound_udp(b"valid");
        assert_eq!(parse_udp(&valid.bytes[..valid.len]).unwrap().data, b"valid");

        let mut bad = valid.bytes;
        bad[24] ^= 1;
        assert!(parse_udp(&bad[..valid.len]).is_none());

        let mut bad = valid.bytes;
        bad[40..42].copy_from_slice(&0x1234u16.to_be_bytes());
        assert!(parse_udp(&bad[..valid.len]).is_none());

        let mut bad = valid.bytes;
        bad[16..18].copy_from_slice(&((valid.len + 100) as u16).to_be_bytes());
        repair_ipv4_checksum(&mut bad);
        assert!(parse_udp(&bad[..valid.len]).is_none());

        let mut bad = valid.bytes;
        bad[38..40].copy_from_slice(&400u16.to_be_bytes());
        assert!(parse_udp(&bad[..valid.len]).is_none());

        let mut bad = valid.bytes;
        bad[20..22].copy_from_slice(&16u16.to_be_bytes());
        repair_ipv4_checksum(&mut bad);
        assert!(parse_udp(&bad[..valid.len]).is_none());

        let mut bad = valid.bytes;
        bad[30..34].copy_from_slice(&[10, 0, 2, 99]);
        repair_ipv4_checksum(&mut bad);
        assert!(parse_udp(&bad[..valid.len]).is_none());
    }
}
