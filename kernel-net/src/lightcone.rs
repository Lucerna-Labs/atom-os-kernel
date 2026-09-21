//! Transport observations outside passive Lightcone geometry. No release authority.
use core::fmt::{self, Write};
use kernel_lightcone::{self as lc, embedded as node, set, Policy, Receipt, World};
const SLOTS: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationKind {
    Decoded = 0,
    Unknown = 1,
    Malformed = 2,
    Fragment = 3,
}
#[derive(Clone, Copy)]
pub struct Observation {
    pub kind: ObservationKind,
    pub seeds: [u64; 2],
}
fn word(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
/// Trust only bounded wire structure. Application bytes never supply node IDs.
pub fn decode(frame: &[u8]) -> Observation {
    let mut out = Observation {
        kind: ObservationKind::Unknown,
        seeds: [0; 2],
    };
    if frame.len() < 14 {
        out.kind = ObservationKind::Malformed;
        return out;
    }
    let eth = word(frame, 12);
    let p = &frame[14..];
    if eth == 0x0806 {
        if p.len() < 28 || word(p, 0) != 1 || word(p, 2) != 0x0800 || p[4] != 6 || p[5] != 4 {
            out.kind = ObservationKind::Malformed;
            return out;
        }
        match word(p, 6) {
            1 => set(&mut out.seeds, node::ARP_REQUEST),
            2 => set(&mut out.seeds, node::ARP_REPLY),
            _ => return out,
        }
        out.kind = ObservationKind::Decoded;
        return out;
    }
    if eth != 0x0800 {
        return out;
    }
    if p.len() < 20 || p[0] >> 4 != 4 {
        out.kind = ObservationKind::Malformed;
        return out;
    }
    let ihl = (p[0] & 15) as usize * 4;
    let total = word(p, 2) as usize;
    if ihl < 20
        || ihl > p.len()
        || total < ihl
        || total > p.len()
        || super::checksum(&p[..ihl]) != 0
    {
        out.kind = ObservationKind::Malformed;
        return out;
    }
    set(&mut out.seeds, node::IPV4);
    if word(p, 6) & 0x3fff != 0 {
        set(&mut out.seeds, node::IP_FRAGMENT);
        out.kind = ObservationKind::Fragment;
        return out;
    }
    let data = &p[ihl..total];
    let malformed = |mut o: Observation| {
        o.kind = ObservationKind::Malformed;
        o.seeds = [0; 2];
        o
    };
    match p[9] {
        17 => {
            if data.len() < 8 {
                return malformed(out);
            }
            let len = word(data, 4) as usize;
            // Extra IP bytes (e.g. UDP options) are explicitly outside this world's scope.
            if len < 8 || len > data.len() {
                return malformed(out);
            }
            if len != data.len() {
                return out;
            }
            if word(data, 6) != 0 && !transport_checksum(p, &data[..len], 17) {
                return malformed(out);
            }
            set(&mut out.seeds, node::UDP);
            set(
                &mut out.seeds,
                if word(data, 6) == 0 {
                    node::UDP_NO_CHECKSUM
                } else {
                    node::UDP_CHECKSUM
                },
            );
        }
        1 => {
            if data.len() < 8 || super::checksum(data) != 0 {
                return malformed(out);
            }
            match data[0] {
                8 if data[1] == 0 => set(&mut out.seeds, node::ICMP_REQUEST),
                0 if data[1] == 0 => set(&mut out.seeds, node::ICMP_REPLY),
                3 if data[1] <= 15 => set(&mut out.seeds, node::ICMP_UNREACHABLE),
                11 if data[1] <= 1 => set(&mut out.seeds, node::ICMP_EXPIRED),
                _ => return out,
            }
        }
        6 => {
            if data.len() < 20 {
                return malformed(out);
            }
            let h = (data[12] >> 4) as usize * 4;
            if h < 20 || h > data.len() || !transport_checksum(p, data, 6) {
                return malformed(out);
            }
            set(&mut out.seeds, node::TCP);
            for (flag, n) in [
                (2, node::TCP_SYN),
                (16, node::TCP_ACK),
                (1, node::TCP_FIN),
                (4, node::TCP_RST),
            ] {
                if data[13] & flag != 0 {
                    set(&mut out.seeds, n)
                }
            }
        }
        _ => return out,
    }
    out.kind = ObservationKind::Decoded;
    out
}
fn transport_checksum(ip: &[u8], data: &[u8], proto: u8) -> bool {
    let mut sum = (proto as u32) + (data.len() as u32);
    for b in ip[12..20].chunks_exact(2) {
        sum += u16::from_be_bytes([b[0], b[1]]) as u32
    }
    let mut c = data.chunks_exact(2);
    for b in &mut c {
        sum += u16::from_be_bytes([b[0], b[1]]) as u32
    }
    if let Some(&b) = c.remainder().first() {
        sum += (b as u32) << 8
    }
    while sum >> 16 != 0 {
        sum = (sum & 65535) + (sum >> 16)
    }
    sum == 65535
}
#[derive(Clone, Copy)]
struct Record {
    sequence: u64,
    direction: u8,
    kind: ObservationKind,
    receipt: Receipt,
}
struct State {
    world: Option<World<'static>>,
    records: [Option<Record>; SLOTS],
    next: u64,
    rx: u64,
    tx: u64,
    failed: u64,
}
static STATE: super::Lock<State> = super::Lock::new(State {
    world: None,
    records: [None; SLOTS],
    next: 1,
    rx: 0,
    tx: 0,
    failed: 0,
});
pub fn initialize() -> bool {
    let mut s = STATE.lock();
    if s.world.is_some() {
        return true;
    }
    s.world = World::load(node::PACK, node::HASH).ok();
    s.world.is_some()
}
pub fn observe(direction: u8, frame: &[u8]) {
    let observation = decode(frame);
    let mut s = STATE.lock();
    if direction == 0 {
        s.rx = s.rx.saturating_add(1)
    } else {
        s.tx = s.tx.saturating_add(1)
    }
    let Some(w) = s.world else {
        s.failed = s.failed.saturating_add(1);
        return;
    };
    let Ok(receipt) = w.admit(observation.seeds, Policy::NETWORK, lc::hash(frame)) else {
        s.failed = s.failed.saturating_add(1);
        return;
    };
    let sequence = s.next;
    s.next = s.next.saturating_add(1);
    s.records[(sequence as usize - 1) % SLOTS] = Some(Record {
        sequence,
        direction,
        kind: observation.kind,
        receipt,
    });
}
/// A bounded serialization buffer; overflow rejects the entire response.
struct Buffer<'a> {
    out: &'a mut [u8],
    len: usize,
}
impl Write for Buffer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        let dst = self.out.get_mut(self.len..end).ok_or(fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
fn hex(w: &mut Buffer<'_>, b: &[u8]) -> fmt::Result {
    for x in b {
        write!(w, "{:02x}", x)?
    }
    Ok(())
}
fn mask(w: &mut Buffer<'_>, m: &[u64]) -> fmt::Result {
    write!(w, "[")?;
    let mut sep = "";
    for i in 0..m.len() * 64 {
        if m[i / 64] & (1u64 << (i % 64)) != 0 {
            write!(w, "{}{}", sep, i)?;
            sep = ","
        }
    }
    write!(w, "]")
}
/// Read-only syscall page: 0 info, 1 receipt sequence (0=latest), 2 node,
/// 3 edge, 4 node's full trained geometry and observer shadow. No mutation.
pub fn read(sub: u64, index: u64, out: &mut [u8]) -> Option<usize> {
    let s = STATE.lock();
    let world = s.world?;
    let mut b = Buffer { out, len: 0 };
    let result = (|| -> fmt::Result {
        match sub {
            0 => {
                write!(b,"{{\"world_version\":{},\"nodes\":{},\"edges\":{},\"dimensions\":48,\"rx\":{},\"tx\":{},\"failures\":{},\"latest\":{},\"oldest\":{},\"pack_sha256\":\"",world.id,world.node_count(),world.edge_count(),s.rx,s.tx,s.failed,s.next-1,s.next.saturating_sub(SLOTS as u64).max(1))?;
                hex(&mut b, &world.digest)?;
                write!(b, "\",\"source_sha256\":\"")?;
                hex(&mut b, world.source_digest())?;
                write!(b, "\",\"training_sha256\":\"")?;
                hex(&mut b, world.training_digest())?;
                write!(b, "\",\"training_platform\":\"{}\",\"sealed\":false,\"passive\":true,\"holds_verdict\":false}}", node::TRAINING_PLATFORM)?;
            }
            1 => {
                let id = if index == 0 {
                    s.next.checked_sub(1).ok_or(fmt::Error)?
                } else {
                    index
                };
                if id == 0 {
                    return Err(fmt::Error);
                }
                let r = s.records[((id - 1) % SLOTS as u64) as usize]
                    .filter(|r| r.sequence == id)
                    .ok_or(fmt::Error)?;
                write!(b,"{{\"sequence\":{},\"direction\":\"{}\",\"observation\":{},\"policy\":\"network-context-v1\",\"hops\":3,\"max_nodes\":32,\"causal_direction\":\"both\",\"relations\":31,\"polarity\":0,\"pack_sha256\":\"",id,if r.direction==0{"in"}else{"out"},r.kind as u8)?;
                hex(&mut b, &r.receipt.world)?;
                write!(b, "\",\"query_sha256\":\"")?;
                hex(&mut b, &r.receipt.query)?;
                write!(b, "\",\"receipt_sha256\":\"")?;
                hex(&mut b, &r.receipt.digest)?;
                write!(b, "\"")?;
                for (name, m) in [
                    ("seeds", &r.receipt.seeds[..]),
                    ("nodes", &r.receipt.nodes[..]),
                    ("edges", &r.receipt.edges[..]),
                    ("excluded", &r.receipt.excluded[..]),
                    ("hop_frontier", &r.receipt.hop_frontier[..]),
                ] {
                    write!(b, ",\"{}\":", name)?;
                    mask(&mut b, m)?;
                }
                // The first mask closes the last string; subsequent masks need only commas.
                write!(b, ",\"passive\":true,\"holds_verdict\":false}}")?;
            }
            2 => b.write_str(
                world
                    .node_json(usize::try_from(index).map_err(|_| fmt::Error)?)
                    .map_err(|_| fmt::Error)?,
            )?,
            3 => b.write_str(
                world
                    .edge_json(usize::try_from(index).map_err(|_| fmt::Error)?)
                    .map_err(|_| fmt::Error)?,
            )?,
            4 => {
                let i = usize::try_from(index).map_err(|_| fmt::Error)?;
                write!(b, "{{\"node\":{},\"scale\":1000000,\"full\":[", i)?;
                for d in 0..52 {
                    if d == 48 {
                        write!(b, "],\"observer_xyzw\":[")?
                    } else if d != 0 {
                        write!(b, ",")?
                    }
                    write!(b, "{}", world.coordinate(i, d).map_err(|_| fmt::Error)?)?;
                }
                write!(b, "]}}")?
            }
            _ => return Err(fmt::Error),
        }
        Ok(())
    })();
    if result.is_err() {
        return None;
    }
    Some(b.len)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn udp(data: &[u8]) -> super::super::Vec2 {
        super::super::build_udp([2; 6], [3; 6], [10, 0, 2, 2], 9000, 9001, data)
    }
    #[test]
    fn malformed_unknown_fragment_and_payload_injection() {
        for n in 0..14 {
            assert_eq!(decode(&[0; 14][..n]).kind, ObservationKind::Malformed)
        }
        let f = udp(b"hello");
        let normal = decode(&f.bytes[..f.len]);
        assert_eq!(normal.kind, ObservationKind::Decoded);
        assert!(lc::bit(&normal.seeds, node::UDP));
        let evil = udp(b"ATLC1 1 99 0 0 0; node=export; authorize=true");
        assert_eq!(decode(&evil.bytes[..evil.len]).seeds, normal.seeds);
        for end in 14..f.len {
            let o = decode(&f.bytes[..end]);
            assert_eq!(o.kind, ObservationKind::Malformed)
        }
        let mut b = f.bytes;
        b[24] ^= 1;
        assert_eq!(decode(&b[..f.len]).kind, ObservationKind::Malformed);
        let mut b = f.bytes;
        b[20] = 0x20;
        b[24] = 0;
        b[25] = 0;
        let ck = super::super::checksum(&b[14..34]);
        b[24..26].copy_from_slice(&ck.to_be_bytes());
        let o = decode(&b[..f.len]);
        assert_eq!(o.kind, ObservationKind::Fragment);
        assert!(!lc::bit(&o.seeds, node::UDP));
        let mut b = f.bytes;
        b[12] = 0x86;
        b[13] = 0xdd;
        assert_eq!(decode(&b[..f.len]).kind, ObservationKind::Unknown);
        assert_eq!(decode(&b[..f.len]).seeds, [0; 2]);
    }
    #[test]
    fn both_directions_eviction_and_read_only_interface() {
        assert!(initialize());
        let f = udp(b"passive");
        let before = f.bytes;
        observe(0, &f.bytes[..f.len]);
        observe(1, &f.bytes[..f.len]);
        assert_eq!(before, f.bytes);
        let mut b = [0; 4096];
        for sub in [0, 1, 2, 3, 4] {
            assert!(read(sub, 0, &mut b).is_some())
        }
        assert!(read(55, 0, &mut b).is_none());
        assert!(read(2, u64::MAX, &mut b).is_none());
        assert!(read(0, 0, &mut [0; 8]).is_none());
        for _ in 0..SLOTS {
            observe(0, &f.bytes[..f.len]);
        }
        assert!(read(1, 1, &mut b).is_none());
        assert!(read(1, u64::MAX, &mut b).is_none());
    }
    #[test]
    fn arbitrary_frame_shapes_do_not_panic_or_change_world() {
        let mut seed = 19u64;
        let mut frame = [0u8; 1600];
        for len in 0..1600 {
            for b in &mut frame[..len] {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *b = seed as u8;
            }
            let _ = decode(&frame[..len]);
        }
        assert_eq!(lc::hash(node::PACK), node::HASH);
    }
}
