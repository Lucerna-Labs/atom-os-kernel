//! Explicit diagnostics for isolated VM disks. All I/O uses public syscalls.
//!
//! Eight 64 KiB files (16 disk blocks each, so a save writes each in one
//! virtio request) whose bytes depend on a nonce and a phase: `seed` writes
//! the old phase, `mutate` rewrites every file with the new phase, `verify`
//! reports which phase is on disk and fails on any mixture.
use alloc::{format, string::String, vec::Vec};
use user_rt::{self as rt, abi::*};

const FILES: usize = 8;
const FILE_BYTES: usize = 65536;

fn names(nonce: &str) -> Vec<String> {
    (0..FILES).map(|n| format!("/r-{}-{}.bin", nonce, n)).collect()
}
fn value(nonce: &str, file: usize, offset: usize, next: bool) -> u8 {
    ((offset * 17 + file * 43 + nonce.as_bytes()[file % nonce.len()] as usize + if next { 101 } else { 0 }) % 251) as u8
}
/// Some(false) when every file holds the old phase, Some(true) for the new one.
fn inspect(nonce: &str, paths: &[String]) -> Option<bool> {
    let mut old = true; let mut new = true;
    for (index, path) in paths.iter().enumerate() {
        let bytes = rt::read_file(path)?;
        if bytes.len() != FILE_BYTES { return None; }
        for (offset, &byte) in bytes.iter().enumerate() {
            old &= byte == value(nonce, index, offset, false);
            new &= byte == value(nonce, index, offset, true);
        }
    }
    if old { Some(false) } else if new { Some(true) } else { None }
}
pub fn run(arguments: &str) {
    let words: Vec<_> = arguments.split_whitespace().collect();
    if words.len() != 2 || words[1].len() != 16 || !words[1].bytes().all(|b| b.is_ascii_hexdigit()) {
        rt::print("usage: storageprobe seed|mutate|verify 16-hex-digit-nonce\n"); return;
    }
    let nonce = words[1]; let paths = names(nonce);
    let total = FILES * FILE_BYTES;
    match words[0] {
        "verify" => match inspect(nonce, &paths) {
            Some(next) => rt::print_args(format_args!("STORAGE_VERIFY_OK phase={} files={} bytes={} dirty={}\n",
                if next { "new" } else { "old" }, FILES, total, rt::fs_stat(4))),
            None => rt::print("STORAGE_VERIFY_FAILED\n"),
        },
        "seed" | "mutate" => {
            let next = words[0] == "mutate";
            let fresh = paths.iter().all(|path| rt::stat(path).is_err());
            if rt::fs_stat(10) != 1 || (!next && !fresh) || (next && inspect(nonce, &paths) != Some(false)) {
                rt::print("STORAGE_PROBE_REFUSED unexpected filesystem contents\n"); return;
            }
            for (index, path) in paths.iter().enumerate() {
                let bytes: Vec<_> = (0..FILE_BYTES).map(|offset| value(nonce, index, offset, next)).collect();
                let fd = if next { rt::open_existing(path) } else { rt::open(path) };
                if fd == ERROR || !rt::replace(fd, &bytes) {
                    rt::print_args(format_args!("STORAGE_PROBE_FAILED {}\n", fs_error_message(rt::fs_error())));
                    if fd != ERROR { rt::close(fd); } return;
                }
                rt::close(fd);
            }
            rt::print_args(format_args!("STORAGE_{}_READY files={} bytes={}\n", if next { "NEXT" } else { "SEED" }, FILES, total));
        }
        _ => rt::print("unknown storage probe mode\n"),
    }
}
