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
    rt::print(&alloc::format!("LIGHTCONE_INFO {}\n", info));
    let latest = number(&info, "latest");
    let first = number(&info, "oldest");
    for id in first..=latest {
        if let Some(r) = read(1, id) {
            rt::print(&alloc::format!("LIGHTCONE_RECEIPT {}\n", r));
        }
    }
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
