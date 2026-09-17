#![no_std]
#![no_main]
extern crate alloc;
use user_rt::{self as rt, abi::*};
user_rt::entry!(main);

fn read_exact(fd: u64, expected: &[u8]) {
    for &byte in expected { assert_eq!(rt::read(fd), Some(byte)); }
    assert_eq!(rt::read(fd), None);
}
fn failure(ok: bool, expected: FsError) {
    assert!(!ok); assert_eq!(rt::fs_error(), expected as u64);
}
fn main() {
    if rt::fs_stat(0) != 0 || rt::fs_stat(10) != 1 {
        rt::print("FSTEST_REQUIRES_EMPTY_FILESYSTEM_AND_DISK\n"); rt::exit(2);
    }
    let held = rt::open("held"); assert_ne!(held, ERROR);
    assert!(rt::write(held, b"original"));
    assert!(rt::rename("held", "renamed")); assert!(rt::remove("renamed"));
    let replacement = rt::open("renamed"); assert_ne!(replacement, ERROR);
    assert!(rt::write(replacement, b"new")); assert!(rt::write(held, b" detached"));
    read_exact(held, b"original detached"); read_exact(replacement, b"new");
    rt::close(held); assert_eq!(rt::fs_stat(6), 4);
    assert!(rt::remove("renamed")); rt::close(replacement); assert_eq!(rt::fs_stat(6), 0);
    let builtin = rt::open("shell.elf");
    failure(rt::write(builtin, b"x"), FsError::ReadOnly); rt::close(builtin);
    failure(rt::remove("shell.elf"), FsError::ReadOnly);
    failure(rt::rename("shell.elf", "changed"), FsError::ReadOnly);
    let a = rt::open("a"); let b = rt::open("b");
    assert!(rt::write(a, b"a")); assert!(rt::write(b, b"b"));
    failure(rt::rename("a", "b"), FsError::Exists);
    read_exact(a, b"a"); read_exact(b, b"b");
    assert_eq!(rt::call3(SYS_WRITE_FILE_BUFFER, a, 0x200000, 1), ERROR);
    assert_eq!(rt::fs_error(), FsError::BadBuffer as u64);
    rt::close(a); rt::close(b); assert!(rt::remove("a")); assert!(rt::remove("b"));
    rt::print("FILE_LIFECYCLE_OK\n");

    for n in 0..128 {
        let name = alloc::format!("n{}", n); let fd = rt::open(&name);
        assert_ne!(fd, ERROR); rt::close(fd);
    }
    assert_eq!(rt::fs_stat(0), 128);
    assert_eq!(rt::open("overflow"), ERROR); assert_eq!(rt::fs_error(), FsError::NoSpace as u64);
    assert!(rt::remove("n0")); let fd = rt::open("replacement"); assert_ne!(fd, ERROR); rt::close(fd);
    for n in 1..128 { assert!(rt::remove(&alloc::format!("n{}", n))); }
    assert!(rt::remove("replacement")); assert_eq!(rt::fs_stat(0), 0);

    let bytes = alloc::vec![0x5a; 65536];
    for n in 0..7 {
        let fd = rt::open(&alloc::format!("cap{}", n)); assert_ne!(fd, ERROR);
        assert!(rt::write(fd, &bytes)); rt::close(fd);
    }
    let tail = rt::open("tail"); assert_ne!(tail, ERROR);
    let remaining = (rt::fs_stat(3) - rt::fs_stat(2)) as usize;
    assert!(remaining < bytes.len()); assert!(rt::write(tail, &bytes[..remaining]));
    assert_eq!(rt::fs_stat(2), rt::fs_stat(3));
    assert_eq!(rt::call(SYS_SYNC, 0, 0), 0); assert_eq!(rt::fs_stat(4), 0);
    failure(rt::write(tail, b"x"), FsError::NoSpace);
    failure(rt::replace(tail, &bytes), FsError::NoSpace);
    failure(rt::rename("tail", "tail-longer"), FsError::NoSpace);
    assert_eq!(rt::fs_stat(4), 0);
    read_exact(tail, &bytes[..remaining]);
    assert!(rt::remove("cap0")); assert!(rt::write(tail, b"x"));
    for n in 1..7 { assert!(rt::remove(&alloc::format!("cap{}", n))); }
    assert!(rt::remove("tail")); rt::close(tail);
    assert_eq!(rt::fs_stat(0), 0); assert_eq!(rt::fs_stat(2), 4); assert_eq!(rt::fs_stat(6), 0);
    assert_eq!(rt::fs_stat(4), 1); assert_eq!(rt::call(SYS_SYNC, 0, 0), 0); assert_eq!(rt::fs_stat(4), 0);
    rt::print("DURABLE_QUOTAS_OK\nFS_STATE_OK\n");
}
