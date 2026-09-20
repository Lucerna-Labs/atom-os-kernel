//! Owned, bounded IPC copies. Inspired by atom-os-system's ipc_recv_into;
//! the current ABI supplies at most 255 UTF-8 bytes plus a NUL terminator.
pub const MAX_MESSAGE_BYTES: usize = 255;
pub(crate) const RECEIVE_BYTES: usize = MAX_MESSAGE_BYTES + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveError {
    /// Checked before receiving: the pending message remains queued.
    BufferTooSmall { required: usize },
    Syscall,
    MalformedMessage,
}

pub(crate) fn check_capacity(buffer: &[u8]) -> Result<(), ReceiveError> {
    if buffer.len() < MAX_MESSAGE_BYTES {
        Err(ReceiveError::BufferTooSmall { required: MAX_MESSAGE_BYTES })
    } else { Ok(()) }
}

pub(crate) fn copy_message(source: &[u8], buffer: &mut [u8]) -> Result<usize, ReceiveError> {
    check_capacity(buffer)?;
    let end = source.iter().take(RECEIVE_BYTES).position(|&b| b == 0)
        .ok_or(ReceiveError::MalformedMessage)?;
    buffer[..end].copy_from_slice(&source[..end]);
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_capacity_message_keeps_every_byte_and_leaves_the_tail_untouched() {
        let mut page = [b'x'; RECEIVE_BYTES]; page[MAX_MESSAGE_BYTES] = 0;
        let mut out = [0xa5; 300];
        assert_eq!(copy_message(&page, &mut out), Ok(MAX_MESSAGE_BYTES));
        assert_eq!(&out[..MAX_MESSAGE_BYTES], &page[..MAX_MESSAGE_BYTES]);
        assert!(out[MAX_MESSAGE_BYTES..].iter().all(|&b| b == 0xa5));
    }
    #[test]
    fn short_buffers_are_rejected_without_partial_data() {
        for capacity in [0, 1, MAX_MESSAGE_BYTES - 1] {
            let mut out = [0xa5; MAX_MESSAGE_BYTES];
            assert_eq!(copy_message(b"hello\0", &mut out[..capacity]),
                Err(ReceiveError::BufferTooSmall { required: MAX_MESSAGE_BYTES }));
            assert!(out.iter().all(|&b| b == 0xa5));
        }
    }
    #[test]
    fn missing_terminator_is_rejected_without_reading_past_the_abi_bound() {
        let mut page = [b'x'; RECEIVE_BYTES + 1]; page[RECEIVE_BYTES] = 0;
        let mut out = [0xa5; MAX_MESSAGE_BYTES];
        assert_eq!(copy_message(&page, &mut out), Err(ReceiveError::MalformedMessage));
        assert_eq!(copy_message(b"", &mut out), Err(ReceiveError::MalformedMessage));
        assert!(out.iter().all(|&b| b == 0xa5));
    }
    #[test]
    fn empty_message_is_present_and_copy_survives_source_reuse() {
        let mut out = [0xa5; MAX_MESSAGE_BYTES];
        assert_eq!(copy_message(b"\0", &mut out), Ok(0));
        assert_eq!(out[0], 0xa5);
        let mut page = *b"first\0";
        assert_eq!(copy_message(&page, &mut out), Ok(5));
        page.fill(0);
        assert_eq!(&out[..5], b"first");
    }
}
