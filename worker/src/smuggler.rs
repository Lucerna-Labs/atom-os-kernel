//! E24 smuggler payload — tries three exfiltrations through the
//! egress cone. The key-shaped data is embedded (labeled: not a real
//! key; the channel gate is what's under test). Even if secrets were
//! readable INSIDE, the door out is prose-shaped.

#![no_std]
#![no_main]
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

// 64 pseudo-random bytes: what raw key material looks like.
const KEY_SHAPED: [u8; 64] = [
    0x8d, 0x20, 0xfb, 0x62, 0x5f, 0x6f, 0x5c, 0xc4, 0x68, 0x08, 0xd7, 0x0f, 0x37, 0x2e, 0xc4, 0xad,
    0x91, 0x7c, 0x3b, 0xa4, 0xe2, 0xf1, 0x55, 0x19, 0x0d, 0xc2, 0x7e, 0x41, 0xb8, 0x9d, 0x02, 0x51,
    0xf7, 0x30, 0xa1, 0xcc, 0x6e, 0x19, 0x4b, 0x8a, 0x2d, 0x76, 0x03, 0xc9, 0xaa, 0x5e, 0x71, 0xf0,
    0x44, 0x2b, 0xbe, 0x87, 0xd6, 0x12, 0xc4, 0x99, 0x37, 0xe5, 0x8c, 0x21, 0x0b, 0x74, 0xde, 0x66,
];

const HEX: &[u8] = b"8d20fb625f6f5cc46808d70f372ec4ad917c3ba4e2f155190dc27e41b89d0251f730a1cc6e194b8a2d7603c9aa5e71f0442bbe87d612c49937e58c210b74de66";

fn hex_encode(bytes: &[u8]) -> [u8; 128] {
    let mut out = [0u8; 128];
    for (i, byte) in bytes.iter().enumerate() {
        out[i * 2] = b"0123456789abcdef"[(byte >> 4) as usize];
        out[i * 2 + 1] = b"0123456789abcdef"[(byte & 0xf) as usize];
    }
    out
}

fn sanitize(bytes: &mut [u8]) {
    // Keep the payload key-shaped but free of accidental structure
    // bytes (raw random can luck into >=5% punctuation; real key
    // material has no such obligation — the gate's job is the
    // distinct-count bound, not luck).
    for byte in bytes.iter_mut() {
        while matches!(*byte, b' ' | b'.' | b',' | b'\n' | b':' | b';' | b'-' | b'(' | b')' | b'[' | b']' | b'!' | b'?') {
            *byte = byte.rotate_left(3) ^ 0x5A;
        }
    }
}

fn main() {
    rt::sleep(50);
    // The kernel's own prints are per-byte SYS_WRITE (under the
    // gating size), so the smuggler trains the cone itself first:
    // one long legitimate prose write = 256 training windows.
    let template = b"[Smuggler] training the egress cone with ordinary outbound operations prose so the gate learns what legitimate traffic looks like before any test runs. ";
    let mut training = [0u8; 256 * 64];
    for (slot, byte) in training.iter_mut().zip(template.iter().copied().cycle()) {
        *slot = byte;
    }
    // SYS_WRITE_BUFFER caps at 4096 bytes per call: four calls carry
    // the 16 KiB, 64 windows each — exactly the 256-window freeze.
    let mut trained = 0u64;
    for chunk in training.chunks(4096) {
        if rt::call3(SYS_WRITE_BUFFER, chunk.as_ptr() as u64, chunk.len() as u64, 0) != ERROR {
            trained += chunk.len() as u64 / 64;
        }
    }
    if trained >= 256 {
        rt::print_args(format_args!("[Smuggler] cone trained on {trained} prose windows\n"));
    } else {
        rt::print_args(format_args!("[Smuggler] training FAILED at {trained} windows\n"));
        rt::exit(9);
    }
    let mut refused = 0;
    let mut key = KEY_SHAPED;
    sanitize(&mut key);
    let hex = hex_encode(&key);

    // Attempt 1: raw key material straight out.
    if rt::call3(SYS_WRITE_BUFFER, key.as_ptr() as u64, key.len() as u64, 0) == ERROR {
        refused += 1;
        rt::print("[Smuggler] raw 64-byte key: REFUSED (entropy ceiling — the noise class)\n");
    } else {
        rt::print("[Smuggler] raw key PASSED?! — GATE FAILED\n");
        rt::exit(7);
    }

    // Attempt 2: hex-encoded (16 symbols — under the distinct
    // ceiling, but zero prose structure: no spaces, no punctuation).
    if rt::call3(SYS_WRITE_BUFFER, hex.as_ptr() as u64, hex.len() as u64, 0) == ERROR {
        refused += 1;
        rt::print("[Smuggler] hex-encoded key: REFUSED (structure profile — bits without prose)\n");
    } else {
        rt::print("[Smuggler] hex PASSED?! — GATE FAILED\n");
        rt::exit(7);
    }

    // Attempt 3: key bytes smuggled INSIDE a prose sentence.
    let mut combined = [0u8; 128];
    let prefix = b"status report: all systems nominal, telemetry follows ";
    combined[..prefix.len()].copy_from_slice(prefix);
    combined[prefix.len()..prefix.len() + 48].copy_from_slice(&key[..48]);
    if rt::call3(SYS_WRITE_BUFFER, combined.as_ptr() as u64, 128, 0) == ERROR {
        refused += 1;
        rt::print("[Smuggler] key-inside-prose: REFUSED (the embedding window is noise)\n");
    } else {
        rt::print("[Smuggler] embedded key PASSED?! — GATE FAILED\n");
        rt::exit(7);
    }

    // Control: ordinary prose sails through.
    let prose = b"[Smuggler] attempt log: three refusals. This sentence is ordinary outbound prose and must pass the cone.\n";
    if rt::call3(SYS_WRITE_BUFFER, prose.as_ptr() as u64, prose.len() as u64, 0) != ERROR {
        rt::print("[Smuggler] prose control: PASSED — E24 PASS: the door out is prose-shaped\n");
        rt::exit(0);
    } else {
        rt::print("[Smuggler] prose REFUSED?! — false positive, GATE FAILED\n");
        rt::exit(8);
    }
}
