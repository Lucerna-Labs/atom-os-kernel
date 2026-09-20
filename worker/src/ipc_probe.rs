//! Real ring-3 checks for the bounded receive API and its existing ABI.
use user_rt::{self as rt, abi::*, ipc::{MAX_MESSAGE_BYTES, ReceiveError}};

pub fn run() {
    let pid = rt::call(SYS_GETPID, 0, 0);
    let mut first = [0xa5; MAX_MESSAGE_BYTES];
    let mut second = [0xa5; MAX_MESSAGE_BYTES];
    assert_eq!(rt::receive_into(&mut first), Ok(None));
    assert!(rt::send(pid, "queued message"));
    for size in [0, 1, MAX_MESSAGE_BYTES - 1] {
        assert_eq!(rt::receive_into(&mut first[..size]),
            Err(ReceiveError::BufferTooSmall { required: MAX_MESSAGE_BYTES }));
    }
    assert!(first.iter().all(|&b| b == 0xa5));
    let len = rt::receive_into(&mut first).unwrap().unwrap();
    assert_eq!(&first[..len], b"queued message");
    assert!(first[len..].iter().all(|&b| b == 0xa5));
    rt::print("IPC_SHORT_BUFFER_PRESERVES_QUEUE_OK\n");

    assert!(rt::send(pid, ""));
    assert_eq!(rt::receive_into(&mut second), Ok(Some(0)));
    let mut maximum = [b'a'; MAX_MESSAGE_BYTES];
    for index in (15..MAX_MESSAGE_BYTES).step_by(16) { maximum[index] = b' '; }
    maximum[MAX_MESSAGE_BYTES - 2] = 0xc3; maximum[MAX_MESSAGE_BYTES - 1] = 0xa9;
    let text = core::str::from_utf8(&maximum).unwrap();
    assert!(rt::send(pid, text));
    assert_eq!(rt::receive_into(&mut second), Ok(Some(MAX_MESSAGE_BYTES)));
    assert_eq!(second, maximum);
    assert_eq!(&first[..len], b"queued message"); // receive page was reused
    assert_eq!(rt::receive_into(&mut second), Ok(None));
    rt::print("IPC_EMPTY_MAXIMUM_UTF8_OWNERSHIP_OK\n");

    for text in ["first", "second", "third", "fourth"] { assert!(rt::send(pid, text)); }
    assert!(!rt::send(pid, "fifth"));
    for text in ["first", "second", "third", "fourth"] {
        let len = rt::receive_into(&mut second).unwrap().unwrap();
        assert_eq!(&second[..len], text.as_bytes());
    }
    assert_eq!(rt::receive_into(&mut second), Ok(None));
    rt::print("IPC_FIFO_BACKPRESSURE_OK\n");

    let before = rt::call(SYS_FREE_FRAMES, 0, 0);
    for _ in 0..32 {
        assert!(rt::send(pid, "bounded reuse"));
        let len = rt::receive_into(&mut second).unwrap().unwrap();
        assert_eq!(&second[..len], b"bounded reuse");
    }
    let after = rt::call(SYS_FREE_FRAMES, 0, 0);
    assert_eq!(before, after);
    assert!(rt::send(pid, "compatibility"));
    let owned = rt::receive().unwrap();
    assert!(rt::send(pid, "later"));
    assert_eq!(rt::receive_into(&mut second), Ok(Some(5)));
    assert_eq!(owned, "compatibility");
    assert_eq!(rt::receive_into(&mut second), Ok(None));
    drop(owned);
    rt::print_args(format_args!("IPC_BUFFER_OK rounds=32 free_before={} free_after={}\n", before, after));
}
