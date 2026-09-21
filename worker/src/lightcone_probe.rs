//! Exercise the real read-only network-world ABI from userspace.
use alloc::string::String;
use user_rt::{self as rt, abi::*};
fn read(sub: u64, index: u64) -> Option<String> {
    let n = rt::call3(SYS_LIGHTCONE, sub, index, 0);
    if n == ERROR {
        return None;
    }
    assert!(n <= 4096);
    let b = unsafe { core::slice::from_raw_parts(LIGHTCONE_PAGE as *const u8, n as usize) };
    Some(core::str::from_utf8(b).unwrap().into())
}
fn number(s: &str, key: &str) -> u64 {
    let pattern = alloc::format!("\"{}\":", key);
    let start = s.find(&pattern).unwrap() + pattern.len();
    let end = s[start..]
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len() - start);
    s[start..start + end].parse().unwrap()
}
pub fn show() {
    let info = read(0, 0).expect("world unavailable");
    let info_line = alloc::format!("LIGHTCONE_INFO {}\n", info);
    assert!(rt::try_print(&info_line));
    let latest = number(&info, "latest");
    let first = number(&info, "oldest");
    let mut retrieved = 0;
    let mut emitted = 0;
    let mut rejected = 0;
    for id in first..=latest {
        if let Some(r) = read(1, id) {
            retrieved += 1;
            let line = alloc::format!("LIGHTCONE_RECEIPT {}\n", r);
            if rt::try_print(&line) {
                emitted += 1;
            } else {
                rejected += 1;
            }
        }
    }
    assert!(rt::try_print(&alloc::format!(
        "LIGHTCONE_SHOW retrieved={} emitted={} rejected={}\n",
        retrieved,
        emitted,
        rejected
    )));
}
pub fn run() {
    assert!(read(0, 0).is_some());
    assert!(read(99, 0).is_none());
    assert!(read(1, u64::MAX).is_none());
    assert!(read(2, u64::MAX).is_none());
    rt::print("LIGHTCONE_READY\n");
    for _ in 0..300 {
        rt::call3(SYS_NET, 3, 0, 0);
        rt::sleep(1);
    }
    let payload = b"causal network probe";
    let mut msg = [0u8; 64];
    msg[..8].copy_from_slice(&[10, 0, 2, 2, 0x23, 0x28, 0x23, 0x29]);
    msg[8..8 + payload.len()].copy_from_slice(payload);
    assert_eq!(
        rt::call3(SYS_NET, 6, msg.as_ptr() as u64, (8 + payload.len()) as u64),
        1
    );
    for _ in 0..10 {
        rt::sleep(1)
    }
    show();
    let info = read(0, 0).unwrap();
    assert!(number(&info, "rx") >= 5);
    assert!(number(&info, "tx") >= 1);
    assert_eq!(number(&info, "failures"), 0);
    for sub in [2, 3, 4] {
        rt::print(&alloc::format!(
            "LIGHTCONE_METADATA {} {}\n",
            sub,
            read(sub, 0).unwrap()
        ));
    }
    let latest = number(&info, "latest");
    assert_eq!(read(1, latest), read(1, latest));
    rt::print("LIGHTCONE_KERNEL_OK\n");
}

/// Test instrumentation only: observe which inputs the existing UDP stack delivers.
pub fn ingress_audit() {
    assert_eq!(rt::call3(SYS_NET, 5, 9100, 0), 1);
    rt::print("AUDIT_INGRESS_READY\n");
    for _ in 0..600 {
        rt::call3(SYS_NET, 3, 0, 0);
        let n = rt::call3(SYS_NET, 7, 9100, 0);
        if n != ERROR {
            assert!(n <= 512);
            let b = unsafe { core::slice::from_raw_parts(LIGHTCONE_PAGE as *const u8, n as usize) };
            let s = core::str::from_utf8(b).expect("audit sends ASCII only");
            rt::print("AUDIT_DELIVERED ");
            rt::print(s);
            rt::print("\n");
        }
        rt::sleep(1);
    }
    show();
    rt::print("AUDIT_INGRESS_DONE\n");
}

/// Verify the ledger through its read-only ABI and emit an honest compact
/// summary. Full JSON remains available on LIGHTCONE_PAGE; the diagnostic does
/// not bypass or disable the independent console egress policy.
fn audit_dump() {
    let info = read(0, 0).unwrap();
    rt::print(&alloc::format!("AUDIT_LEDGER_INFO {}\n", info));
    let first = number(&info, "oldest");
    let latest = number(&info, "latest");
    let mut retrieved = 0;
    let mut verified = 0;
    for id in first..=latest {
        if let Some(body) = read(1, id) {
            retrieved += 1;
            if read(1, id).as_ref() == Some(&body) {
                verified += 1;
            }
        }
    }
    assert!(rt::try_print(&alloc::format!(
        "AUDIT_LEDGER retrieved={} verified={} first={} latest={}\n",
        retrieved,
        verified,
        first,
        latest
    )));
}

/// Test instrumentation only: sustained actual outbound datagrams and ledger reads.
pub fn soak_audit() {
    rt::print("AUDIT_SOAK_READY\n");
    let mut free = [0u64; 3];
    for round in 0..3 {
        for sample in 0..256 {
            let payload = alloc::format!("soak round {} sample {}", round, sample);
            let mut msg = [0u8; 96];
            msg[..8].copy_from_slice(&[10, 0, 2, 2, 0x23, 0x8e, 0x23, 0x8d]);
            msg[8..8 + payload.len()].copy_from_slice(payload.as_bytes());
            assert_eq!(
                rt::call3(SYS_NET, 6, msg.as_ptr() as u64, (8 + payload.len()) as u64),
                1
            );
            rt::sleep(1);
        }
        for _ in 0..20 {
            rt::sleep(1);
        }
        let info = read(0, 0).unwrap();
        assert_eq!(number(&info, "failures"), 0);
        let latest = number(&info, "latest");
        assert!(read(1, 1).is_none());
        assert!(read(1, latest + 1).is_none());
        assert_eq!(read(1, latest), read(1, latest));
        free[round] = rt::call(SYS_FREE_FRAMES, 0, 0);
        rt::print(&alloc::format!(
            "AUDIT_ROUND {} free={} {}\n",
            round,
            free[round],
            info
        ));
    }
    assert_eq!(free[1], free[2]);
    audit_dump();
    rt::print("AUDIT_SOAK_DONE\n");
}
