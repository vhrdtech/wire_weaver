//! How a mount reaches its backend: a byte stream it can open again (SRV-78).

use std::future::Future;
use std::io;

use futures_util::io::{AsyncRead, AsyncWrite};

/// Opens a byte stream to a mounted backend, a `wire_weaver_server` listening with preamble framing (a local socket,
/// TCP, an in-memory pipe in a test). Called for every link: the slot's control link and each front connection's
/// own, and again after a backend went away.
///
/// The stream is futures-io, so any runtime fits: [LocalSocket] for tokio (feature `local_socket`), `ww_calloop`'s
/// `UnixConnector` for a calloop event loop.
pub trait Connect {
    /// An open stream.
    type Stream: AsyncRead + AsyncWrite + Unpin;
    /// Opening one; it may fail at once (nobody listens) or after a while.
    type Opening: Future<Output = io::Result<Self::Stream>>;

    /// Starts opening a stream.
    fn connect(&self) -> Self::Opening;
}

/// A backend listening on a local socket of the same user (a Unix socket, a named pipe on Windows), on tokio.
#[cfg(feature = "local_socket")]
#[derive(Clone, Debug)]
pub struct LocalSocket {
    path: std::path::PathBuf,
}

#[cfg(feature = "local_socket")]
impl LocalSocket {
    /// The socket at `path`; a process of another user listening there is refused.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        LocalSocket { path: path.into() }
    }
}

#[cfg(feature = "local_socket")]
impl Connect for LocalSocket {
    type Stream = ww_local_socket::Stream;
    type Opening = futures_util::future::BoxFuture<'static, io::Result<Self::Stream>>;

    fn connect(&self) -> Self::Opening {
        let path = self.path.clone();
        Box::pin(async move { Ok(ww_local_socket::connect(&path).await?.0) })
    }
}
