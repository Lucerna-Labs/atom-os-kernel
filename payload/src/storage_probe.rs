//! Explicit diagnostics for isolated VM disks. All I/O uses public syscalls.
use alloc::{format, string::String, vec::Vec};
use user_rt::{self as rt, abi::*};

fn names(nonce: &str) -> Vec<String> {
    (0..8).map(|n| format!("r-{}-{}.bin", nonce, n)).collect()
}
fn length(names: &[String], index: usize) -> usize {
    if index < 7 { 65536 } else { 524288 - 4 - names.iter().map(|n| 6 + n.len()).sum::<usize>() - 7 * 65536 }
}
fn value(nonce: &str, file: usize, offset: usize, next: bool) -> u8 {
    ((offset * 17 + file * 43 + nonce.as_bytes()[file % nonce.len()] as usize + if next { 101 } else { 0 }) % 251) as u8
}
fn inspect(nonce: &str, paths: &[String]) -> Option<bool> {
    if rt::fs_stat(0) != 8 || rt::fs_stat(2) != 524288 { return None; }
    let mut old = true; let mut new = true;
    for (index, name) in paths.iter().enumerate() {
        let fd = rt::open_existing(name); if fd == ERROR { return None; }
        let mut exact_length = true;
        for offset in 0..length(paths, index) {
            if let Some(byte) = rt::read(fd) {
                old &= byte == value(nonce, index, offset, false);
                new &= byte == value(nonce, index, offset, true);
            } else { exact_length = false; break; }
        }
        if rt::read(fd).is_some() { exact_length = false; }
        rt::close(fd);
        if !exact_length { return None; }
    }
    if old { Some(false) } else if new { Some(true) } else { None }
}
pub fn run(arguments: &str) {
    let words: Vec<_> = arguments.split_whitespace().collect();
    if words.len() != 2 || words[1].len() != 16 || !words[1].bytes().all(|b| b.is_ascii_hexdigit()) {
        rt::print("usage: storageprobe seed|mutate|verify 16-hex-digit-nonce\n"); return;
    }
    let nonce = words[1]; let paths = names(nonce);
    match words[0] {
        "verify" => match inspect(nonce, &paths) {
            Some(next) => rt::print_args(format_args!("STORAGE_VERIFY_OK phase={} files=8 bytes=524288 dirty={}\n",
                if next { "new" } else { "old" }, rt::fs_stat(4))),
            None => rt::print("STORAGE_VERIFY_FAILED\n"),
        },
        "seed" | "mutate" => {
            let next = words[0] == "mutate";
            if rt::fs_stat(10) != 1 || (!next && rt::fs_stat(0) != 0) || (next && inspect(nonce, &paths) != Some(false)) {
                rt::print("STORAGE_PROBE_REFUSED unexpected filesystem contents\n"); return;
            }
            for (index, name) in paths.iter().enumerate() {
                let size = length(&paths, index);
                let bytes: Vec<_> = (0..size).map(|offset| value(nonce, index, offset, next)).collect();
                let fd = if next { rt::open_existing(name) } else { rt::open(name) };
                if fd == ERROR || !rt::replace(fd, &bytes) {
                    rt::print_args(format_args!("STORAGE_PROBE_FAILED {}\n", fs_error_message(rt::fs_error())));
                    if fd != ERROR { rt::close(fd); } return;
                }
                rt::close(fd);
            }
            rt::print_args(format_args!("STORAGE_{}_READY files={} bytes={}\n", if next { "NEXT" } else { "SEED" }, rt::fs_stat(0), rt::fs_stat(2)));
        }
        _ => rt::print("unknown storage probe mode\n"),
    }
}
