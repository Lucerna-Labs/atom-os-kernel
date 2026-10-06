//! The conversation a syscall is: who it addresses, read off the ABI.
//!
//! The shadow web (`kernel-sense`) learns WHO TALKS TO WHOM. The sensor is
//! inert: it scars whatever (identity, target, syscall) triple it is handed.
//! What the target *means* is ABI knowledge, which belongs here at the edge:
//! a pid for the pid-addressed calls, the sub-function for the multiplexed
//! calls, and nothing (0) for calls whose first argument is a pointer, a
//! length, a handle or a byte, which name no partner and would otherwise land
//! the same conversation on a different site every call.
//!
//! A pure table, no state: the root atom is `compare` (which call is this),
//! and the mapping is the only policy, kept out of the sensor.

use kernel_kit::abi::*;

/// Identity under which conversations are keyed when the site should belong
/// to the conversation itself rather than to one program: every program
/// shares the kernel's learned vocabulary. (Never 0, which the sensor reads
/// as "use the pid".)
pub const SHARED_IDENTITY: u64 = 1;

/// The partner a syscall addresses, from its number and first argument.
pub fn partner(number: u64, arg: u64) -> u64 {
    match number {
        // Pid-addressed: the partner is the other process.
        SYS_IPC_SEND | SYS_KILL | SYS_WAIT => arg,
        // Multiplexed control surfaces: the sub-function is the conversation.
        SYS_SENSE | SYS_KEY | SYS_INSTANT | SYS_KEYLANE | SYS_TAINT | SYS_CRYPT | SYS_NET | SYS_VGA | SYS_FS_STAT => arg,
        // Pointers, lengths, handles, bytes and ticks name no partner.
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_addressed_calls_name_their_partner() {
        assert_eq!(partner(SYS_IPC_SEND, 7), 7);
        assert_eq!(partner(SYS_KILL, 9), 9);
        assert_eq!(partner(SYS_WAIT, 3), 3);
    }

    #[test]
    fn multiplexed_calls_name_their_sub_function() {
        assert_eq!(partner(SYS_SENSE, 13), 13);
        assert_eq!(partner(SYS_TAINT, 2), 2);
    }

    #[test]
    fn pointer_and_length_calls_name_nobody() {
        for (number, arg) in [(SYS_WRITE, b'x' as u64), (SYS_ALLOC, 4096), (SYS_FILE_WRITE, 3),
                              (SYS_WRITE_BUFFER, 0x7f00_0000), (SYS_SLEEP, 5), (SYS_YIELD, 0)] {
            assert_eq!(partner(number, arg), 0, "syscall {number}");
        }
    }
}
