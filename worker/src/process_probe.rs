//! Process ABI acceptance exercised in ring 3 on the real kernel.
use alloc::{format, string::String, vec, vec::Vec};
use user_rt::{self as rt, abi::*};

fn pid() -> u64 { rt::call(SYS_GETPID, 0, 0) }
fn launch(args: &[&str]) -> u64 {
    let child = rt::spawn_args("worker.elf", args); assert_ne!(child, ERROR); child
}
fn state(child: u64, expected: u64) {
    for _ in 0..500 {
        if rt::processes().unwrap().iter().any(|p| p.pid == child && p.state == expected) { return; }
        rt::sleep(1);
    }
    panic!("pid {} never entered state {}", child, expected);
}
fn child_of(parent: u64) -> u64 {
    for _ in 0..500 {
        if let Some(p) = rt::processes().unwrap().iter().find(|p| p.parent == parent) { return p.pid; }
        rt::sleep(1);
    }
    panic!("missing child of {}", parent);
}

pub fn dispatch() {
    let args = rt::args(); assert_eq!(args[0], "worker.elf");
    if args.len() == 1 { return; }
    match args[1].as_str() {
        "--lightcone-ingress-audit" => { crate::lightcone_probe::ingress_audit(); rt::exit(0) }
        "--lightcone-soak-audit" => { crate::lightcone_probe::soak_audit(); rt::exit(0) }
        "--lightcone-test" => { crate::lightcone_probe::run(); rt::exit(0) }
        "--network-lightcone" => { crate::lightcone_probe::show(); rt::exit(0) }
        "--ipc-test" => { crate::ipc_probe::run(); rt::exit(0) }
        "--args" => {
            rt::print_args(format_args!("ARGS pid={} count={}\n", pid(), args.len()));
            for (index, arg) in args.iter().enumerate() {
                rt::print_args(format_args!("ARG {} len={} ", index, arg.len())); rt::print(arg); rt::print("\n");
            }
            rt::exit(41)
        }
        "--check-args" => {
            assert_eq!(&args[2..], &["two words", "", "quote\"", "slash\\", "é"]);
            rt::exit(41)
        }
        "--count" => { assert_eq!(args.len(), MAX_ARGS); assert!(args[2..].iter().all(|s| s.is_empty())); rt::exit(42) }
        "--exec-check" => {
            rt::exec_args("worker.elf", &["--exec-result", &format!("{}", pid()), "kept argument"]);
            panic!("exec returned")
        }
        "--exec-result" => {
            assert_eq!(args.len(), 4); assert_eq!(args[2].parse::<u64>().unwrap(), pid());
            assert_eq!(args[3], "kept argument"); rt::exit(43)
        }
        "--sleep" => loop { rt::sleep(10000); },
        "--spin" => loop { core::hint::spin_loop(); },
        "--selfkill" => { rt::kill(pid()); panic!("self kill returned") }
        "--hold" => {
            let buffer = vec![0x5a; 65536];
            let fd = rt::open("kill-held.txt"); assert_ne!(fd, ERROR);
            assert!(rt::replace(fd, &buffer)); assert!(rt::remove("kill-held.txt"));
            loop { rt::sleep(10000); core::hint::black_box(&buffer); }
        }
        "--wait-child" => {
            let child = launch(&["--sleep"]);
            assert_eq!(rt::wait(child), KILLED_STATUS); rt::exit(44)
        }
        "--process-test" => { test(&args); rt::exit(0) }
        _ if args.len() == 2 && args[1].len() == MAX_ARG_BYTES - "worker.elf".len() - 2 => {
            assert!(args[1].bytes().all(|b| b == b'x')); rt::exit(45)
        }
        _ => panic!("unknown worker arguments"),
    }
}

fn test(original: &[String]) {
    let name = b"worker.elf\0";
    let before_tasks = rt::call(SYS_TASK_COUNT, 0, 0);
    let before_frames = rt::call(SYS_FREE_FRAMES, 0, 0);
    // Kernel rejects bad pointers, short/readonly output buffers and malformed
    // packed arguments before creating or replacing any process.
    let bytes = [b'x'; MAX_ARG_BYTES + 1];
    for number in [SYS_SPAWN_ARGS, SYS_EXEC_ARGS] {
        for (ptr, len) in [(0, 1), (u64::MAX, 2), (0x200000, 2),
                           (bytes.as_ptr() as u64, 1), (bytes.as_ptr() as u64, bytes.len() as u64)] {
            assert_eq!(rt::call3(number, name.as_ptr() as u64, ptr, len), ERROR);
        }
        let bad_utf8 = [255u8, 0];
        assert_eq!(rt::call3(number, name.as_ptr() as u64, bad_utf8.as_ptr() as u64, 2), ERROR);
        let too_many = [0u8; MAX_ARGS];
        assert_eq!(rt::call3(number, name.as_ptr() as u64, too_many.as_ptr() as u64, MAX_ARGS as u64), ERROR);
    }
    assert_eq!(rt::exec_args("absent.elf", &["unchanged"]), ERROR);
    assert_eq!(rt::args(), original);
    for number in [SYS_ARGS, SYS_PROCESSES] {
        let len = if number == SYS_ARGS { MAX_ARG_BYTES as u64 } else { MAX_PROCESSES as u64 };
        assert_eq!(rt::call(number, 0, len), ERROR);
        assert_eq!(rt::call(number, u64::MAX, len), ERROR);
        assert_eq!(rt::call(number, name.as_ptr() as u64, len), ERROR);
        assert_eq!(rt::call(number, bytes.as_ptr() as u64, 0), ERROR);
    }
    assert_eq!(rt::call(SYS_TASK_COUNT, 0, 0), before_tasks);
    assert_eq!(rt::call(SYS_FREE_FRAMES, 0, 0), before_frames);
    let page = rt::call(SYS_ALLOC, 4096, 4096); assert_ne!(page, ERROR);
    unsafe { *((page + 4095) as *mut u8) = b'x'; }
    assert_eq!(rt::call3(SYS_SPAWN_ARGS, name.as_ptr() as u64, page + 4095, 2), ERROR);
    assert_eq!(rt::call(SYS_PROCESSES, page + 4095, MAX_PROCESSES as u64), ERROR);
    assert_eq!(rt::call(SYS_ARGS, page + 4095, MAX_ARG_BYTES as u64), ERROR);
    assert_eq!(rt::call(SYS_FREE, page, 0), 0);
    rt::print("PROCESS_INPUT_VALIDATION_OK\n");

    assert_eq!(rt::wait(launch(&["--check-args", "two words", "", "quote\"", "slash\\", "é"])), 41);
    let mut many = [""; MAX_ARGS - 1]; many[0] = "--count";
    assert_eq!(rt::wait(launch(&many)), 42);
    let longest = "x".repeat(MAX_ARG_BYTES - "worker.elf".len() - 2);
    assert_eq!(rt::wait(launch(&[&longest])), 45);
    assert_eq!(rt::spawn_args("worker.elf", &[&(longest + "x")]), ERROR);
    assert_eq!(rt::wait(launch(&["--exec-check"])), 43);
    rt::print("ARGUMENT_LIMITS_EXEC_OK\n");

    assert!(!rt::kill(0)); assert!(!rt::kill(u64::MAX));
    assert_eq!(rt::wait(launch(&["--selfkill"])), KILLED_STATUS);
    let sleeper = launch(&["--sleep"]); state(sleeper, PROCESS_SLEEPING);
    assert!(rt::kill(sleeper)); assert!(!rt::kill(sleeper));
    let zombie = rt::processes().unwrap().into_iter().find(|p| p.pid == sleeper).unwrap();
    assert_eq!(zombie.state, PROCESS_EXITED); assert_eq!(zombie.exit_code, KILLED_STATUS);
    assert_eq!(rt::wait(sleeper), KILLED_STATUS); assert_eq!(rt::wait(sleeper), ERROR);
    let spinner = launch(&["--spin"]); rt::sleep(2);
    assert!(rt::kill(spinner)); assert_eq!(rt::wait(spinner), KILLED_STATUS);
    // Wake a parent blocked in wait when a different process kills its child.
    let parent = launch(&["--wait-child"]); let child = child_of(parent);
    state(parent, PROCESS_WAITING); assert!(rt::kill(child)); assert_eq!(rt::wait(parent), 44);
    // Killing a blocked parent must orphan its child without reviving the parent.
    let parent = launch(&["--wait-child"]); let child = child_of(parent);
    state(parent, PROCESS_WAITING); assert!(rt::kill(parent)); assert_eq!(rt::wait(parent), KILLED_STATUS);
    assert_eq!(rt::processes().unwrap().iter().find(|p| p.pid == child).unwrap().parent, 0);
    assert!(rt::kill(child)); rt::sleep(2);
    assert!(!rt::processes().unwrap().iter().any(|p| p.pid == child));
    rt::print("KILL_WAIT_ORPHAN_OK\n");

    // Warm all mappings before measuring kill/reap churn with open unlinked
    // files and live heap allocations, including slot reuse beyond 16 tasks.
    let child = launch(&["--hold"]); state(child, PROCESS_SLEEPING);
    assert!(rt::kill(child)); assert_eq!(rt::wait(child), KILLED_STATUS); rt::sleep(2);
    let before = rt::call(SYS_FREE_FRAMES, 0, 0); let buffers = rt::fs_stat(6);
    for _ in 0..48 {
        let child = launch(&["--hold"]); state(child, PROCESS_SLEEPING);
        assert!(rt::kill(child)); assert_eq!(rt::wait(child), KILLED_STATUS);
    }
    rt::sleep(2);
    let after = rt::call(SYS_FREE_FRAMES, 0, 0);
    assert_eq!(before, after); assert_eq!(rt::fs_stat(6), buffers);
    assert_eq!(rt::call(SYS_TASK_COUNT, 0, 0), before_tasks);
    rt::print_args(format_args!("KILL_REAP_OK rounds=48 free_before={} free_after={}\n", before, after));
}
