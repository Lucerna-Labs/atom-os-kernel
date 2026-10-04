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
/// Filesystem acceptance in ring 3: file lifecycle and errors, folders and the
/// working folder, and the saved state. Works inside its own folder, so files
/// other programs keep (boot.done, user data) do not matter.
fn main() {
    if rt::fs_stat(10) != 1 { rt::print("FSTEST_REQUIRES_DISK\n"); rt::exit(2); }
    let _ = rt::remove("/fsprobe/sub/inner"); let _ = rt::remove("/fsprobe/sub"); let _ = rt::remove("/fsprobe");
    rt::mkdir("/fsprobe").unwrap();
    rt::chdir("/fsprobe").unwrap();
    assert_eq!(rt::cwd(), "/fsprobe");

    // Lifecycle: an open file can be renamed but not removed.
    let held = rt::open("held"); assert_ne!(held, ERROR);
    assert!(rt::write(held, b"original"));
    assert!(rt::rename("held", "renamed"));
    failure(rt::remove("renamed"), FsError::Busy);
    assert!(rt::write(held, b" kept"));
    rt::close(held);
    let again = rt::open_existing("/fsprobe/renamed"); assert_ne!(again, ERROR);
    read_exact(again, b"original kept"); rt::close(again);
    assert!(rt::remove("renamed"));
    failure(rt::open_existing("renamed") != ERROR, FsError::NotFound);
    assert!(rt::stat("renamed").is_err(), "opening a missing file for reading never creates it");

    // Built-in programs are read-only.
    let builtin = rt::open("/bin/shell.elf"); assert_ne!(builtin, ERROR);
    failure(rt::write(builtin, b"x"), FsError::ReadOnly); rt::close(builtin);
    failure(rt::remove("/bin/shell.elf"), FsError::ReadOnly);
    failure(rt::rename("/bin/shell.elf", "changed"), FsError::ReadOnly);

    // A rename onto an existing name changes nothing.
    let a = rt::open("a"); let b = rt::open("b");
    assert!(rt::write(a, b"a")); assert!(rt::write(b, b"b"));
    failure(rt::rename("a", "b"), FsError::Exists);
    read_exact(a, b"a"); read_exact(b, b"b");
    // Bad buffers and oversized single calls are refused with a reason.
    assert_eq!(rt::call3(SYS_WRITE_FILE_BUFFER, a, 0x200000, 1), ERROR);
    assert_eq!(rt::fs_error(), FsError::BadBuffer as u64);
    assert_eq!(rt::call3(SYS_REPLACE_FILE, a, 0x200000, FILE_IO_MAX as u64 + 1), ERROR);
    assert_eq!(rt::fs_error(), FsError::FileTooLarge as u64);
    rt::close(a); rt::close(b); assert!(rt::remove("a")); assert!(rt::remove("b"));
    // Replace swaps the whole contents and rewinds the descriptor.
    let r = rt::open("replaced"); assert_ne!(r, ERROR);
    assert!(rt::write(r, b"long first contents")); assert!(rt::replace(r, b"short"));
    read_exact(r, b"short"); rt::close(r); assert!(rt::remove("replaced"));
    rt::print("FILE_LIFECYCLE_OK\n");

    // Folders, relative paths and the working folder.
    rt::mkdir("sub").unwrap();
    failure(rt::mkdir("missing/child").is_ok(), FsError::NotFound);
    rt::chdir("sub").unwrap();
    assert_eq!(rt::cwd(), "/fsprobe/sub");
    let inner = rt::open("inner"); assert_ne!(inner, ERROR); assert!(rt::write(inner, b"deep")); rt::close(inner);
    rt::chdir("..").unwrap();
    let names: alloc::vec::Vec<_> = rt::read_dir("sub").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["inner"]);
    failure(rt::remove("sub"), FsError::NotEmpty);
    failure(rt::chdir("sub/inner").is_ok(), FsError::NotDir);
    let fd = rt::open_existing("./sub/../sub/inner"); assert_ne!(fd, ERROR); read_exact(fd, b"deep"); rt::close(fd);
    assert!(rt::remove("sub/inner")); assert!(rt::remove("sub"));
    rt::print("FOLDERS_CWD_OK\n");

    // The saved state: changes are unsaved until a sync commits them.
    rt::chdir("/").unwrap();
    assert!(rt::remove("/fsprobe"));
    assert_eq!(rt::fs_stat(4), 1);
    let generation = rt::fs_stat(5);
    assert_eq!(rt::call(SYS_SYNC, 0, 0), 0);
    assert_eq!(rt::fs_stat(4), 0);
    assert_eq!(rt::fs_stat(8), rt::fs_stat(9));
    assert_eq!(rt::fs_stat(5), generation + 1);
    rt::print("FS_STATE_OK\n");
}
