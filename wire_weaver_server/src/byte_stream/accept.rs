//! What every source that accepts connections (TCP, a local socket, on any event loop) does with errors.

use std::io;
use std::time::Duration;

/// How long a source waits before it accepts again after the system ran out of descriptors or memory.
pub const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// The system ran out of something a later accept may find again: a source waits [ACCEPT_RETRY_DELAY] and accepts
/// again instead of ending.
pub fn is_transient(e: &io::Error) -> bool {
    #[cfg(unix)]
    if let Some(code) = e.raw_os_error()
        && [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM].contains(&code)
    {
        return true;
    }
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionAborted | io::ErrorKind::Interrupted | io::ErrorKind::OutOfMemory
    )
}
